# PROGRESS — handoff

_Last updated: 2026-10-21 (Windows 11, Node 26.7.0, pnpm 12.14.0)_

## Current milestone
**Foundation: done** for everything that can be checked in this environment. Next: **B0 — architecture gate** ([docs/roadmap.md](docs/roadmap.md)).

## Completed
- Research is preserved verbatim at `docs/research/download-manager-rnd-bn.md` (SHA-256 `ae56b81e…da641d`, identical to the original).
- Documentation:
  - top level: CLAUDE.md, README.md, requirements.md (91 requirements: ARCH, CORE, COR, BRW, UI, SEC, REL, MOB, F01–F16, E01–E06, ADV01–ADV12), design.md;
  - `docs/`: architecture, project-structure, roadmap, traceability, testing, setup, and ADR 0001.
- Claude Code: `.claude/settings.json`, `.claude/launch.json`, and 6 rules (3 path-scoped). There are 4 skills: dm-engine, dm-ui, dm-browser and dm-verify. Claude Code discovered all four.
- Tooling: pnpm workspace and lockfile, Cargo workspace (no lockfile yet), TypeScript, ESLint (including the boundary rule), Prettier, rustfmt, clippy, `.editorconfig`, `.gitignore`, `.gitattributes`.
- Contracts: the canonical JSON Schema, generated TypeScript, and 5 valid and 2 invalid examples.
- Desktop UI, restyled to the classic IDM-style layout at the user's request (2026-10-21; layout and workflow only, no IDM artwork or name):
  - menu bar: Tasks, File, Downloads, View, Help;
  - big-icon toolbar: commands that need the service are faded and keep a stated reason;
  - categories tree, a columned download table and a status bar;
  - Options and About dialogs.
  - New primitives: MenuBar, ToolbarButton, Dialog. Classic chrome tokens were added, and design.md §1, §3–§5 and §8 were rewritten.
  - Values shown come only from the service. Rate, ETA and last try show "—" because the v1 contract does not provide them.
  - Audit against IDM-style surfaces (design.md §13). Added a close button on the categories panel, column sorting (name, size, status), file-type row icons, header tooltips, and the Speed limiter and Check for updates menu entries (unavailable, with reasons).
  - 16 vitest tests.
- Tauri shell: config, a minimal capability, CSP, and icons generated from `app-icon.svg`.
- Rust: `dm-domain` (IDs, ByteCount, state policy, ErrorKind, capability, redaction/Secret, API types, contract round-trip test) and `dm-service` (`DownloadBackend` trait, `require_capability`, `hello_result`).
- Scripts: `verify.mjs` (single entry point, reports UNAVAILABLE honestly), `check-structure.mjs`, and `gen-contracts.mjs`.

## Checks actually run (Windows 11, 2026-10-21)
| Command | Result |
|---|---|
| `pnpm verify` → structure | PASSED (119 files, 91 requirements traced, 24 Markdown files link-checked) |
| → contracts (`gen-contracts --check`) | PASSED (types current; 5 valid and 2 invalid examples behave as expected) |
| → prettier `--check` | PASSED |
| → eslint `--max-warnings=0` | PASSED |
| → tsc contracts and desktop | PASSED |
| → vitest desktop | PASSED (16 tests: menu-bar keyboard, toolbar arrows, categories plus close and re-show, Options dialog focus and Esc, theme, Bangla, table formatting, sorting, file-type icons, pause vs policy status) |
| → vite build | PASSED |
| → cargo fmt, clippy, test | **UNAVAILABLE** (no Rust toolchain) |
| Negative checks: broken link, wrong skill name, forbidden crate dependency, Tauri import in a feature, missing traceability row | All 5 detected by `check-structure`; ESLint also blocks the Tauri import. All test changes reverted |
| Browser preview (`pnpm dev`, after restyle) | English and Bangla, light and dark, Options dialog, and 540px width checked visually. Fixed: page overflow at narrow width, empty message clipped, empty-row hover |

## Blockers and open decisions
- **No Rust toolchain or MSVC Build Tools on this machine.** No Rust code (domain, service, Tauri shell) has been compiled or tested, so the related requirements stay `in-progress`. To fix:
  1. install rustup and the "Desktop development with C++" Build Tools;
  2. run `cargo generate-lockfile`;
  3. run `pnpm verify`.
- No macOS or Linux machine available. Nothing is verified on those OSes.
- aria2 binaries are not yet chosen or pinned (B0.1).
- Open decisions (ADR 0001): project license, product name, and bundle identifier (`dev.downloadmanager.desktop` is a placeholder).
- pnpm is not installed globally. Use `npx pnpm@12.14.0` or `npm i -g pnpm@12.14.0`.

## Platform limitations
- All results above are Windows-only. `src-tauri` has not been built anywhere.

## Exact next task
1. **Unblock Rust:** install the toolchain, then run `cargo generate-lockfile` and `pnpm verify`. Fix any Rust compile, lint or test issues in `crates/domain`, `crates/service` and `apps/desktop/src-tauri`, then update the evidence in traceability.
2. **B0.1 — `crates/backend-aria2` (skill `dm-engine`):**
   - spawn a pinned aria2c on loopback with a random RPC secret;
   - map attempt IDs to GIDs inside the adapter;
   - implement `DownloadBackend` and a complete capability report (`unverified` until fixtures pass);
   - verify with a synthetic-secret test that the session and config files contain no secrets.
   - Requirements: ARCH-03, ARCH-04, REL-05, SEC-01.
