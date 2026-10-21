---
paths:
  - "crates/**/*.rs"
  - "crates/**/Cargo.toml"
  - "apps/desktop/src-tauri/**/*.rs"
  - "Cargo.toml"
---

# Rust service and backend development

Use the `dm-engine` skill for service, backend, storage and recovery work.

- Edition 2024 and the pinned toolchain (`rust-toolchain.toml`). Inherit workspace lints (`[lints] workspace = true`). No `unsafe`.
- `dm-domain` stays pure: no I/O, no async runtime, no Tauri, SQLite or aria2.
- Byte offsets and counts are `u64` or `ByteCount`. Use checked arithmetic (`clippy::arithmetic_side_effects` is on). HTTP ranges are half-open internally and inclusive only in headers.
- Wire types must match `packages/contracts/schema/service-api.v1.schema.json`. Add an example and keep `tests/contract_examples.rs` passing.
- Wrap secrets in `dm_domain::redact::Secret`. Log URLs only through `redact_url`. Never derive `Serialize` on anything holding a secret.
- Classify errors into `ErrorKind` at the boundary where they occur. Do not leak raw engine messages containing URLs.
- No `unwrap` or `expect` in non-test code except where an invariant is documented. Return typed errors.
- Every behaviour change gets a unit or integration test that would fail without it. Protocol behaviour gets a fixture under `tests/fixtures/`.
- Run `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`. If cargo is unavailable, say so and do not claim the code compiles.
