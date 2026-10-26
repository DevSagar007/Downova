//! Durable resume checkpoints for `dm-backend-http` (B1.2, ADR 0007).
//!
//! **Format (version 1, JSON, at most [`MAX_CHECKPOINT_BYTES`]):** the transfer mode (`single` or
//! `segmented`), the destination directory and file name it is bound to, the representation
//! identity (final URL, strong ETag, length) and one region per connection: `start..end` of the
//! file, `done` bytes confirmed from `start`, and the SHA-256 **digest** of exactly those bytes.
//! The in-memory hasher state is never stored: on import every region is re-read, its digest
//! compared, and the hasher rebuilt from the bytes on disk.
//!
//! **Export** happens only for bytes the writer has flushed *and synced* (`sync_data`) before it
//! published them, so the service can commit the checkpoint at once. Transfers without a strong
//! ETag or a known length export nothing (they cannot be resumed safely).
//!
//! **Import** refuses, with an explicit error, anything that is not exactly ours: another backend
//! or format version, an oversized or malformed payload, a different destination, an unsupported
//! URL scheme, regions that do not tile the file, a destination that already exists, or a partial
//! file that is missing, shorter than the confirmed data, or whose confirmed bytes no longer match
//! their digest. In single-connection mode the bytes after the confirmed offset were written after
//! the last checkpoint (a hard kill between a write and the next checkpoint); they are
//! **truncated**, never trusted. In segmented mode the file is pre-sized; bytes after a segment's
//! confirmed point are overwritten when that segment continues.

use std::path::{Path, PathBuf};

use dm_backend_api::{BackendError, DurableCheckpoint, MAX_CHECKPOINT_BYTES};
use dm_domain::{ByteCount, ErrorKind};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::classify::io_error;
use crate::segmented::{SegCheckpoint, Segment};
use crate::transfer::{Checkpoint, Identity, WRITE_BUFFER};

