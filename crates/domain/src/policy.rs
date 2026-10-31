//! Pure policies shared by the service and its clients (B2.8, B2.11): file-type categories for
//! destination rules, duplicate-link identity, and the admission order of queued jobs.
//!
//! Nothing here does I/O; the service applies these decisions to its state.

use serde::{Deserialize, Serialize};

/// File-type category of a download (destination rules, ADR 0015). `Other` is everything no
/// default rule matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileCategory {
    Compressed,
    Documents,
    Music,
    Programs,
    Video,
    Other,
}

impl FileCategory {
    pub const ALL: [FileCategory; 6] = [
        FileCategory::Compressed,
        FileCategory::Documents,
        FileCategory::Music,
        FileCategory::Programs,
        FileCategory::Video,
        FileCategory::Other,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            FileCategory::Compressed => "compressed",
            FileCategory::Documents => "documents",
            FileCategory::Music => "music",
            FileCategory::Programs => "programs",
            FileCategory::Video => "video",
            FileCategory::Other => "other",
        }
    }

    pub fn parse(s: &str) -> Option<FileCategory> {
        FileCategory::ALL.into_iter().find(|c| c.as_str() == s)
    }

    /// The default extensions of each category (the desktop's category panel uses the same list).
    fn extensions(self) -> &'static [&'static str] {
        match self {
            FileCategory::Compressed => {
                &["zip", "rar", "7z", "tar", "gz", "bz2", "xz", "zst", "iso"]
            }
            FileCategory::Documents => &[
                "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt", "txt", "epub", "csv",
            ],
            FileCategory::Music => &["mp3", "flac", "wav", "aac", "m4a", "ogg", "opus", "wma"],
            FileCategory::Programs => &[
                "exe", "msi", "msix", "dmg", "pkg", "deb", "rpm", "appimage", "apk",
            ],
            FileCategory::Video => &["mp4", "mkv", "webm", "avi", "mov", "wmv", "m4v", "ts"],
            FileCategory::Other => &[],
        }
    }

    /// The category of a file name by its extension (case-insensitive).
    pub fn of_name(name: &str) -> FileCategory {
        let ext = match name.rsplit_once('.') {
            Some((stem, ext)) if !stem.is_empty() => ext.to_ascii_lowercase(),
            _ => return FileCategory::Other,
        };
        FileCategory::ALL
            .into_iter()
            .find(|c| c.extensions().contains(&ext.as_str()))
            .unwrap_or(FileCategory::Other)
    }
}

/// What `downloads.v1.add` does when the same link is already in the list (ADR 0015). Clients
/// that omit it get `Allow`, the behaviour before B2.8.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DuplicatePolicy {
    /// Add another download of the same link.
    #[default]
    Allow,
    /// Refuse with `DUPLICATE_DOWNLOAD` while an unremoved job has the same link.
    Refuse,
}

/// What happens when a download's final name is already taken (ADR 0015). Nothing is ever
/// overwritten under either policy.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollisionPolicy {
    /// Use the next free name automatically (`name (1).ext`), as before B2.8.
    #[default]
    Rename,
    /// Hold the verified file before placement until the user confirms the free name or cancels.
    Ask,
}

/// Job priority within admission (B2.11, ADR 0017).
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Low,
    #[default]
    Normal,
    High,
}

impl Priority {
    /// Stored as -1, 0, 1.
    pub fn as_i64(self) -> i64 {
        match self {
            Priority::Low => -1,
            Priority::Normal => 0,
            Priority::High => 1,
        }
    }

    pub fn from_i64(v: i64) -> Option<Priority> {
        match v {
            -1 => Some(Priority::Low),
            0 => Some(Priority::Normal),
            1 => Some(Priority::High),
            _ => None,
        }
    }
}

/// The identity used to find "the same link" (ADR 0015): the exact URL with only the scheme and
/// host lower-cased, because those are case-insensitive. The path, query (including signed
/// tokens) and fragment are kept byte for byte, so different signatures or path case never
/// match. Never derived from a redacted URL. Returns `None` for anything that is not an absolute
/// http(s) URL.
pub fn link_identity(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(end);
    if authority.is_empty() {
        return None;
    }
    // Userinfo (if any) is part of the identity and keeps its case; only the host is folded.
    let (userinfo, host) = match authority.rsplit_once('@') {
        Some((u, h)) => (Some(u), h),
        None => (None, authority),
    };
    let mut out = format!("{scheme}://");
    if let Some(u) = userinfo {
        out.push_str(u);
        out.push('@');
    }
    out.push_str(&host.to_ascii_lowercase());
    out.push_str(tail);
    Some(out)
}

/// One job that is ready to start, as admission sees it (ADR 0017).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// Index of the job in the caller's list (returned in the order).
    pub index: usize,
    /// Which queue it belongs to (position of the queue in the queue list).
    pub queue_rank: u32,
    pub priority: Priority,
    /// Manual position within its queue (smaller first).
    pub position: i64,
    /// Acceptance time; the stable last tie-break with `index`.
    pub created_at_ms: u64,
}

