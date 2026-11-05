//! Bandwidth limits (CORE-06, ADR 0024): token buckets for the global budget and for each
//! download.
//!
//! - **Unit:** bytes per second. `None` (stored as 0) means unlimited, and an unlimited bucket
//!   costs one atomic load per charge: no lock, no clock read.
//! - **Burst:** the capacity is `max(rate × 250 ms, 1 KiB)`. From 4,096 B/s up a long idle period
//!   allows exactly a quarter second of burst; at the 1,024 B/s minimum, one second.
//! - **Large charges** are taken in capacity-sized parts, waiting for each; a charge never
//!   overdraws, and no debt is carried.
//! - **Live changes:** `set_rate` swaps the rate atomically, clamps the stored tokens to the new
//!   capacity and wakes every waiter, which recomputes its deadline.
//! - **Cancellation:** a charge is a plain future; dropping it (the caller's stop/halt select)
//!   ends the wait at once. Tokens already taken for a part stay spent, which can only under-use a
//!   budget, never exceed it.
//! - **Coupling:** [`Rates::charge`] takes from the download's bucket, then from the global one,
//!   never reserving one while waiting on the other beyond the part being charged.
//!
//! Waiting for tokens is never a network timeout: the engine owns its per-read timeout
//! (`transfer::within`) and runs it only while it waits for the network. reqwest's own
//! `read_timeout` is not used: it keeps its deadline running while the body is not polled, so a
//! bandwidth wait failed as NetworkOffline (found by tests/speed_limits.rs S05).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::watch;
use tokio::time::{Duration, Instant};

/// Smallest accepted limit (ADR 0024): below it the burst floor would dominate.
pub const MIN_BYTES_PER_SECOND: u64 = 1024;
/// Largest accepted limit (1 TB/s); larger values are refused rather than meaning "unlimited".
pub const MAX_BYTES_PER_SECOND: u64 = 1_000_000_000_000;
const MIN_CAPACITY: u64 = 1024;

/// Bucket capacity for `rate`: a quarter second of the rate, at least 1 KiB.
pub fn capacity(rate: u64) -> u64 {
    (rate / 4).max(MIN_CAPACITY)
}

/// `Some(rate)` within the documented bounds, or why not.
pub fn validate(rate: Option<u64>) -> Result<Option<u64>, &'static str> {
    match rate {
        None => Ok(None),
        Some(r) if r < MIN_BYTES_PER_SECOND => {
            Err("a speed limit must be at least 1024 bytes per second")
        }
        Some(r) if r > MAX_BYTES_PER_SECOND => {
            Err("a speed limit must be at most 1000000000000 bytes per second")
        }
        Some(r) => Ok(Some(r)),
    }
}

#[derive(Debug)]
struct State {
    tokens: f64,
    last: Instant,
}

/// One token bucket. Shared through `Arc` by every writer it limits.
#[derive(Debug)]
pub struct Bucket {
    /// Bytes per second; 0 = unlimited.
    rate: AtomicU64,
    state: Mutex<State>,
    changed: watch::Sender<u64>,
}

fn lock(m: &Mutex<State>) -> std::sync::MutexGuard<'_, State> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Bucket {
    /// A bucket at `rate`, starting empty (a new or resumed transfer gets no initial burst).
    pub fn new(rate: Option<u64>) -> Self {
        let (changed, _) = watch::channel(0);
        Self {
            rate: AtomicU64::new(rate.unwrap_or(0)),
            state: Mutex::new(State {
                tokens: 0.0,
                last: Instant::now(),
            }),
            changed,
        }
    }

    /// The current limit (`None` = unlimited).
    pub fn rate(&self) -> Option<u64> {
        match self.rate.load(Ordering::Acquire) {
            0 => None,
            r => Some(r),
        }
    }

    /// The largest part one charge takes at once, or `None` when unlimited.
    pub fn part_limit(&self) -> Option<u64> {
        self.rate().map(capacity)
    }

    /// Changes the limit now and wakes every waiter to recompute its deadline.
    pub fn set_rate(&self, rate: Option<u64>) {
        let mut s = lock(&self.state);
        let now = Instant::now();
        // Settle the old rate's refill up to now before switching.
        if let Some(old) = self.rate() {
            refill(&mut s, old, now);
        } else {
            s.last = now;
        }
        self.rate.store(rate.unwrap_or(0), Ordering::Release);
        if let Some(new) = rate {
            // The burst bound holds for the new rate at once.
            s.tokens = s.tokens.min(capacity(new) as f64);
        } else {
            s.tokens = 0.0;
        }
        s.last = now;
        drop(s);
        self.changed.send_modify(|g| *g = g.wrapping_add(1));
    }

    /// Waits until `n` bytes may be written, then takes them. Larger than the capacity: taken in
    /// capacity-sized parts. Cancellation: drop the future.
    pub async fn take(&self, n: u64) {
        let mut remaining = n;
        while remaining > 0 {
            let mut changed = self.changed.subscribe();
            let Some(rate) = self.rate() else {
                return;
            };
            let part = remaining.min(capacity(rate));
            let wait = {
                let mut s = lock(&self.state);
                refill(&mut s, rate, Instant::now());
                let need = part as f64;
                if s.tokens >= need {
                    s.tokens -= need;
                    remaining = remaining.saturating_sub(part);
                    continue;
                }
                Duration::from_secs_f64((need - s.tokens) / rate as f64)
            };
            tokio::select! {
                () = tokio::time::sleep(wait) => {}
                _ = changed.changed() => {}
            }
        }
    }
}

