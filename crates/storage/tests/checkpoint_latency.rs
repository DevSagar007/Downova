//! COR-08 benchmark: the cost of a durable checkpoint commit with the production settings
//! (WAL, `synchronous=FULL`, exclusive locking), measured through `Store::save_checkpoint` on
//! isolated temporary databases. Ignored in normal runs; run it explicitly:
//!
//! ```text
//! cargo test -p dm-storage --release --test checkpoint_latency -- --ignored --nocapture
//! ```
//!
//! `DOWNOVA_BENCH_SAMPLES` (default 200) sets the commits per configuration and
//! `DOWNOVA_BENCH_DIR` the parent folder of the temporary databases (default: the system temp
//! folder, which decides the device being measured). For comparison only, the same two
//! statements are also timed on a raw connection with `synchronous=NORMAL`; production never uses
//! that mode. A process kill cannot lose a committed transaction in either mode; the difference
//! is what a power loss may lose (SQLite docs, "WAL mode" and "PRAGMA synchronous").

#![allow(
    clippy::unwrap_used,
    clippy::print_stdout,
    clippy::arithmetic_side_effects,
    clippy::cast_precision_loss
)]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use dm_domain::{AttemptId, ByteCount, DesiredState, JobId, Phase};
use dm_storage::{CheckpointRow, FILE_NAME, JobRow, Store};

/// Representative checkpoint payloads: single connection (one region), segmented (16 regions
/// with digests) and the format's upper bound.
const PAYLOADS: [(&str, usize); 3] = [
    ("single 512 B", 512),
    ("segmented 4 KiB", 4 * 1024),
    ("maximum 64 KiB", 64 * 1024),
];

fn samples() -> usize {
    std::env::var("DOWNOVA_BENCH_SAMPLES")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|n| *n >= 10)
        .unwrap_or(200)
}

fn temp_dir() -> tempfile::TempDir {
    match std::env::var_os("DOWNOVA_BENCH_DIR") {
        Some(parent) => tempfile::tempdir_in(parent).unwrap(),
        None => tempfile::tempdir().unwrap(),
    }
}

fn job(attempt: &AttemptId) -> JobRow {
    JobRow {
        id: JobId::parse("job_bench00000001").unwrap(),
        url: "https://example.org/large.iso".into(),
        display_name: "large.iso".into(),
        staging_name: "large.iso".into(),
        name_source: Some("request_url".into()),
        destination: PathBuf::from("downloads"),
        desired_state: DesiredState::Running,
        phase: Phase::Transferring,
        downloaded: ByteCount(0),
        total: Some(ByteCount(8 << 30)),
        expected_sha256: None,
        error: None,
        attempt_id: Some(attempt.clone()),
        verification: None,
        created_at_ms: 1,
        updated_at_ms: 1,
        queue_id: dm_storage::DEFAULT_QUEUE.into(),
        priority: dm_domain::policy::Priority::Normal,
        queue_position: 0,
        category: None,
        name_confirmed: true,
    }
}

struct Stats {
    n: usize,
    mean: Duration,
    p50: Duration,
    p90: Duration,
    p99: Duration,
    max: Duration,
}

fn stats(mut d: Vec<Duration>) -> Stats {
    d.sort();
    let at = |q: f64| d[(((d.len() - 1) as f64) * q).round() as usize];
    Stats {
        n: d.len(),
        mean: d.iter().sum::<Duration>() / u32::try_from(d.len()).unwrap(),
        p50: at(0.50),
        p90: at(0.90),
        p99: at(0.99),
        max: *d.last().unwrap(),
    }
}

fn print(label: &str, s: &Stats) {
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    println!(
        "{label:<38} n={:<5} mean={:>7.3} ms  p50={:>7.3}  p90={:>7.3}  p99={:>7.3}  max={:>7.3}",
        s.n,
        ms(s.mean),
        ms(s.p50),
        ms(s.p90),
        ms(s.p99),
        ms(s.max)
    );
}