/// The order in which ready jobs are started: higher priority first; then the queue's order in
/// the queue list; then the manual position inside the queue; then acceptance time; then the
/// caller's list order. Deterministic for equal keys. A priority always wins over queue order, so
/// a high-priority job of a later queue starts before a normal one of the first queue (the
/// documented fairness policy: priority is global, ties are broken by queue order, never by
/// random IDs).
pub fn admission_order(candidates: &[Candidate]) -> Vec<usize> {
    let mut sorted: Vec<&Candidate> = candidates.iter().collect();
    sorted.sort_by(|a, b| {
        b.priority
            .cmp(&a.priority)
            .then(a.queue_rank.cmp(&b.queue_rank))
            .then(a.position.cmp(&b.position))
            .then(a.created_at_ms.cmp(&b.created_at_ms))
            .then(a.index.cmp(&b.index))
    });
    sorted.into_iter().map(|c| c.index).collect()
}

/// Picks which of the ordered candidates start now, honouring the free global slots and each
/// queue's remaining capacity (`queue_free[queue_rank]`). A candidate of a full queue is skipped
/// without blocking the ones behind it.
pub fn select_admissions(
    ordered: &[(usize, u32)],
    mut global_free: usize,
    queue_free: &mut [usize],
) -> Vec<usize> {
    let mut chosen = Vec::new();
    for (index, rank) in ordered {
        if global_free == 0 {
            break;
        }
        let Some(free) = usize::try_from(*rank)
            .ok()
            .and_then(|r| queue_free.get_mut(r))
        else {
            continue;
        };
        if *free == 0 {
            continue;
        }
        *free = free.saturating_sub(1);
        global_free = global_free.saturating_sub(1);
        chosen.push(*index);
    }
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_follow_the_extension_case_insensitively() {
        assert_eq!(FileCategory::of_name("a.ZIP"), FileCategory::Compressed);
        assert_eq!(FileCategory::of_name("report.pdf"), FileCategory::Documents);
        assert_eq!(FileCategory::of_name("song.flac"), FileCategory::Music);
        assert_eq!(FileCategory::of_name("setup.exe"), FileCategory::Programs);
        assert_eq!(FileCategory::of_name("clip.mkv"), FileCategory::Video);
        assert_eq!(FileCategory::of_name("data.bin"), FileCategory::Other);
        assert_eq!(FileCategory::of_name(".zip"), FileCategory::Other);
        assert_eq!(FileCategory::of_name("noext"), FileCategory::Other);
        for c in FileCategory::ALL {
            assert_eq!(FileCategory::parse(c.as_str()), Some(c));
        }
    }

    #[test]
    fn link_identity_folds_only_scheme_and_host() {
        assert_eq!(
            link_identity("HTTPS://Example.COM/A.bin?sig=AbC#x").as_deref(),
            Some("https://example.com/A.bin?sig=AbC#x")
        );
        // Case-sensitive paths and distinct signed queries never match.
        assert_ne!(
            link_identity("https://h/A.bin"),
            link_identity("https://h/a.bin")
        );
        assert_ne!(
            link_identity("https://h/f?sig=1"),
            link_identity("https://h/f?sig=2")
        );
        assert_eq!(
            link_identity("http://User:Pw@HOST:8080/x").as_deref(),
            Some("http://User:Pw@host:8080/x")
        );
        assert_eq!(link_identity("ftp://h/x"), None);
        assert_eq!(link_identity("https:///x"), None);
    }

    fn c(
        index: usize,
        queue_rank: u32,
        priority: Priority,
        position: i64,
        created: u64,
    ) -> Candidate {
        Candidate {
            index,
            queue_rank,
            priority,
            position,
            created_at_ms: created,
        }
    }

    #[test]
    fn priority_then_queue_order_then_position_then_age() {
        let order = admission_order(&[
            c(0, 0, Priority::Normal, 2, 10),
            c(1, 1, Priority::High, 9, 50),
            c(2, 0, Priority::Normal, 1, 20),
            c(3, 0, Priority::Low, 0, 0),
            c(4, 1, Priority::Normal, 0, 0),
            c(5, 0, Priority::Normal, 1, 20),
        ]);
        // High first (later queue); then queue 0 normals by position, ties by age then index;
        // then queue 1's normal; low last.
        assert_eq!(order, vec![1, 2, 5, 0, 4, 3]);
    }

    #[test]
    fn full_queues_are_skipped_without_blocking_others() {
        // Queue 0 has one free slot, queue 1 two; two global slots.
        let mut free = vec![1, 2];
        let chosen = select_admissions(&[(10, 0), (11, 0), (12, 1)], 2, &mut free);
        assert_eq!(chosen, vec![10, 12]);
        assert_eq!(free, vec![0, 1]);
        // No global slot: nothing starts.
        let mut free = vec![3];
        assert!(select_admissions(&[(1, 0)], 0, &mut free).is_empty());
        // Unknown queue rank is skipped.
        let mut free = vec![1];
        assert_eq!(select_admissions(&[(1, 5), (2, 0)], 2, &mut free), vec![2]);
    }

    #[test]
    fn priority_round_trips_through_storage_values() {
        for p in [Priority::Low, Priority::Normal, Priority::High] {
            assert_eq!(Priority::from_i64(p.as_i64()), Some(p));
        }
        assert_eq!(Priority::from_i64(7), None);
    }
}
