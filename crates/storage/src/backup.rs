//! Consistent database backups for updates (ADV09, ADR 0030).
//!
//! The service is the only SQLite writer, so it is also the only thing that copies the database.
//! [`Store::backup_to`] uses `VACUUM INTO` on the service's own connection: SQLite writes one
//! transaction-consistent snapshot, including content that is still in the WAL, into a new file
//! (never over an existing one). [`check_backup`] reopens the copy read-only and checks it
//! (`integrity_check`, schema version, job count) and hashes it; a backup is only offered for
//! recovery when that check passes and matches what was recorded. [`restore_backup`] replaces a
//! database that cannot be used with a checked backup, setting the old files aside (never
//! deleting them); it must only run while no [`Store`] is open and the caller holds the
//! service's instance lock.

use std::io::Write;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};

use crate::{StorageError, Store, sql};

/// What a checked backup contains.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupCheck {
    pub schema_version: u32,
    pub jobs: u64,
    pub size_bytes: u64,
    /// SHA-256 of the backup file, lower-case hex.
    pub sha256: String,
}

fn sha256_file(path: &Path) -> Result<(u64, String), StorageError> {
    let bytes = std::fs::read(path).map_err(|e| StorageError::Open(e.kind().to_string()))?;
    let digest = ring::digest::digest(&ring::digest::SHA256, &bytes);
    let hex: String = digest.as_ref().iter().map(|b| format!("{b:02x}")).collect();
    Ok((u64::try_from(bytes.len()).unwrap_or(u64::MAX), hex))
}

impl Store {
    /// Writes a consistent copy of the database to `target` (which must not exist), syncs it,
    /// and checks it. The copy is taken by this connection, so nothing else ever reads the live
    /// file.
    pub fn backup_to(&self, target: &Path) -> Result<BackupCheck, StorageError> {
        if target.exists() {
            return Err(StorageError::Open("the backup file already exists".into()));
        }
        let name = target
            .to_str()
            .ok_or_else(|| StorageError::Open("the backup path is not UTF-8".into()))?;
        self.conn.execute("VACUUM INTO ?1", [name]).map_err(sql)?;
        // Synced through a write handle: on Windows, FlushFileBuffers needs write access (a
        // read-only handle fails with "permission denied").
        std::fs::OpenOptions::new()
            .write(true)
            .open(target)
            .and_then(|f| f.sync_all())
            .map_err(|e| StorageError::Open(e.kind().to_string()))?;
        let check = check_backup(target)?;
        let live = self.schema_version()?;
        if check.schema_version != live {
            return Err(StorageError::Corrupt(
                "the backup's schema version differs from the database".into(),
            ));
        }
        Ok(check)
    }
}

/// Opens `path` read-only and checks it: SQLite's `integrity_check` must answer `ok`, and the
/// job table must be readable. Returns its schema version, job count, size and SHA-256.
pub fn check_backup(path: &Path) -> Result<BackupCheck, StorageError> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| StorageError::Open(e.to_string()))?;
    let integrity: String = conn
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .map_err(sql)?;
    if integrity != "ok" {
        return Err(StorageError::Corrupt(
            "the backup failed SQLite's integrity check".into(),
        ));
    }
    let schema_version: u32 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(sql)?;
    let jobs: i64 = conn
        .query_row("SELECT count(*) FROM jobs", [], |r| r.get(0))
        .map_err(sql)?;
    drop(conn);
    let (size_bytes, sha256) = sha256_file(path)?;
    Ok(BackupCheck {
        schema_version,
        jobs: u64::try_from(jobs).unwrap_or(0),
        size_bytes,
        sha256,
    })
}

/// The files SQLite keeps beside a database.
fn companions(db: &Path) -> [PathBuf; 3] {
    let with = |suffix: &str| {
        let mut name = db.as_os_str().to_owned();
        name.push(suffix);
        PathBuf::from(name)
    };
    [db.to_owned(), with("-wal"), with("-shm")]
}

/// Replaces the database at `db` with the checked `backup` (whose SHA-256 must equal `sha256`).
/// The current database and its WAL/SHM files are moved into `set_aside` (created), never
/// deleted; the backup is copied (it stays available), synced, and renamed into place.
///
/// The caller must hold the service's instance lock and have no [`Store`] open on `db`.
pub fn restore_backup(
    db: &Path,
    backup: &Path,
    sha256: &str,
    set_aside: &Path,
) -> Result<BackupCheck, StorageError> {
    let check = check_backup(backup)?;
    if check.sha256 != sha256 {
        return Err(StorageError::Corrupt(
            "the backup does not match its recorded checksum".into(),
        ));
    }
    let io = |e: std::io::Error| StorageError::Open(e.kind().to_string());
    std::fs::create_dir_all(set_aside).map_err(io)?;
    for f in companions(db) {
        if f.exists() {
            let name = f
                .file_name()
                .ok_or_else(|| StorageError::Open("bad path".into()))?;
            std::fs::rename(&f, set_aside.join(name)).map_err(io)?;
        }
    }
    let dir = db
        .parent()
        .ok_or_else(|| StorageError::Open("bad path".into()))?;
    let tmp = dir.join(".downova-restore.tmp");
    let _ = std::fs::remove_file(&tmp);
    let bytes = std::fs::read(backup).map_err(io)?;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .map_err(io)?;
    f.write_all(&bytes).map_err(io)?;
    f.sync_all().map_err(io)?;
    drop(f);
    std::fs::rename(&tmp, db).map_err(io)?;
    Ok(check)
}
