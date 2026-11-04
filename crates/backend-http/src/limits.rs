//! Per-host connection budget (B1.5, CORE-02/E03; ADR 0010).
//!
//! Every HTTP request this backend sends (a probe, a whole-file or resumed transfer, each segment
//! worker, and each redirect hop) first takes a permit for its destination host and keeps it until
//! that request and its body are finished (dropped). At most [`HttpConfig::max_connections_per_host`]
//! permits exist per host, shared by all attempts of the backend, so concurrent requests to one host
//! never exceed it.
//!
//! The client keeps **no idle pooled connections** (`pool_max_idle_per_host(0)`): each request's
//! connection closes when its permit is released, so open TCP connections per host also stay within
//! the budget (plus connections the OS is still tearing down).
//!
//! **Host key:** the URL's host as parsed (ASCII-lowercased; internationalised names in punycode),
//! without a trailing dot; IPv6 literals as written. The port and scheme are ignored, so
//! `http://h:80` and `https://h:8443` share one budget: the conservative choice for a server.
//! A redirect hop releases the previous hop's permit before taking one for its own host.
//!
//! **Permit trace (diagnostics, B2.32):** when `DOWNOVA_PERMIT_TRACE` names a file, every permit taken
//! and released appends one JSON line `{"t_ms","event","host_hash","held"}`, where `held` is the
//! number of this host's permits held right after the event. This is the engine's own, exact view of
//! the budget (a server only sees connections, and later). The host is hashed, never written.
//!
//! [`HttpConfig::max_connections_per_host`]: crate::HttpConfig::max_connections_per_host

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use dm_backend_api::BackendError;
use dm_domain::ErrorKind;
use reqwest::Url;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// A held connection slot for one host; released when dropped.
#[derive(Debug)]
pub(crate) struct Permit {
    // Released first in `Drop` so a traced count is the count after release.
    inner: Option<OwnedSemaphorePermit>,
    traced: Option<Traced>,
}

#[derive(Debug)]
struct Traced {
    budget: Arc<HostBudget>,
    per_host: usize,
    host_hash: String,
}

impl Drop for Permit {
    fn drop(&mut self) {
        drop(self.inner.take());
        if let Some(t) = &self.traced {
            let held = t
                .per_host
                .saturating_sub(t.budget.slots.available_permits());
            trace_permit("release", &t.host_hash, held);
        }
    }
}

/// The trace file, read once per process (`None`: tracing off).
fn trace_path() -> Option<&'static std::path::Path> {
    static PATH: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
    PATH.get_or_init(|| std::env::var_os("DOWNOVA_PERMIT_TRACE").map(Into::into))
        .as_deref()
}

fn trace_permit(event: &str, host_hash: &str, held: usize) {
    let Some(path) = trace_path() else {
        return;
    };
    let t_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let line = format!(
        "{{\"t_ms\":{t_ms},\"event\":\"{event}\",\"host_hash\":\"{host_hash}\",\"held\":{held}}}\n"
    );
    // One write per line: a reader never sees half a line from a single writer.
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        use std::io::Write;
        let _ = file.write_all(line.as_bytes());
    }
}

/// A short, non-reversible tag for a host key (the trace never names hosts).
fn host_hash(key: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut h);
    format!("{:08x}", h.finish() & 0xffff_ffff)
}

#[derive(Debug)]
struct HostBudget {
    slots: Arc<Semaphore>,
    waiting: AtomicUsize,
    /// The most slots of this host held at once since the backend started (diagnostics).
    peak: AtomicUsize,
}

/// Cancellation of acquire also clears the pressure signal.
#[derive(Debug)]
struct Waiting(Arc<HostBudget>);

impl Drop for Waiting {
    fn drop(&mut self) {
        self.0.waiting.fetch_sub(1, Ordering::SeqCst);
    }
}

/// The shared per-host budget.
#[derive(Debug)]
pub(crate) struct HostLimiter {
    per_host: usize,
    hosts: Mutex<HashMap<String, Arc<HostBudget>>>,
}

impl HostLimiter {
    pub(crate) fn new(per_host: usize) -> Self {
        Self {
            per_host: per_host.max(1),
            hosts: Mutex::new(HashMap::new()),
        }
    }

    /// The canonical host key (see the module documentation).
    pub(crate) fn key(url: &Url) -> String {
        url.host_str()
            .unwrap_or_default()
            .trim_end_matches('.')
            .to_ascii_lowercase()
    }

