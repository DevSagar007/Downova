# @dm/contracts

Canonical service API contract shared by the service, CLI, browser host, desktop UI and extension.

| Path | Role |
|---|---|
| `schema/service-api.v1.schema.json` | **Single source of truth** (JSON Schema 2020-12). Edit this first. |
| `src/generated/service-api.v1.ts` | Generated TypeScript types. Do not edit; run `pnpm contracts:gen`. |
| `examples/*.json` | Valid examples (`{"$contract": "<Def>", "value": ...}`) checked by both TypeScript (Ajv) and Rust (`crates/domain/tests/contract_examples.rs`). |
| `examples/invalid/*.json` | Examples that **must** fail schema validation. |

Sync rules (see `docs/architecture.md#contracts`):

1. Change the schema, then run `pnpm contracts:gen`.
2. Update the Rust mirror in `crates/domain/src/api.rs` and add or adjust an example.
3. `pnpm contracts:check` fails if generated TypeScript is stale or an example disagrees with the schema; `cargo test -p dm-domain` fails if Rust cannot round-trip the examples.
4. Breaking wire changes require a new method version (`downloads.v2.*`) and an ADR.
