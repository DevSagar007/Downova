// Test-only history seeding (perf measurements and packaged smoke, ADV12). Writes `count` finished
// jobs straight into a Downova database that the service created and then released (it holds the
// database exclusively while running). Never used by a product build; the records live only in
// the caller's isolated data folder.
import { DatabaseSync } from "node:sqlite";

const NAMES = [
  "report",
  "দলিল-প্রতিবেদন",
  "video-lecture",
  "dataset",
  "installer",
  "সংগীত-অ্যালবাম",
];
const EXTS = ["pdf", "mp4", "zip", "iso", "mp3"];
const CATEGORY = {
  pdf: "documents",
  mp4: "video",
  zip: "compressed",
  iso: "compressed",
  mp3: "music",
};

/** Display name of seeded record `n` (deterministic; includes Bangla names). */
export const seededName = (n) => `${NAMES[n % NAMES.length]}-${n}.${EXTS[n % EXTS.length]}`;

export function seedHistory(dbFile, downloadDir, count) {
  const db = new DatabaseSync(dbFile);
  const insert =
    db.prepare(`INSERT INTO jobs (job_id, url, display_name, destination, desired_state,
      phase, downloaded_bytes, total_bytes, expected_sha256, error_json, created_at_ms, updated_at_ms,
      attempt_id, verified_sha256, verification_source, staging_name, name_source, queue_id, priority,
      queue_position, category, name_confirmed, finished_at_ms)
    VALUES (?, ?, ?, ?, 'running', ?, ?, ?, NULL, ?, ?, ?, NULL, ?, ?, ?, 'legacy', 'main', 0, ?, ?, 1, ?)`);
  const now = Date.now();
  db.exec("BEGIN");
  for (let n = 0; n < count; n++) {
    const name = seededName(n);
    const ext = EXTS[n % EXTS.length];
    const failed = n % 17 === 0;
    const size = 1_000_000 + ((n * 7919) % 50_000_000);
    const at = now - (count - n) * 1000;
    insert.run(
      `job_${(0x10000000 + n).toString(16).padStart(16, "0")}`,
      `http://127.0.0.1:9/history/${name}`,
      name,
      downloadDir,
      failed ? "failed" : "completed",
      failed ? 0 : size,
      size,
      failed
        ? JSON.stringify({
            kind: "NETWORK_OFFLINE",
            retryable: true,
            next_action: "retry",
            detail: null,
          })
        : null,
      at,
      at,
      failed ? null : "a".repeat(64),
      failed ? null : "local_hash",
      name,
      n + 1,
      CATEGORY[ext],
      at,
    );
  }
  db.exec("COMMIT");
  // Seeded history is old news: no notification decision is pending for it (migration 9 marks
  // pre-existing finished jobs the same way).
  try {
    db.exec(
      "UPDATE jobs SET notify_state = 'skipped' WHERE notify_state IS NULL AND phase IN ('completed','failed','cancelled')",
    );
  } catch {
    // An older schema without notify_state (the baseline package): nothing to mark.
  }
  db.close();
}
