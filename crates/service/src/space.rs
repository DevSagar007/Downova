//! The service's disk budget policy (B1.4, ADV05 basic; ADR 0009).
//!
//! [`PlatformSpaceGuard`] turns the platform's free-space snapshot into the
//! [`SpaceGuard`] decision the backend and the finalizer ask for:
//! - free ≥ needed + [`SAFETY_MARGIN`] → go on;
//! - less → `DISK_FULL` with the amounts and what to do;
//! - the destination folder or its drive is missing → `VOLUME_MISSING`;
//! - the OS cannot tell → go on (advisory: a real "disk full" error is still caught and handled),
//!   reported once on stderr. No free-space value is ever invented.
//!
//! **Advisory, not a reservation.** Nothing is reserved: concurrent downloads and other programs on
//! the same volume can consume the space between the check and the write, so the real `ENOSPC` path
//! stays essential. The checks run before the first body byte, at every durable sync (about once a
//! second while transferring), before finalization's copy fallback, and every
//! [`crate::core::DISK_RECHECK_INTERVAL`] while a job waits for space.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use dm_backend_api::{BackendError, SpaceGuard};
use dm_domain::ErrorKind;
use dm_platform::space::{Space, SpaceProbe};

/// Kept free on the destination volume beyond what a download still needs: room for the
/// database, the journal and other programs (64 MiB).
pub const SAFETY_MARGIN: u64 = 64 * 1024 * 1024;

const MIB: u64 = 1024 * 1024;

/// The policy over a platform probe.
#[derive(Debug)]
pub struct PlatformSpaceGuard {
    probe: Arc<dyn SpaceProbe>,
    margin: u64,
    warned: AtomicBool,
}

impl PlatformSpaceGuard {
    pub fn new(probe: Arc<dyn SpaceProbe>) -> Self {
        Self::with_margin(probe, SAFETY_MARGIN)
    }

    pub fn with_margin(probe: Arc<dyn SpaceProbe>, margin: u64) -> Self {
        Self {
            probe,
            margin,
            warned: AtomicBool::new(false),
        }
    }
}

impl SpaceGuard for PlatformSpaceGuard {
    fn ensure(&self, dir: &Path, needed: u64) -> Result<(), BackendError> {
        match self.probe.query(dir) {
            Space::Available(free) => {
                let wanted = needed.saturating_add(self.margin);
                if free >= wanted {
                    return Ok(());
                }
                Err(BackendError::new(
                    ErrorKind::DiskFull,
                    format!(
                        "not enough free space on the destination drive: {} MiB more are needed (including a {} MiB safety margin) and {} MiB are free; free some space and the download continues by itself",
                        wanted.div_ceil(MIB),
                        self.margin / MIB,
                        free / MIB
                    ),
                ))
            }
            Space::VolumeMissing => Err(BackendError::new(
                ErrorKind::VolumeMissing,
                "the destination folder or its drive is not available; reconnect it and the download continues by itself",
            )),
            Space::Unavailable(why) => {
                if !self.warned.swap(true, Ordering::Relaxed) {
                    eprintln!(
                        "dm-service: free space cannot be determined ({why}); continuing without a disk budget check"
                    );
                }
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct Fixed(Space);
    impl SpaceProbe for Fixed {
        fn query(&self, _dir: &Path) -> Space {
            self.0.clone()
        }
    }

    fn guard(space: Space) -> PlatformSpaceGuard {
        PlatformSpaceGuard::with_margin(Arc::new(Fixed(space)), 10)
    }

    #[test]
    fn the_margin_is_added_to_what_is_needed() {
        let dir = Path::new(".");
        assert!(guard(Space::Available(110)).ensure(dir, 100).is_ok());
        let e = guard(Space::Available(109)).ensure(dir, 100).unwrap_err();
        assert_eq!(e.kind, ErrorKind::DiskFull);
        assert!(e.detail.contains("free some space"), "{e}");
    }

    #[test]
    fn a_missing_volume_and_an_unknown_answer_are_reported_not_invented() {
        let dir = Path::new(".");
        assert_eq!(
            guard(Space::VolumeMissing).ensure(dir, 1).unwrap_err().kind,
            ErrorKind::VolumeMissing
        );
        // Unknown: advisory pass; the real ENOSPC path still protects the download.
        assert!(
            guard(Space::Unavailable("test".into()))
                .ensure(dir, u64::MAX)
                .is_ok()
        );
    }
}
