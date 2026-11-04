# Downova versus IDM: comparison protocol (REL-04)

- **Status:** protocol only. **No IDM measurement has been run.** Every IDM row below is *unrun* until it is
  performed under this protocol, after the gates in §1.
- **Requirement:** REL-04 (requirements.md): the same device, server and network profile; no concurrent apps; at
  least 5 rotated runs with medians; at least 20 runs for p95; the full process tree; published scripts and raw
  results; no claim without them.
- **Downova baseline:** `scripts/perf/measure.mjs` (resources) and this document's transfer workloads (§4). The
  Downova numbers recorded so far are in [the B2.32 report](../reports/configurable-concurrency.md).

## 1. Gates before any IDM run

1. **B2 exited.** Remote CI and package smoke are green on all platforms, and the installed-app and human acceptance
   items in PROGRESS.md are done.
2. **A normal, installed Downova build is recorded** (package SHA-256, source commit, any source diff) on the
   benchmark machine.
3. **The same Windows machine for both**, with:
   - AC power, the "Best performance" power mode, and Windows Update paused;
   - no other user applications running, and real-time antivirus behaving the same for both: either excluded for
     the benchmark folders for both, or for neither (record which).
4. **IDM version and settings recorded** (§3), from IDM's own About dialog and settings pages.

## 2. Server, storage and network profile

- **Server:**
  - The repository fixture, run on a **second machine** on the same wired LAN (1 Gbit/s), so the server's CPU never
    competes with the client.
  - `node scripts/acceptance/fixture-server.mjs --port 8765` serves `public.bin` and the deterministic scenarios.
  - `node scripts/perf/fixture-process.mjs --count N --size-mib M` serves `f0…fN-1.bin`, range-capable with a
    strong ETag, and prints every file's SHA-256 on its first line.
- **Shaping:** none at the server. Where a constrained link is wanted, shape it identically for both clients (the
  same switch or router rule) and record it.
- **Storage:** one empty folder on the same SSD for both clients, emptied before every run; record the drive model
  and its free space.
- **Cache handling:** both clients download to a fresh empty folder each run. Between runs, delete the files. Do not
  reuse partial files, except in the resume workload. The OS file cache is left as is, but the run order (§5)
  alternates clients, so neither always runs warm.

## 3. Settings: two comparison classes, never mixed

| Class | Downova | IDM |
|---|---|---|
| **A. Defaults** | Fresh install, default settings (global limit 3, Main 3, at most 4 segments per file, 8 requests per host) | Fresh install, default settings |
| **B. Matched limits** | Global and Main limits = the workload's file count (at most 16); segments per file 4 (fixed); per-host requests 8 (fixed) | "Max connections number" = 4 per file; simultaneous downloads in the queue = the workload's file count; speed limiter off |

- **Both classes:** browser integration off, so no capture interferes; antivirus scanning after download off, or
  the same for both; no scheduler; no speed limit.
- **Record** every other changed setting with its value.

## 4. Workloads (each with known SHA-256)

| ID | Workload | Server command | Checked |
|---|---|---|---|
| W1 | One large range-capable file, 1 GiB | `fixture-process.mjs --count 1 --size-mib 1024` | completion time, mean throughput, SHA-256 |
| W2 | Six simultaneous files, 256 MiB each, one host | `--count 6 --size-mib 256` | every file receiving bytes before the first completes (Downova: `dm list` or the measure tool's activity check; IDM: its progress per file), completion time, SHA-256 of all six |
| W3 | Six files across two hosts (3 + 3) | two `fixture-process.mjs` instances on two LAN addresses | as W2 |
| W4 | Non-range fallback: one 256 MiB file with ranges ignored | `fixture-process.mjs --count 1 --size-mib 256`, URL `…/f0.bin?norange=1` (always 200, no `Accept-Ranges`) | completes as one stream, SHA-256 |
| W5 | Interrupted transfer and resume: W1, network unplugged at 50 % for 30 s, then reconnected | as W1 | resumes (not restarts), total bytes from the server ≤ 1.05 × file size (the fixture logs bytes served), SHA-256 |
| W6 | Resources: idle (window visible), idle hidden to the tray, after the UI is closed (Downova only: service running), and during W1 and W2 | — | private bytes per process and summed (not summed working sets), CPU % over 10 s, process count, startup time to a usable window |

## 5. Run order and counts

- **Rotation:** ABBA by run (Downova, IDM, IDM, Downova, …), and each workload's order rotated between runs.
- **Counts:** at least **5 runs** per client, workload and class for medians; at least **20 runs** before any p95
  is published (run-level percentiles).
- **Failures:** a failed or excluded run is kept in the raw results with its reason. A run is excluded only for a
  documented external cause (for example a network drop that was not part of W5).

## 6. What is recorded per run

- **Identity:** client and version, package SHA-256 (Downova) or installer version (IDM), class, workload, run
  number and order position.
- **Timing:** start time, completion time per file, total.
- **Bytes:** bytes written per file, compared with the expected size; bytes served (fixture log).
- **Integrity:** SHA-256 per file, compared with the fixture's.
- **Resources (W6):** the whole process tree of each client. For Downova: desktop, WebView2 processes, service
  (and aria2 if used). For IDM: IDMan and its helper processes. Private bytes per process and total, CPU, process
  count.
- **Raw files:** samples, a CSV and a summary JSON per run.

## 7. Reporting rules

- **Labels:** medians with the run count; p95 only with n ≥ 20; ranges always.
- **Classes:** A and B are reported separately; no "faster" or "lighter" claim crosses classes.
- **No inference beyond the data:** no claim of equal bandwidth per file, and no claim beyond the measured
  workloads and machine.
- **CI runners:** results from shared CI runners are diagnostic only and never compared with IDM.

## 8. Result table (fill in only after running)

| Workload | Class | Downova (median, n) | IDM (median, n) | Notes |
|---|---|---|---|---|
| W1–W6 | A, B | unrun | unrun | — |
