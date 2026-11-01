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
}
