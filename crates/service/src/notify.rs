//! Completion and failure notifications (CORE-09, ADR 0022).
//!
//! The service decides; a [`Notifier`] only shows. Decisions are made in the poll loop for jobs
//! whose terminal state is already durable (completed after verification and placement, or
//! failed for good — an automatic retry is not terminal), recorded in the store at most once per
//! outcome, and handed to the notifier *after* the coordinator lock is released, on a blocking
//! thread. Delivery is therefore at most once: a crash between the recorded decision and the
//! system call loses that notification, it is never repeated at startup, and nothing historical
//! is notified. Texts carry the file's display name only — never a URL, header or path — and a
//! notification never opens or runs anything.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, OnceLock};

use dm_domain::api::{NotificationAvailability, NotificationLocale};

/// One notification as the system shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub title: String,
    pub body: String,
}

/// Why a notice is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    Completed,
    Failed,
    /// The confirmation shown when the user turns notifications on.
    Enabled,
}

/// Shows notices on the operating system. Implementations must not block for long and must never
/// open, run or reveal a file.
pub trait Notifier: Send + Sync + std::fmt::Debug + 'static {
    fn availability(&self) -> NotificationAvailability;
    /// Hands one notice to the system; an error means it was refused there.
    fn show(&self, notice: &Notice) -> Result<(), String>;
}

/// No system notifications (tests, unsupported systems).
#[derive(Debug, Default)]
pub struct NoNotifier;

impl Notifier for NoNotifier {
    fn availability(&self) -> NotificationAvailability {
        NotificationAvailability::Unavailable
    }
    fn show(&self, _notice: &Notice) -> Result<(), String> {
        Err("notifications are not available".to_owned())
    }
}

/// Removes control characters (and bidi overrides) from a display name for a notification.
fn clean(name: &str) -> String {
    name.chars()
        .filter(|c| {
            !c.is_control()
                && !matches!(
                    c,
                    '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
                )
        })
        .take(200)
        .collect()
}

/// The localized notice (English and Bangla).
pub fn notice(kind: NoticeKind, locale: NotificationLocale, name: &str) -> Notice {
    let name = clean(name);
    let (title, body) = match (locale, kind) {
        (NotificationLocale::En, NoticeKind::Completed) => (
            "Download complete".to_owned(),
            format!("{name} was downloaded and checked."),
        ),
        (NotificationLocale::En, NoticeKind::Failed) => (
            "Download failed".to_owned(),
            format!("{name} could not be downloaded. Open Downova to see why."),
        ),
        (NotificationLocale::En, NoticeKind::Enabled) => (
            "Notifications are on".to_owned(),
            "Downova will tell you when a download completes or fails.".to_owned(),
        ),
        (NotificationLocale::Bn, NoticeKind::Completed) => (
            "ডাউনলোড সম্পূর্ণ".to_owned(),
            format!("{name} নামানো ও যাচাই করা হয়েছে।"),
        ),
        (NotificationLocale::Bn, NoticeKind::Failed) => (
            "ডাউনলোড ব্যর্থ".to_owned(),
            format!("{name} নামানো যায়নি। কারণ দেখতে Downova খুলুন।"),
        ),
        (NotificationLocale::Bn, NoticeKind::Enabled) => (
            "নোটিফিকেশন চালু".to_owned(),
            "ডাউনলোড সম্পূর্ণ বা ব্যর্থ হলে Downova জানাবে।".to_owned(),
        ),
    };
    Notice { title, body }
}

/// Most notices waiting for the system at once. A notifier that blocks (an unresponsive desktop
/// notification service) cannot make the queue grow without bound: further notices are dropped and
/// reported as a failed delivery; downloads are never affected.
pub const DELIVERY_QUEUE: usize = 32;

/// Hands notices to the [`Notifier`] on one dedicated thread, in the order they were decided
/// (B2.24). The thread starts with the first notice and ends when the service drops this value.
#[derive(Debug)]
pub struct Delivery {
    notifier: Arc<dyn Notifier>,
    failed: Arc<AtomicBool>,
    queue: OnceLock<Option<SyncSender<Notice>>>,
}

impl Delivery {
    pub fn new(notifier: Arc<dyn Notifier>) -> Self {
        Self {
            notifier,
            failed: Arc::new(AtomicBool::new(false)),
            queue: OnceLock::new(),
        }
    }

    pub fn availability(&self) -> NotificationAvailability {
        self.notifier.availability()
    }

    /// The most recent hand-off to the system failed (or a notice was dropped).
    pub fn last_failed(&self) -> bool {
        self.failed.load(Ordering::SeqCst)
    }

