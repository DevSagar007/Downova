# ADR 0010 — Per-host connection budget and active-file admission

- **Status:** Accepted
- **Date:** 2026-10-26
- **Builds on:** ADR 0002 (backend seam), ADR 0004/0005 (`dm-backend-http`), ADR 0007–0009
- **Requirements:** CORE-02, E03, CORE-03, COR-07
- **Evidence:** [B1.5 connection-budget report](../reports/b1.5-connection-budget.md)

## Context

CORE-02 sets the starting policy:
- 2–4 connections for large range-capable files;
- one connection for small or unsupported ones;
- **at most 8 connections per host across all jobs**;
- **at most 3 active files**.

Until B1.5, every accepted job started at once, and each segmented file opened up to 4 connections regardless of what
other jobs did. Nothing bounded the load on one server.

## Decision

1. **Per-host budget, enforced where the requests are made** (`dm-backend-http`, `limits.rs`).
   - **What it covers:** every request the backend sends, each counted once while it lives:
     - the probe;
     - a single-connection transfer;
     - a resumed `Range`;
     - each segment worker;
     - each redirect hop.
   - **How it is held:** the request takes a permit for its destination host **before** it is sent and holds it until
     the response and body are dropped. Permits are RAII values owned by the request's task, so pause, cancel,
     timeout, failure and completion release them, and no attempt can hold or release another attempt's permit.
   - **Redirects:** a redirect drops the previous hop's response and permit, then takes a permit for the next hop's
     host.
   - **Waiting** for a permit is part of the request and stops with the attempt (pause or cancel).
   - **Budget:** `HttpConfig.max_connections_per_host`, default 8, shared by all attempts of the backend.
   - **Host key:** `url.host_str()` (ASCII-lowercased; internationalised names in punycode), trailing dot removed, IPv6
     literal as written. **Ports and schemes are ignored**: one server name is one budget, the conservative choice.
   - **No idle pool:** `pool_max_idle_per_host(0)`, so a request's connection closes with its permit. The open TCP
     connections Downova keeps to a host therefore follow the permits.
   - **The guarantee:** at most 8 requests to one host are open from Downova at any time, and so at most 8 connections.
     A server observes a dropped connection only at its next read or write, so its own count can briefly include
     connections Downova already closed. Tests measure the server's view with an explicit 100 ms teardown allowance
     and report the raw count as well.
2. **Active-file admission in the service** (`Core::admit`, `MAX_ACTIVE_FILES = 3`).
   - **Queued work:** a job ready to transfer gets a *pending* start and shows `queued` with the `queue_limit` blocker.
     Ready means: newly added, resumed by the user, restored after a restart, due after a retry cool-down, or cleared
     by the disk policy. Downloads are always accepted and persisted first.
   - **Admission order:** `admit` counts the jobs occupying a slot and starts pending jobs while slots are free.
     Order is oldest first (`created_at`, then job ID): deterministic and starvation-free. Priorities and user queues
     are B2.
   - **Who occupies a slot:** a live attempt the user wants running that is not pending, finalizing (`verifying`),
     waiting for disk or cooling down for a retry. Paused, waiting, disk-blocked and cooling-down jobs never hold a
     slot.
   - **Non-blocking:** admission counts and starts; it never waits. Waiting for per-host capacity happens inside the
     backend's request tasks, so RPCs and the coordinator lock are never held waiting for capacity.
   - **User intent is preserved:**
     - a job paused by the user is not admitted;
     - cancel removes pending work and a cancelled job is never revived;
     - clearing `queue_limit` never overrides another blocker (only jobs whose sole blocker is `queue_limit` are admitted);
     - disk (ADR 0009) and `Retry-After` policies are unchanged;
     - a resume that can start at once still returns its refusal to the caller (e.g. no strong validator). A resume
       refused later at admission leaves the job paused with the reason, never failed.
   - **After a restart:** recovered attempts are restored paused (no network) and then admitted like any pending job,
     so the limits hold before any recovered job transfers. Capacity is rebuilt from the live jobs, never persisted. A
     job that was accepted but never started stays queued instead of failing as interrupted.
3. **aria2** (optional) does not use the HTTP limiter.
   - Its accounting is conservative: the service passes one connection per file (aria2 reports `SegmentedHttp`
     unsupported) and admits at most 3 files, so at most 3 aria2 connections to any host exist.
   - Its redirects reuse that one connection. No backend is switched silently.

## Consequences

- One server sees at most 8 connections from Downova. With three large segmented files, some segment workers wait for a
  slot.
- No speed improvement is claimed; that needs REL-04 benchmarks.
- Without an idle pool, every request opens a new connection. That costs one handshake per request (a probe, then each
  segment), measurable on high-latency HTTPS, not benchmarked.
- The 3-file and 8-per-host values are fixed (settings are B2/B4).
- Open: the per-host budget is per backend instance, so another program or a second Downova service is not counted. HTTP/2 multiplexing would need a
  separate transport decision (E03; the backend is HTTP/1.1 only).
