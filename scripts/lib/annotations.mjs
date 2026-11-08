// GitHub Actions annotations for the packaged smoke and the real-browser test (B2.6, B2.28).
//
// Annotations are the only evidence readable through the public API without signing in (job logs
// and artifacts are not). GitHub cuts an annotation message at about 4 KB, which hid most failed
// rows of a ~96-row run (runs #30–#36 showed 51–56 lines). So:
// - the summary annotation lists every non-passed row in full first, then the passed rows by
//   their short ID only, and stays under the limit;
// - every non-passed row also gets its own annotation with its detail (bounded), up to
//   MAX_DETAIL_ANNOTATIONS, so the cause of a failure is readable remotely.

/** GitHub's per-annotation message limit is about 4 KB; stay below it. */
export const ANNOTATION_BYTES = 3_800;
/** GitHub shows at most 10 error annotations per step. One goes to the summary. */
export const MAX_DETAIL_ANNOTATIONS = 9;

const escapeData = (v) => v.replace(/%/g, "%25").replace(/\r/g, "%0D").replace(/\n/g, "%0A");
const escapeTitle = (v) => escapeData(v).replace(/[,:]/g, " ");

/** Cuts `text` to at most `bytes` UTF-8 bytes on a character boundary, marking the cut. */
export function clip(text, bytes = ANNOTATION_BYTES) {
  const buf = Buffer.from(text, "utf8");
  if (buf.length <= bytes) return text;
  const mark = " …[cut]";
  let cut = buf.subarray(0, bytes - Buffer.byteLength(mark)).toString("utf8");
  // A cut inside a multi-byte character decodes as U+FFFD: drop it.
  cut = cut.replace(/�+$/, "");
  return cut + mark;
}

/** The row's short ID: its leading token ("P2", "R3b", "D2"), or the start of its text. */
export function rowId(step) {
  const m = /^([A-Z][A-Za-z0-9]*\d[a-z]?|[A-Z]\d*[a-z]?)\b/.exec(step);
  return m ? m[1] : step.slice(0, 24);
}

/** Consecutive equal IDs as one entry with a count ("R3×22"). */
function runs(ids) {
  const out = [];
  for (const id of ids) {
    const last = out[out.length - 1];
    if (last && last.id === id) last.n += 1;
    else out.push({ id, n: 1 });
  }
  return out.map(({ id, n }) => (n > 1 ? `${id}×${n}` : id));
}

/**
 * The annotation commands for `results` ({ verdict, steps: [{ step, status, ...detail }] }), as
 * strings to print. Pure: callers decide whether they run on GitHub Actions.
 */
export function annotationLines(title, results) {
  const counts = {};
  for (const s of results.steps) counts[s.status] = (counts[s.status] ?? 0) + 1;
  const notPassed = results.steps.filter((s) => s.status !== "passed");
  const passed = results.steps.filter((s) => s.status === "passed");
  const summary = clip(
    [
      `verdict ${results.verdict}; ${Object.entries(counts)
        .map(([k, n]) => `${n} ${k}`)
        .join(", ")}`,
      ...notPassed.map((s) => `${s.status.toUpperCase()} ${s.step}`),
      `PASSED (${passed.length}): ${runs(passed.map((s) => rowId(s.step))).join(" ")}`,
    ].join("\n"),
  );
  const level = results.verdict === "failed" ? "error" : "notice";
  const out = [`::${level} title=${escapeTitle(title)}::${escapeData(summary)}`];
  for (const s of notPassed.slice(0, MAX_DETAIL_ANNOTATIONS)) {
    const { step, status, ...detail } = s;
    const rowLevel = status === "failed" ? "error" : "warning";
    const text = clip(`${status.toUpperCase()} ${step}\n${JSON.stringify(detail)}`);
    out.push(
      `::${rowLevel} title=${escapeTitle(`${title} — ${rowId(step)}`)}::${escapeData(text)}`,
    );
  }
  return out;
}

/** Prints the annotations when running on GitHub Actions. */
export function publishRows(title, results) {
  if (process.env.GITHUB_ACTIONS !== "true") return;
  for (const line of annotationLines(title, results)) console.log(line);
}

/**
 * One error annotation for a failed build step (B3.7 follow-up): the telling lines of its log
 * first (errors, HTTP statuses, timeouts, package/retry messages), then its last lines, clipped
 * under the annotation limit. Job logs need admin rights to read; annotations do not.
 */
export function logTailAnnotation(title, log, tailLines = 40) {
  const lines = log
    .replace(/\r/g, "")
    .split("\n")
    .filter((l) => l.trim() !== "");
  const telling = lines
    .filter((l) =>
      /error|failed|failure|panicked|http status|status code|\b5\d\d\b|timeout|timed out|hash mismatch|package: |retrying|Downloading/i.test(
        l,
      ),
    )
    .slice(-25);
  const text = [...telling, "--- last lines ---", ...lines.slice(-tailLines)].join("\n");
  return `::error title=${escapeTitle(title)}::${escapeData(clip(text))}`;
}