    /// Queues a notice; never blocks the caller.
    pub fn send(&self, notice: Notice) {
        let queue = self.queue.get_or_init(|| {
            let (tx, rx) = sync_channel::<Notice>(DELIVERY_QUEUE);
            let notifier = Arc::clone(&self.notifier);
            let failed = Arc::clone(&self.failed);
            std::thread::Builder::new()
                .name("dm-notify".into())
                .spawn(move || {
                    for notice in rx {
                        let result = notifier.show(&notice);
                        if let Err(e) = &result {
                            eprintln!("dm-service: notification not shown: {e}");
                        }
                        failed.store(result.is_err(), Ordering::SeqCst);
                    }
                })
                .map_err(|e| eprintln!("dm-service: notification thread not started: {e}"))
                .ok()
                .map(|_| tx)
        });
        let sent = match queue {
            Some(tx) => match tx.try_send(notice) {
                Ok(()) => true,
                Err(TrySendError::Full(_)) => {
                    eprintln!("dm-service: notification dropped: the system is not accepting them");
                    false
                }
                Err(TrySendError::Disconnected(_)) => false,
            },
            None => false,
        };
        if !sent {
            self.failed.store(true, Ordering::SeqCst);
        }
    }
}

#[cfg(test)]
pub(crate) mod recording {
    use std::sync::Mutex;

    use super::*;

    /// Records notices instead of showing them; can be told to fail.
    #[derive(Debug, Default)]
    pub struct RecordingNotifier {
        pub shown: Mutex<Vec<Notice>>,
        pub fail: std::sync::atomic::AtomicBool,
    }

    impl Notifier for RecordingNotifier {
        fn availability(&self) -> NotificationAvailability {
            NotificationAvailability::Available
        }
        fn show(&self, notice: &Notice) -> Result<(), String> {
            self.shown
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(notice.clone());
            if self.fail.load(std::sync::atomic::Ordering::SeqCst) {
                Err("blocked by the system".to_owned())
            } else {
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_are_localized_and_carry_only_a_clean_name() {
        let n = notice(
            NoticeKind::Completed,
            NotificationLocale::En,
            "re\u{202E}port\n.pdf",
        );
        assert_eq!(n.title, "Download complete");
        assert_eq!(n.body, "report.pdf was downloaded and checked.");
        let b = notice(NoticeKind::Failed, NotificationLocale::Bn, "বই.pdf");
        assert_eq!(b.title, "ডাউনলোড ব্যর্থ");
        assert!(b.body.starts_with("বই.pdf"));
        assert!(
            !notice(NoticeKind::Enabled, NotificationLocale::En, "x")
                .body
                .contains("http")
        );
    }

    /// A notifier that blocks until released, recording what it was given.
    #[derive(Debug, Default)]
    struct Stuck {
        release: std::sync::Mutex<bool>,
        wake: std::sync::Condvar,
        shown: std::sync::Mutex<Vec<String>>,
    }

    impl Notifier for Stuck {
        fn availability(&self) -> NotificationAvailability {
            NotificationAvailability::Available
        }
        fn show(&self, notice: &Notice) -> Result<(), String> {
            let mut open = self.release.lock().unwrap();
            while !*open {
                open = self.wake.wait(open).unwrap();
            }
            self.shown.lock().unwrap().push(notice.body.clone());
            Ok(())
        }
    }

    /// B2.24: delivery keeps decision order, never blocks the caller, and a system that stops
    /// accepting notices bounds the queue (dropped notices are reported as a failed delivery).
    #[test]
    fn delivery_is_ordered_bounded_and_never_blocks_the_caller() {
        let stuck = Arc::new(Stuck::default());
        let delivery = Delivery::new(stuck.clone());
        let started = std::time::Instant::now();
        let total = DELIVERY_QUEUE + 10;
        for i in 0..total {
            delivery.send(notice(
                NoticeKind::Completed,
                NotificationLocale::En,
                &format!("f{i}"),
            ));
        }
        assert!(
            started.elapsed() < std::time::Duration::from_secs(1),
            "send never waits"
        );
        assert!(delivery.last_failed(), "overflow is reported");
        *stuck.release.lock().unwrap() = true;
        stuck.wake.notify_all();
        // One notice was taken by the thread before the queue filled, plus the queue itself.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while stuck.shown.lock().unwrap().len() < DELIVERY_QUEUE
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let shown = stuck.shown.lock().unwrap().clone();
        assert!(
            shown.len() >= DELIVERY_QUEUE && shown.len() <= DELIVERY_QUEUE + 1,
            "{}",
            shown.len()
        );
        let order: Vec<usize> = shown
            .iter()
            .map(|b| {
                b.trim_start_matches('f')
                    .split_whitespace()
                    .next()
                    .unwrap()
                    .parse()
                    .unwrap()
            })
            .collect();
        assert!(
            order.windows(2).all(|w| w[0] < w[1]),
            "in decision order: {order:?}"
        );
        assert!(order.iter().all(|&i| i < total));
    }
}