fn refill(s: &mut State, rate: u64, now: Instant) {
    let elapsed = now.saturating_duration_since(s.last).as_secs_f64();
    s.tokens = (s.tokens + elapsed * rate as f64).min(capacity(rate) as f64);
    s.last = now;
}

/// The two budgets one download writes under.
#[derive(Debug, Clone)]
pub struct Rates {
    pub job: Arc<Bucket>,
    pub global: Arc<Bucket>,
}

impl Rates {
    pub fn new(job: Arc<Bucket>, global: Arc<Bucket>) -> Self {
        Self { job, global }
    }

    /// The largest part to write at once: the tightest capacity, or `None` when both are
    /// unlimited (the whole chunk is written in one go, as before CORE-06).
    pub fn part_limit(&self) -> Option<usize> {
        match (self.job.part_limit(), self.global.part_limit()) {
            (None, None) => None,
            (a, b) => {
                let p = a.unwrap_or(u64::MAX).min(b.unwrap_or(u64::MAX));
                Some(usize::try_from(p).unwrap_or(usize::MAX))
            }
        }
    }

    /// Takes `n` bytes from the download's budget, then from the global one.
    pub async fn charge(&self, n: u64) {
        self.job.take(n).await;
        self.global.take(n).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn timed(f: impl std::future::Future<Output = ()>) -> Duration {
        let t0 = Instant::now();
        f.await;
        t0.elapsed()
    }
    fn near(d: Duration, secs: f64) -> bool {
        (d.as_secs_f64() - secs).abs() < 0.02
    }

    #[test]
    fn capacity_and_bounds() {
        assert_eq!(capacity(1024), 1024); // floor: 1 s at the minimum
        assert_eq!(capacity(4096), 1024); // exactly 250 ms
        assert_eq!(capacity(1_000_000), 250_000);
        assert_eq!(validate(None), Ok(None));
        assert!(validate(Some(1023)).is_err());
        assert!(
            validate(Some(0)).is_err(),
            "0 is neither unlimited nor pause"
        );
        assert_eq!(validate(Some(1024)), Ok(Some(1024)));
        assert!(validate(Some(MAX_BYTES_PER_SECOND + 1)).is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn steady_rate_without_initial_burst() {
        let b = Bucket::new(Some(10_000));
        // 50,000 bytes at 10,000 B/s from an empty bucket: 5 s.
        assert!(near(timed(b.take(50_000)).await, 5.0));
    }

    #[tokio::test(start_paused = true)]
    async fn idle_burst_is_bounded_to_a_quarter_second() {
        let b = Bucket::new(Some(100_000));
        tokio::time::sleep(Duration::from_secs(60)).await;
        // After a minute idle only 25,000 bytes are free; 125,000 needs 1 more second.
        assert!(near(timed(b.take(25_000)).await, 0.0));
        assert!(near(timed(b.take(100_000)).await, 1.0));
    }

    #[tokio::test(start_paused = true)]
    async fn a_charge_larger_than_capacity_is_taken_in_parts_without_overdraw() {
        let b = Bucket::new(Some(4096)); // capacity 1024
        assert!(near(timed(b.take(65_536)).await, 16.0));
    }

    #[tokio::test(start_paused = true)]
    async fn unlimited_is_free_and_transitions_both_ways() {
        let b = Arc::new(Bucket::new(None));
        assert!(near(timed(b.take(u64::MAX)).await, 0.0));
        b.set_rate(Some(1024));
        assert!(near(timed(b.take(2048)).await, 2.0));
        b.set_rate(None);
        assert!(near(timed(b.take(1 << 30)).await, 0.0));
    }

    #[tokio::test(start_paused = true)]
    async fn a_live_change_wakes_waiters_and_recomputes() {
        let b = Arc::new(Bucket::new(Some(1024)));
        let waiter = {
            let b = Arc::clone(&b);
            tokio::spawn(async move { timed(b.take(1024)).await })
        };
        tokio::time::sleep(Duration::from_millis(100)).await;
        // From 1 KiB/s (1 s for the part) to unlimited: the waiter returns at once.
        b.set_rate(None);
        assert!(near(waiter.await.unwrap(), 0.1));
        // A decrease mid-wait also applies at once: 1,000,000 B/s → 1024 B/s for a 1024 B part.
        b.set_rate(Some(1_000_000));
        let waiter = {
            let b = Arc::clone(&b);
            tokio::spawn(async move { timed(b.take(200_000)).await })
        };
        tokio::time::sleep(Duration::from_millis(50)).await;
        b.set_rate(Some(1024));
        // The remaining part is re-sized to the new capacity and paced at the new rate.
        let took = waiter.await.unwrap().as_secs_f64();
        assert!(
            took > 100.0,
            "a decrease must slow the waiter down, took {took}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn cancellation_ends_the_wait_and_never_overdraws() {
        let b = Bucket::new(Some(1024));
        let r = tokio::time::timeout(Duration::from_millis(300), b.take(10_000)).await;
        assert!(r.is_err(), "the wait is cancellable");
        // The cancelled charge took nothing beyond what had accrued: the next 1024 B wait
        // at most 1 s (empty bucket after the 300 ms that refilled ~307 B).
        assert!(timed(b.take(1024)).await.as_secs_f64() <= 1.0 + 1e-6);
    }

    #[tokio::test(start_paused = true)]
    async fn segments_share_one_file_budget() {
        let job = Arc::new(Bucket::new(Some(8192)));
        let t0 = Instant::now();
        let mut tasks = Vec::new();
        for _ in 0..4 {
            let job = Arc::clone(&job);
            tasks.push(tokio::spawn(async move { job.take(8192).await }));
        }
        for t in tasks {
            t.await.unwrap();
        }
        // Four segments × 8 KiB at 8 KiB/s together: 4 s, not 1 s.
        assert!(near(t0.elapsed(), 4.0), "{:?}", t0.elapsed());
    }

    #[tokio::test(start_paused = true)]
    async fn both_budgets_apply_and_the_smaller_wins() {
        let global = Arc::new(Bucket::new(Some(10_000)));
        let fast_job = Rates::new(Arc::new(Bucket::new(Some(1_000_000))), Arc::clone(&global));
        assert!(
            near(timed(fast_job.charge(20_000)).await, 2.0),
            "global limits"
        );
        let slow_job = Rates::new(
            Arc::new(Bucket::new(Some(2_000))),
            Arc::new(Bucket::new(None)),
        );
        assert!(near(timed(slow_job.charge(4_000)).await, 2.0), "job limits");
        assert_eq!(fast_job.part_limit(), Some(2_500));
        let free = Rates::new(Arc::new(Bucket::new(None)), Arc::new(Bucket::new(None)));
        assert_eq!(free.part_limit(), None);
    }

    #[tokio::test(start_paused = true)]
    async fn three_and_six_downloads_share_the_global_budget() {
        for n in [1u64, 3, 6] {
            let global = Arc::new(Bucket::new(Some(60_000)));
            let t0 = Instant::now();
            let mut tasks = Vec::new();
            for _ in 0..n {
                let r = Rates::new(Arc::new(Bucket::new(None)), Arc::clone(&global));
                tasks.push(tokio::spawn(async move { r.charge(60_000).await }));
            }
            for t in tasks {
                t.await.unwrap();
            }
            // n × 60,000 B at 60,000 B/s: n seconds whatever the number of downloads.
            assert!(near(t0.elapsed(), n as f64), "n={n}: {:?}", t0.elapsed());
        }
    }
}