/// Backend id written into every checkpoint.
pub(crate) const BACKEND_ID: &str = "http";
/// Current payload format.
pub(crate) const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Mode {
    Single,
    Segmented,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Region {
    start: u64,
    end: u64,
    done: u64,
    /// SHA-256 of `start..start+done`, lower-case hex.
    sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Payload {
    v: u32,
    mode: Mode,
    destination_dir: String,
    file_name: String,
    url: String,
    etag: String,
    length: u64,
    regions: Vec<Region>,
}

/// What an imported checkpoint resumes.
#[derive(Debug)]
pub(crate) enum Restored {
    Single(Checkpoint),
    Segmented(SegCheckpoint),
}

impl Restored {
    pub(crate) fn confirmed(&self) -> u64 {
        match self {
            Self::Single(cp) => cp.offset,
            Self::Segmented(cp) => cp.confirmed(),
        }
    }

    pub(crate) fn length(&self) -> Option<u64> {
        match self {
            Self::Single(cp) => cp.identity.length,
            Self::Segmented(cp) => Some(cp.length),
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn refused(why: &str) -> BackendError {
    BackendError::new(
        ErrorKind::BackendCapabilityUnavailable,
        format!("the saved resume point cannot be used ({why}); the download must restart"),
    )
}

fn changed(what: &str) -> BackendError {
    BackendError::new(
        ErrorKind::InvalidRange,
        format!(
            "the partial file was {what} after the last saved resume point; the download must restart"
        ),
    )
}

fn dir_string(dir: &Path) -> Result<String, BackendError> {
    dir.to_str()
        .map(str::to_owned)
        .ok_or_else(|| refused("destination path is not UTF-8"))
}

/// Encodes a single-connection checkpoint, or `None` when it cannot be resumed after a restart.
pub(crate) fn export_single(
    cp: &Checkpoint,
    destination_dir: &Path,
    file_name: &str,
) -> Result<Option<DurableCheckpoint>, BackendError> {
    let (Some(etag), Some(length)) = (cp.identity.etag.clone(), cp.identity.length) else {
        return Ok(None);
    };
    if cp.offset == 0 {
        return Ok(None);
    }
    encode(&Payload {
        v: FORMAT_VERSION,
        mode: Mode::Single,
        destination_dir: dir_string(destination_dir)?,
        file_name: file_name.to_owned(),
        url: cp.identity.url.as_str().to_owned(),
        etag,
        length,
        regions: vec![Region {
            start: 0,
            end: length,
            done: cp.offset,
            sha256: hex(&cp.hasher.clone().finalize()),
        }],
    })
    .map(Some)
}

/// Encodes a segmented checkpoint, or `None` when nothing is confirmed yet.
pub(crate) fn export_segmented(
    cp: &SegCheckpoint,
    destination_dir: &Path,
    file_name: &str,
) -> Result<Option<DurableCheckpoint>, BackendError> {
    let Some(etag) = cp.identity.etag.clone() else {
        return Ok(None);
    };
    if cp.confirmed() == 0 {
        return Ok(None);
    }
    encode(&Payload {
        v: FORMAT_VERSION,
        mode: Mode::Segmented,
        destination_dir: dir_string(destination_dir)?,
        file_name: file_name.to_owned(),
        url: cp.identity.url.as_str().to_owned(),
        etag,
        length: cp.length,
        regions: cp
            .segments
            .iter()
            .map(|s| Region {
                start: s.start,
                end: s.end,
                done: s.done,
                sha256: hex(&s.hasher.clone().finalize()),
            })
            .collect(),
    })
    .map(Some)
}

fn encode(payload: &Payload) -> Result<DurableCheckpoint, BackendError> {
    let bytes = serde_json::to_vec(payload)
        .map_err(|_| BackendError::new(ErrorKind::Internal, "cannot encode checkpoint"))?;
    if bytes.len() > MAX_CHECKPOINT_BYTES {
        return Err(BackendError::new(
            ErrorKind::Internal,
            "checkpoint exceeds its size bound",
        ));
    }
    let confirmed = payload
        .regions
        .iter()
        .fold(0u64, |acc, r| acc.saturating_add(r.done));
    Ok(DurableCheckpoint {
        backend: BACKEND_ID.to_owned(),
        format_version: FORMAT_VERSION,
        confirmed: ByteCount(confirmed),
        payload: bytes,
    })
}

/// Decodes and structurally checks a stored checkpoint for this destination.
fn decode(
    checkpoint: &DurableCheckpoint,
    destination_dir: &Path,
    file_name: &str,
) -> Result<Payload, BackendError> {
    if checkpoint.backend != BACKEND_ID {
        return Err(refused("it was written by another download engine"));
    }
    if checkpoint.format_version != FORMAT_VERSION {
        return Err(refused("unknown checkpoint format version"));
    }
    if checkpoint.payload.len() > MAX_CHECKPOINT_BYTES {
        return Err(refused("checkpoint too large"));
    }
    let payload: Payload = serde_json::from_slice(&checkpoint.payload)
        .map_err(|_| refused("checkpoint is corrupt"))?;
    if payload.v != FORMAT_VERSION {
        return Err(refused("unknown checkpoint format version"));
    }
    if payload.destination_dir != dir_string(destination_dir)? || payload.file_name != file_name {
        return Err(refused("it belongs to another destination"));
    }
    let etag = &payload.etag;
    if etag.len() < 2 || !etag.starts_with('"') || !etag.ends_with('"') {
        return Err(refused("no strong validator"));
    }
    if payload.length == 0 {
        return Err(refused("checkpoint is corrupt"));
    }
    // Regions must tile 0..length exactly, each with done <= its length and a 64-hex digest.
    let tiled = match payload.mode {
        Mode::Single => payload.regions.len() == 1,
        Mode::Segmented => {
            (2..=usize::from(crate::segmented::MAX_SEGMENTS)).contains(&payload.regions.len())
        }
    };
    let mut next = 0u64;
    for r in &payload.regions {
        let well_formed = r.start == next
            && r.start < r.end
            && r.done <= r.end.saturating_sub(r.start)
            && r.sha256.len() == 64
            && r.sha256.bytes().all(|b| b.is_ascii_hexdigit());
        if !well_formed {
            return Err(refused("checkpoint is corrupt"));
        }
        next = r.end;
    }
    if !tiled || next != payload.length {
        return Err(refused("checkpoint is corrupt"));
    }
    Ok(payload)
}

/// Imports a checkpoint: decodes it, re-verifies every confirmed region on disk, rebuilds the
/// hashers from the bytes, and (single mode) truncates unconfirmed bytes after the offset.
pub(crate) async fn import(
    checkpoint: &DurableCheckpoint,
    destination_dir: &Path,
    file_name: &str,
    part: &Path,
) -> Result<Restored, BackendError> {
    let payload = decode(checkpoint, destination_dir, file_name)?;
    let url = Url::parse(&payload.url).map_err(|_| refused("checkpoint is corrupt"))?;
    let identity = Identity {
        etag: Some(payload.etag.clone()),
        length: Some(payload.length),
        url,
    };
    let part: PathBuf = part.to_owned();
    let mode = payload.mode;
    let length = payload.length;
    let regions = payload.regions;
    let hashers = tokio::task::spawn_blocking(move || {
        verify_regions(&part, mode, length, &regions).map(|h| (h, regions))
    })
    .await
    .map_err(|_| BackendError::new(ErrorKind::Internal, "partial file check failed"))??;
    let (hashers, regions) = hashers;
    Ok(match mode {
        Mode::Single => {
            let (region, hasher) = regions
                .into_iter()
                .zip(hashers)
                .next()
                .ok_or_else(|| refused("checkpoint is corrupt"))?;
            Restored::Single(Checkpoint {
                identity,
                offset: region.done,
                hasher,
            })
        }
        Mode::Segmented => Restored::Segmented(SegCheckpoint {
            identity,
            length,
            segments: regions
                .into_iter()
                .zip(hashers)
                .map(|(r, hasher)| Segment {
                    start: r.start,
                    end: r.end,
                    done: r.done,
                    hasher,
                })
                .collect(),
        }),
    })
}

fn verify_regions(
    part: &Path,
    mode: Mode,
    length: u64,
    regions: &[Region],
) -> Result<Vec<Sha256>, BackendError> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(part)
    {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(changed("deleted")),
        Err(e) => return Err(io_error("cannot read partial file", &e)),
    };
    let on_disk = file
        .metadata()
        .map_err(|e| io_error("cannot read partial file", &e))?
        .len();
    let needed = match mode {
        Mode::Single => regions.first().map_or(0, |r| r.done),
        Mode::Segmented => length,
    };
    if on_disk < needed {
        return Err(changed("truncated"));
    }
    if mode == Mode::Segmented && on_disk != length {
        return Err(changed("extended"));
    }
    let mut buf = vec![0u8; WRITE_BUFFER];
    let mut hashers = Vec::with_capacity(regions.len());
    for r in regions {
        file.seek(SeekFrom::Start(r.start))
            .map_err(|e| io_error("cannot read partial file", &e))?;
        let mut hasher = Sha256::new();
        let mut left = r.done;
        while left > 0 {
            let want = usize::try_from(left.min(WRITE_BUFFER as u64)).unwrap_or(WRITE_BUFFER);
            let chunk = buf.get_mut(..want).unwrap_or_default();
            file.read_exact(chunk).map_err(|_| changed("truncated"))?;
            hasher.update(&*chunk);
            left = left.saturating_sub(want as u64);
        }
        if hex(&hasher.clone().finalize()) != r.sha256.to_ascii_lowercase() {
            return Err(changed("modified"));
        }
        hashers.push(hasher);
    }
    if mode == Mode::Single && on_disk > needed {
        // Written after the last checkpoint: not confirmed, so not trusted.
        file.set_len(needed)
            .and_then(|()| file.sync_all())
            .map_err(|e| io_error("cannot discard unconfirmed bytes", &e))?;
    }
    Ok(hashers)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn single(dir: &Path, data: &[u8], offset: usize, length: u64) -> DurableCheckpoint {
        let mut hasher = Sha256::new();
        hasher.update(&data[..offset]);
        export_single(
            &Checkpoint {
                identity: Identity {
                    etag: Some("\"v1\"".into()),
                    length: Some(length),
                    url: Url::parse("http://h/f.bin").unwrap(),
                },
                offset: offset as u64,
                hasher,
            },
            dir,
            "f.bin",
        )
        .unwrap()
        .unwrap()
    }

    #[tokio::test]
    async fn a_single_checkpoint_round_trips_and_unconfirmed_bytes_are_truncated() {
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join("f.bin.downova-part");
        let data: Vec<u8> = (0..300u32).map(|i| i as u8).collect();
        let cp = single(dir.path(), &data, 100, 300);
        assert_eq!(cp.confirmed, ByteCount(100));
        // 50 bytes written after the checkpoint (hard kill before the next one).
        std::fs::write(&part, &data[..150]).unwrap();
        let Restored::Single(r) = import(&cp, dir.path(), "f.bin", &part).await.unwrap() else {
            panic!("single expected");
        };
        assert_eq!(r.offset, 100);
        assert_eq!(
            std::fs::metadata(&part).unwrap().len(),
            100,
            "tail discarded"
        );
        let mut expect = Sha256::new();
        expect.update(&data[..100]);
        assert_eq!(
            r.hasher.finalize(),
            expect.finalize(),
            "hasher rebuilt from disk"
        );
    }

    #[tokio::test]
    async fn changed_missing_or_short_partial_files_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join("f.bin.downova-part");
        let data = vec![7u8; 300];
        let cp = single(dir.path(), &data, 100, 300);
        assert_eq!(
            import(&cp, dir.path(), "f.bin", &part)
                .await
                .unwrap_err()
                .kind,
            ErrorKind::InvalidRange,
            "missing"
        );
        std::fs::write(&part, &data[..99]).unwrap();
        let e = import(&cp, dir.path(), "f.bin", &part).await.unwrap_err();
        assert!(e.detail.contains("truncated"), "{e}");
        let mut modified = data[..100].to_vec();
        modified[42] ^= 1;
        std::fs::write(&part, &modified).unwrap();
        let e = import(&cp, dir.path(), "f.bin", &part).await.unwrap_err();
        assert!(e.detail.contains("modified"), "{e}");
        assert_eq!(std::fs::read(&part).unwrap(), modified, "nothing rewritten");
    }

    #[tokio::test]
    async fn foreign_corrupt_or_misbound_checkpoints_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join("f.bin.downova-part");
        std::fs::write(&part, vec![7u8; 100]).unwrap();
        let good = single(dir.path(), &[7u8; 300], 100, 300);
        let refuse = |cp: DurableCheckpoint, name: &'static str| {
            let dir = dir.path().to_owned();
            let part = part.clone();
            async move {
                let e = import(&cp, &dir, name, &part).await.unwrap_err();
                assert_eq!(e.kind, ErrorKind::BackendCapabilityUnavailable, "{e}");
            }
        };
        refuse(
            DurableCheckpoint {
                backend: "aria2".into(),
                ..good.clone()
            },
            "f.bin",
        )
        .await;
        refuse(
            DurableCheckpoint {
                format_version: 9,
                ..good.clone()
            },
            "f.bin",
        )
        .await;
        refuse(
            DurableCheckpoint {
                payload: b"{not json".to_vec(),
                ..good.clone()
            },
            "f.bin",
        )
        .await;
        refuse(
            DurableCheckpoint {
                payload: vec![b' '; MAX_CHECKPOINT_BYTES + 1],
                ..good.clone()
            },
            "f.bin",
        )
        .await;
        refuse(good.clone(), "other.bin").await;
        // Regions that do not tile the file.
        let mut p: Payload = serde_json::from_slice(&good.payload).unwrap();
        p.regions[0].done = 301;
        refuse(
            DurableCheckpoint {
                payload: serde_json::to_vec(&p).unwrap(),
                ..good.clone()
            },
            "f.bin",
        )
        .await;
        let mut p: Payload = serde_json::from_slice(&good.payload).unwrap();
        p.etag = "W/\"v1\"".into();
        refuse(
            DurableCheckpoint {
                payload: serde_json::to_vec(&p).unwrap(),
                ..good.clone()
            },
            "f.bin",
        )
        .await;
        // The untouched partial file was never truncated by a refused import.
        assert_eq!(std::fs::metadata(&part).unwrap().len(), 100);
    }

    #[test]
    fn transfers_without_a_strong_validator_export_nothing() {
        let cp = Checkpoint {
            identity: Identity {
                etag: None,
                length: Some(10),
                url: Url::parse("http://h/f").unwrap(),
            },
            offset: 5,
            hasher: Sha256::new(),
        };
        assert!(export_single(&cp, Path::new("d"), "f").unwrap().is_none());
    }
}
