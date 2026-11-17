# ADR 0044 — Resume with a strong `Last-Modified` and a byte-overlap proof

- **Status:** Accepted
- **Date:** 2026-11-17
- **Decided by:** the maintainer, for the owner's priority 2 ("resume and recovery: a network drop, a service
  restart, an expired link or a changed file must behave correctly; the finished file's bytes come first")
- **Amends:** COR-02 acceptance ("only a strong ETag … is used for If-Range"), ADR 0007 (durable checkpoint format)
- **Requirements:** COR-02, COR-01, COR-08

## Context

The native engine (`dm-backend-http`) resumed only when the first `200` carried a strong ETag and a length.

Many servers send no ETag, or only a weak one, but do send a `Last-Modified` date:
- static file servers behind some CDNs;
- object stores behind proxies that strip the ETag;
- download scripts.

For those files every pause, network drop or service restart restarted the whole file from zero. This was explicit
(`BACKEND_CAPABILITY_UNAVAILABLE`), but a long download on a weak connection never finished.

RFC 9110 §13.1.5 allows a `Last-Modified` date in `If-Range` only when it is strong: at least one second before the
response's own `Date`. A date is still a weaker promise than an entity tag. A file rewritten within the same second, or
a server that keeps the date while the content changes, would pass the date check.

## Decision

1. **Validator.** The identity of a full `200` keeps a `Last-Modified` only when it is strong (a valid IMF-fixdate
   at least one second before `Date`), exactly as sent.
   - Resume is available with a length and either a strong ETag or a strong `Last-Modified`.
   - The ETag is preferred when both exist; nothing changes for ETag transfers.
2. **Byte-overlap proof.** Without an ETag, the resume request starts `min(offset, 64 KiB)` bytes before the
   confirmed offset, with `If-Range: <Last-Modified>`. Before anything is appended:
   - the `206` must carry the same `Last-Modified`, the exact `Content-Range`, the same length and identity
     encoding;
   - its first overlap bytes must equal the partial file's bytes before the offset.

   A difference ends the attempt as `REMOTE_CHANGED`, and nothing is appended. A `200` (date no longer matches) is
   the usual clean restart.
3. **Single connection only.**
   - Segmented downloads still require a strong ETag (plan unchanged).
   - Replacing a link (ADR 0020) still requires an ETag, and refuses a `Last-Modified` checkpoint with
     `validator_missing`.
4. **Durable checkpoint format 2.**
   - A single-connection checkpoint validated by `Last-Modified` is written as format version 2: `last_modified`
     instead of `etag`, so a service restart resumes it.
   - Version 2 is refused unless it is single mode, has no ETag, and has a valid HTTP date.
   - Version 1 checkpoints are read as before.
   - Older builds refuse version 2 ("unknown checkpoint format version"), which is a safe restart.

## Consequences

- **Wider resume.** Pause, network drop and service restart now resume for servers with only a strong
  `Last-Modified`, at the cost of re-downloading at most 64 KiB per resume.
- **Stronger than a lying ETag.** A changed file under an unchanged date is caught whenever the change reaches the
  last 64 KiB before the resume point (fixture R03). Changes only earlier in the file are not seen. As for any
  validator, only a trusted SHA-256 proves the whole file (COR-05).
- **Unchanged.** A weak `Last-Modified` (equal to `Date`), a missing length, or neither validator: resume stays
  explicitly unavailable.

## Evidence

All on Windows 11, 2026-11-17:
- `crates/backend-http/tests/validated_resume.rs`:
  - R01: resume byte-exact, with one resume request from offset − 64 KiB;
  - R02: a new date gives a clean restart onto the new file;
  - R03: changed bytes under the same date fail as `REMOTE_CHANGED`, with nothing appended;
  - R04: a weak date leaves resume unavailable.
- `durable.rs`: `a_last_modified_checkpoint_round_trips_as_version_2`.
- R01 fails without the change.