/// Production path: one job row update plus one checkpoint upsert per transaction.
fn production(dir: &Path, payload: usize, n: usize) -> Stats {
    let attempt = AttemptId::parse("att_bench00000001").unwrap();
    let mut store = Store::open(&dir.join(FILE_NAME)).unwrap();
    let mut row = job(&attempt);
    store.insert_job(&row, None).unwrap();
    let mut times = Vec::with_capacity(n);
    for i in 1..=n as u64 {
        row.downloaded = ByteCount(i * (4 << 20));
        row.updated_at_ms = i;
        let checkpoint = CheckpointRow {
            job_id: row.id.clone(),
            attempt_id: attempt.clone(),
            backend_id: "http".into(),
            backend_version: Some("0.1.0".into()),
            format_version: 1,
            confirmed: row.downloaded,
            // Varying bytes, so no page is rewritten with identical content.
            payload: (0..payload).map(|b| (b as u64 ^ i) as u8).collect(),
            updated_at_ms: i,
        };
        let started = Instant::now();
        store.save_checkpoint(&row, &checkpoint).unwrap();
        times.push(started.elapsed());
    }
    let reopened = Store::open(&dir.join(FILE_NAME));
    drop(store);
    // The exclusive lock of the open store refuses a second opener.
    assert!(reopened.is_err());
    let stored = Store::open(&dir.join(FILE_NAME))
        .unwrap()
        .load_checkpoint(&row.id)
        .unwrap()
        .unwrap();
    assert_eq!(
        stored.confirmed, row.downloaded,
        "the last commit is durable"
    );
    stats(times)
}

/// Comparison only: the same statements on a raw connection in the given synchronous mode.
fn raw(dir: &Path, payload: usize, n: usize, synchronous: &str) -> Stats {
    let path = dir.join(FILE_NAME);
    let attempt = AttemptId::parse("att_bench00000001").unwrap();
    {
        let mut store = Store::open(&path).unwrap();
        store.insert_job(&job(&attempt), None).unwrap();
    }
    let mut conn = rusqlite::Connection::open(&path).unwrap();
    let mode: String = conn
        .query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))
        .unwrap();
    assert_eq!(mode, "wal");
    conn.pragma_update(None, "synchronous", synchronous)
        .unwrap();
    let mut times = Vec::with_capacity(n);
    for i in 1..=n as i64 {
        let blob: Vec<u8> = (0..payload).map(|b| (b as i64 ^ i) as u8).collect();
        let started = Instant::now();
        let tx = conn.transaction().unwrap();
        tx.execute(
            "UPDATE jobs SET downloaded_bytes = ?1, updated_at_ms = ?2 WHERE job_id = ?3",
            rusqlite::params![i * (4 << 20), i, "job_bench00000001"],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO resume_checkpoints (job_id, attempt_id, backend_id, backend_version,
                                             format_version, confirmed_bytes, payload, updated_at_ms)
             VALUES (?1, ?2, 'http', '0.1.0', 1, ?3, ?4, ?5)
             ON CONFLICT (job_id) DO UPDATE SET confirmed_bytes = excluded.confirmed_bytes,
                 payload = excluded.payload, updated_at_ms = excluded.updated_at_ms",
            rusqlite::params!["job_bench00000001", attempt.as_str(), i * (4 << 20), blob, i],
        )
        .unwrap();
        tx.commit().unwrap();
        times.push(started.elapsed());
    }
    stats(times)
}

#[test]
#[ignore = "benchmark; run with --release --ignored --nocapture (see the module notes)"]
fn checkpoint_commit_latency() {
    let n = samples();
    let parent = temp_dir();
    println!(
        "COR-08 checkpoint commit latency: {} {} ({} build), {n} commits per row, databases under {}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        parent.path().display()
    );
    for (label, payload) in PAYLOADS {
        let full = production(temp_dir_in(parent.path()).path(), payload, n);
        print(&format!("FULL (production) {label}"), &full);
        let normal = raw(temp_dir_in(parent.path()).path(), payload, n, "NORMAL");
        print(&format!("NORMAL (comparison) {label}"), &normal);
        assert_eq!(full.n, n);
    }
}

fn temp_dir_in(parent: &Path) -> tempfile::TempDir {
    tempfile::tempdir_in(parent).unwrap()
}