    fn semaphore(&self, key: String) -> Arc<HostBudget> {
        let mut hosts = crate::lock(&self.hosts);
        Arc::clone(hosts.entry(key).or_insert_with(|| {
            Arc::new(HostBudget {
                slots: Arc::new(Semaphore::new(self.per_host)),
                waiting: AtomicUsize::new(0),
                peak: AtomicUsize::new(0),
            })
        }))
    }

    /// Waits for a slot to `url`'s host. Callers wrap this in their stop/halt select, so a paused
    /// or cancelled attempt never waits forever.
    pub(crate) async fn acquire(&self, url: &Url) -> Result<Permit, BackendError> {
        let key = Self::key(url);
        let budget = self.semaphore(key.clone());
        budget.waiting.fetch_add(1, Ordering::SeqCst);
        let waiting = Waiting(Arc::clone(&budget));
        let result = Arc::clone(&budget.slots)
            .acquire_owned()
            .await
            .map_err(|_| BackendError::new(ErrorKind::Internal, "connection budget closed"));
        drop(waiting);
        let inner = result?;
        let held = self
            .per_host
            .saturating_sub(budget.slots.available_permits());
        budget.peak.fetch_max(held, Ordering::SeqCst);
        let traced = trace_path().map(|_| {
            let host_hash = host_hash(&key);
            trace_permit("acquire", &host_hash, held);
            Traced {
                budget: Arc::clone(&budget),
                per_host: self.per_host,
                host_hash,
            }
        });
        Ok(Permit {
            inner: Some(inner),
            traced,
        })
    }

    /// The most request slots any one host held at the same time (diagnostics and tests: the
    /// client-side measure of the per-host budget, independent of when a server notices a closed
    /// connection).
    pub(crate) fn peak_per_host(&self) -> usize {
        crate::lock(&self.hosts)
            .values()
            .map(|b| b.peak.load(Ordering::SeqCst))
            .max()
            .unwrap_or(0)
    }

    /// Validated segments yield their request when another request is waiting (ADR 0023).
    pub(crate) fn has_waiters(&self, url: &Url) -> bool {
        self.semaphore(Self::key(url))
            .waiting
            .load(Ordering::SeqCst)
            > 0
    }

    /// Slots of `url`'s host in use right now (tests, diagnostics).
    #[cfg(test)]
    pub(crate) fn in_use(&self, url: &Url) -> usize {
        let sem = self.semaphore(Self::key(url));
        self.per_host.saturating_sub(sem.slots.available_permits())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancelling_a_waiter_clears_pressure_without_releasing_another_requests_slot() {
        let l = HostLimiter::new(1);
        let url = Url::parse("http://a.example/f").unwrap();
        let permit = l.acquire(&url).await.unwrap();
        {
            let pending = l.acquire(&url);
            tokio::pin!(pending);
            tokio::select! {
                result = &mut pending => panic!("unexpected permit: {result:?}"),
                _ = tokio::time::sleep(std::time::Duration::from_millis(10)) => {},
            }
            assert!(l.has_waiters(&url));
        }
        assert!(!l.has_waiters(&url));
        assert_eq!(l.in_use(&url), 1);
        drop(permit);
        assert_eq!(l.in_use(&url), 0);
    }

    #[test]
    fn host_keys_ignore_case_ports_schemes_and_a_trailing_dot() {
        let k = |u: &str| HostLimiter::key(&Url::parse(u).unwrap());
        assert_eq!(k("http://Example.ORG/a"), "example.org");
        assert_eq!(k("https://example.org:8443/b"), "example.org");
        assert_eq!(k("http://example.org./c"), "example.org");
        assert_eq!(k("http://bücher.example/"), "xn--bcher-kva.example");
        assert_eq!(k("http://[::1]:8080/"), "[::1]");
        assert_ne!(k("http://localhost/"), k("http://127.0.0.1/"));
    }

    #[tokio::test]
    async fn at_most_the_budget_is_handed_out_per_host_and_released_on_drop() {
        let l = HostLimiter::new(2);
        let a = Url::parse("http://a.example/").unwrap();
        let b = Url::parse("http://b.example/").unwrap();
        let p1 = l.acquire(&a).await.unwrap();
        let _p2 = l.acquire(&a).await.unwrap();
        assert_eq!(l.in_use(&a), 2);
        // A third request to the same host waits…
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), l.acquire(&a))
                .await
                .is_err()
        );
        // …another host does not.
        let _q = l.acquire(&b).await.unwrap();
        drop(p1);
        let _p3 = tokio::time::timeout(std::time::Duration::from_millis(500), l.acquire(&a))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(l.in_use(&a), 2);
    }
}
