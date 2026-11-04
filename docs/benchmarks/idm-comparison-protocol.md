# Downova versus IDM: comparison protocol (REL-04)

- **Status:** protocol only. **No IDM measurement has been run.** Every IDM row below is *unrun* until it is
  performed under this protocol, after the gates in §1.
- **Requirement:** REL-04 (requirements.md): the same device, server and network profile; no concurrent apps; at
  least 5 rotated runs with medians; at least 20 independent runs for p95; the full process tree; published scripts
  and raw results; no claim without them.
- **Downova resource baseline:** `scripts/perf/measure.mjs` on the benchmark machine (§6, W6). The local B2.33
  baseline on the development machine is in [the B2.33 report](../reports/b2.33-closeout.md#performance-baseline).
  It is a Downova-only baseline, not a comparison.
- **Revision B2.33:**
  - the fixture can now be bound to a LAN address with a fixed port, and reports server-side statistics;
  - default-settings workloads are separated from configured ones, because Downova admits 3 at a time by default;
  - the aggregate host budget is recorded rather than assumed equal;
  - the resume workload now has a server-side byte count.

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

- **Server:** `scripts/perf/fixture-process.mjs` on a **second machine** on the same wired LAN (1 Gbit/s), so the
  server's CPU never competes with the client. Start it with its LAN address and a fixed port:

  ```text
  node scripts/perf/fixture-process.mjs --count 6 --size-mib 256 --host 192.168.1.20 --port 8766
  node scripts/perf/fixture-process.mjs --count 6 --size-mib 256 --host 0.0.0.0 --public-host 192.168.1.20 --port 8766
  ```

  - The first line is JSON: `base` (`http://192.168.1.20:8766`) and every file's SHA-256. The files are
    `f0.bin … f5.bin`, range-capable with a strong ETag; `?norange=1` turns ranges off, `?slow=<ms>` paces each
    64 KiB step.
  - The default bind is `127.0.0.1` (local only). A wildcard bind without `--public-host` is refused, because the
    printed URLs would be unreachable.
  - Allow inbound TCP 8766 on the server machine's firewall for the LAN profile only.
  - `scripts/acceptance/fixture-server.mjs` binds `127.0.0.1` only and is **not** used here.
- **Server-side statistics:** `GET <base>/__stats?reset=1` after each run returns the run's statistics and starts
  counting again:
  - per file: requests, full (200) and ranged (206) responses, the first range start offsets, bytes handed to the
    socket, completed and aborted responses;
  - for all files: `max_open_responses`, the most responses the server had open at once. A response counts until the
    server ends it or notices that the client left, so this can briefly exceed a client's own count during
    connection handovers.

  Save the JSON with the run's raw results.
- **Shaping:** none at the server. Where a constrained link is wanted, shape it identically for both clients (the
  same switch or router rule) and record it.
- **Storage:** one empty folder on the same SSD for both clients, emptied before every run; record the drive model
  and its free space.
- **Cache handling:**
  - both clients download into a fresh empty folder each run, and the files are deleted between runs;
  - partial files are never reused, except in W5;
  - the server keeps its payloads in memory, so every run reads them from the same place;
  - the client's OS file cache is left as is, but the run order (§5) alternates clients, so neither always runs warm.

## 3. Settings: two comparison classes, never mixed

| Class | Downova | IDM |
|---|---|---|
| **A. Defaults** | Fresh install, default settings: global limit 3, Main 3 (at most 3 downloads active at once), at most 4 segments per file, at most 8 requests per host | Fresh install, default settings |
| **B. Configured** | Global and Main limits = the workload's file count (at most 16); segments per file 4 and the 8-request host cap (both fixed, not settings) | "Max connections number" per download = 4; the queue's simultaneous downloads = the workload's file count; speed limiter off |

- **What cannot be matched (record it, never adjust it away):**
  - Downova caps all requests to one host at **8 in total**, however many files run. Six files therefore share 8
    requests, at most 4 per file.
  - IDM's connection setting is per download, and this protocol does not assume IDM has an equivalent per-host cap.
    Six files at 4 connections each may open up to 24.
  - Class B matches the per-file setting and the number of simultaneous files, **not the aggregate host budget**.
    Record the budget each client actually used: `max_open_responses` from the server, and, for Downova, its 8-request
    cap. Report it beside every class B result.
- **Both classes:** browser integration off, so no capture interferes; antivirus scanning after download off, or
  the same for both; no scheduler; no speed limit.
- **Record** every other changed setting with its value, and any IDM setting that has no Downova counterpart.

## 4. Workloads (each with known SHA-256)

| ID | Workload | Server command (plus `--host`/`--port`, §2) | Checked |
|---|---|---|---|
| W1 | One large range-capable file, 1 GiB | `--count 1 --size-mib 1024` | completion time, mean throughput, SHA-256, `max_open_responses` |
| W2a | Six files, 256 MiB each, one host, **class A**: all six added at once. Downova admits 3 and queues 3; IDM does what its defaults do | `--count 6 --size-mib 256` | time until all six complete, SHA-256 of all six, how many were active at once (each client's own list, and `max_open_responses`) |
| W2b | The same six files, **class B**: both clients allow six at once | as W2a | every file receiving bytes before the first completes (Downova: `dm list` per file, or `measure.mjs` `active_6`'s activity record; IDM: its progress per file), completion time, SHA-256 of all six, `max_open_responses` |
| W3 | Six files across two hosts (3 + 3), class B | two `fixture-process.mjs` instances under two different host names: two LAN addresses, or one address with a second name in the client's hosts file passed as `--public-host`. Downova's budget is keyed by host name without the port, so two ports under one name are **one** host; record which setup was used | as W2b, per host |
| W4 | Non-range fallback: one 256 MiB file with ranges ignored | `--count 1 --size-mib 256`, URL `<base>/f0.bin?norange=1` (always 200, no `Accept-Ranges`) | completes as one stream (`/__stats`: one full response, no ranged ones), SHA-256 |
| W5 | Interrupted transfer and resume: W1 with the client's network cable unplugged at about 50 % for 30 s, then reconnected | as W1 (pace with `?slow=` so 50 % is reachable by hand) | it resumes rather than restarts: `/__stats` shows ranged responses whose `range_starts` include an offset past 0 after the interruption; total `bytes_written` ≤ 1.05 × the file size (an upper bound of what reached the client); SHA-256 |
| W6 | Resources: idle (window visible), idle hidden to the tray, after the UI is closed (Downova only: service running), and during W1 and W2b | — | private bytes per process and summed (not summed working sets), CPU % over 10 s, process count, startup time to a usable window |

## 5. Run order and counts

- **Rotation:** ABBA by run (Downova, IDM, IDM, Downova, …), and each workload's order rotated between runs.
- **Counts:** at least **5 runs** per client, workload and class for medians; at least **20 independent runs**
  before any p95 is published (run-level percentiles, never pooled observations within runs).
- **Failures:** a failed or excluded run is kept in the raw results with its reason. A run is excluded only for a
  documented external cause (for example a network drop that was not part of W5). `measure.mjs` marks an active run
  `invalid` when its workload was not what it claims (B2.33) and lists it under `excluded`.

## 6. What is recorded per run

- **Identity:** client and version, package SHA-256 (Downova) or installer version (IDM), class, workload, run
  number and order position; the machine (CPU, RAM, drive, OS build), the server machine and the link.
- **Timing:** start time, completion time per file, total.
- **Bytes:** bytes written per file, compared with the expected size; the server's `/__stats` for the run.
- **Integrity:** SHA-256 per file, compared with the fixture's.
- **Resources (W6):** the whole process tree of each client.
  - Downova: desktop, WebView2 processes, service (and aria2 if used); `measure.mjs` does this for Downova.
  - IDM: IDMan and its helper processes, sampled with the same counters: private bytes per process and in total,
    CPU, process count.
  - Private bytes exclude pages shared between processes (mapped DLLs, shared WebView2 memory), so neither client's
    sum double-counts them. Summed working sets over-count shared pages and are not compared.
- **Raw files:** samples, a CSV and a summary JSON per run, plus the `/__stats` JSON.

## 7. Reporting rules

- **Labels:** medians with the run count; p95 only with n ≥ 20 independent runs; ranges always.
- **Classes:** A and B are reported separately; no "faster" or "lighter" claim crosses classes.
- **Host budget:** every class B result names the aggregate requests each client used (§3).
- **No inference beyond the data:** no claim of equal bandwidth per file, and no claim beyond the measured
  workloads and machine.
- **CI runners:** results from shared CI runners are diagnostic only and never compared with IDM.

## 8. Result table (fill in only after running)

| Workload | Class | Downova (median, n) | IDM (median, n) | Host budget used (Downova / IDM) | Notes |
|---|---|---|---|---|---|
| W1–W6 | A, B | unrun | unrun | unrun | — |
