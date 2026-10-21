# ADR 0001 — Project foundation

- **Status:** Accepted
- **Date:** 2026-10-21
- **Sources:** research §1, §4, §5, §14–§19; foundation brief

## Context

The project needs a free, account-free download manager for Windows, macOS and Linux, plus browser
extensions. It must be correct on unreliable networks, honest about what it supports, and respectful of privacy. The
research recommends reusing an established engine first and keeping the UI separate from the
transfer process.

## Decisions

1. **The desktop UI is React 19 + TypeScript + Vite**, running in a **Tauri 2** shell. Reason: the team already knows these tools; the system webview avoids bundling Chromium; and Tauri provides window, tray, packaging and a capability-scoped command surface. RAM and size benefits must still be measured (REL-04).
2. **The background coordinator is a separate per-user Rust process (`dm-service`).** It owns jobs, policy and SQLite. UI and browser lifetimes are independent of it (ARCH-01).
3. **The first backend is aria2, run as a sidecar behind a project-owned `DownloadBackend` trait.** Job and attempt IDs belong to the service. GIDs stay inside the adapter. Capabilities are reported explicitly (ARCH-03/04). Adopting aria2 is **conditional** on the B0 correctness fixtures. That go/no-go becomes ADR 0002.
4. **SQLite holds metadata, with a single writer (the service).** The engine owns segment and resume state (ARCH-05).
5. **Control IPC uses JSON-RPC 2.0 over a user-only named pipe or Unix socket.** Browser integration uses a TypeScript MV3 WebExtension plus a small Rust native-messaging host. Chrome and Edge come first, then Firefox.
6. **Contracts have one canonical JSON Schema** (`packages/contracts`). TypeScript types are generated from it. Rust uses hand-written serde mirrors checked by round-tripping shared examples. JSON Schema was chosen over Rust-first generation (ts-rs/specta/schemars) because it is language-neutral, readable by extension and CLI authors, and verifiable with Node alone. The cost is a hand-maintained Rust mirror. A test guards this.
7. **Tooling:** a pnpm workspace plus a Cargo workspace, with exact version pins and committed lockfiles. `scripts/verify.mjs` is the single verification entry point. It reports unavailable checks instead of skipping them silently.
8. **Design:** semantic CSS tokens with system light/dark themes. Components consume only tokens (see design.md).

## Versions selected (checked against npm and crates.io registries on 2026-10-21)

| Tool / library | Version | Note |
|---|---|---|
| Node.js (local) | 26.7.0 | engines `>=22.12` |
| pnpm | 12.14.0 | `packageManager` field; corepack absent in Node 26, use `npm i -g pnpm@12.14.0` |
| TypeScript | 6.0.3 | 7.0.2 is latest, but typescript-eslint 8.72 supports `<6.1` |
| ESLint | 9.39.5 | 10.x is latest, but eslint-plugin-jsx-a11y 6.10.2 supports ≤9 |
| typescript-eslint | 8.72.0 | |
| React / React DOM | 19.3.1 | |
| Vite / @vitejs/plugin-react | 8.3.4 / 6.1.2 | |
| Vitest / jsdom | 5.0.5 / 30.1.3 | |
| @tauri-apps/api / cli | 2.12.4 / 2.12.3 | |
| lucide-react | 1.58.0 | 1.61.0 rejected by pnpm's minimum release age policy |
| Prettier | 3.9.10 | |
| json-schema-to-typescript / ajv | 16.0.0 / 8.20.0 | |
| Rust toolchain | 1.99.1 (pinned) | edition 2024, MSRV 1.85 |
| tauri / tauri-build | 2.12.4 / 2.7.3 | |
| serde / serde_json | 1.0.229 / 1.0.154 | |

## Alternatives considered

- **Electron + React + aria2.** This keeps JavaScript throughout, but brings Chromium runtime overhead. Rejected because of footprint goals; it remains an option if Rust becomes a blocker.
- **Forking AB Download Manager (Kotlin/Compose) or Gopeed (Go/Flutter).** This is the fastest path to a full app, but means taking on a foreign stack and upstream maintenance. Feasibility comparison stays in B0 (§15.8).
- **A custom Rust engine from day one.** This gives full control, but is the most correctness work. Deferred until fixtures prove aria2 is insufficient (§15.2).
- **UI talking to aria2 RPC directly.** Rejected because it leaks engine secrets and identity into the UI and breaks backend replaceability.

## Assumptions (reversible defaults)

- The product name "Download Manager" and the bundle identifier `dev.downloadmanager.desktop` are **placeholders**. Changing the identifier before the first release is cheap. After release it moves user data paths.
- The project license is **not chosen**. Cargo and npm manifests omit license fields until the dependency audit (REL-03).
- UI preferences (language, theme) live in webview localStorage until the settings API exists (B2).
- Code style: Prettier with a width of 100; rustfmt with a width of 100; Markdown is excluded from Prettier to protect Bangla tables.

## Consequences

- Three processes plus a sidecar need lifecycle tests on every OS.
- Each schema change needs an update to the Rust mirror (guarded by tests).
- Rust work is blocked on machines without a Rust toolchain. `pnpm verify` reports this explicitly.
