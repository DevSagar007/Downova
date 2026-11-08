# Beta-candidate readiness: Linux reconnect CI, normal kits, native installer lifecycle

- **Date:** 2026-11-08. **Local host:** Windows 11 Pro 10.0.26300 x64 (a development machine with real Downova data;
  **not** an isolated environment).
- **Branch:** `fix/beta-candidate-readiness`, from `fe8b8bd`. That is `origin/main` `69a9068` plus the two B3.7
  commits of `fix/b3.7-linux-reconnect-ci` (the S4 fix).
- **Not authorized and not done:** no merge, no push to `main`, no release, no production key.
- **Release gates unchanged:** B2 is not exited, and the BRW-10 private-window decision is pending.

## 1. Linux S4 (the remaining CI failure on `69a9068`)

**Confirmed cause: runner setup.** The ubuntu-24.04 real-browser step ran the service without an OS credential
store. Evidence and the fix are in the [B3.7 report](b3.7-platform-ci-stabilization.md):
- **Store off:** the local Windows reproduction with the store off gives the CI failure exactly: checkpoint
  `sealed: 0`, `AUTH_REQUIRED`, no candidates.
- **Real store:** the real Chrome journey passes, S4 included. The sealed checkpoint was 2,686,976 bytes, nothing
  was requested before the explicit reconnect, and the first range was `bytes=2686976-…` with only the fresh
  cookie. SHA-256 exact, no leaks.

**The fix:**
- a verified disposable Secret Service in the CI step's own D-Bus session;
- S4 with a fixture gate, and a proven sealed checkpoint before the restart.

The persistent data key and the temporary cookie keep their different lifetimes. No test vault is used, and
nothing is persisted in plaintext.

## 2. What this batch adds

- **`scripts/acceptance/installer-lifecycle.mjs`:** the real installer lifecycle, with a hard isolation guard (a
  GitHub-hosted runner, or `DOWNOVA_DISPOSABLE_MACHINE=1`; refuses where Downova exists). Rows:

  | Package | Rows |
  |---|---|
  | NSIS | N1–N6 |
  | MSI | M1–M4 |
  | `.deb` | D1–D4 |
  | `.app` | A1–A3 |

  The rows cover:
  - install, then work (running, manually paused, stopped queue);
  - upgrade while the service runs, then the work continues (SHA-256);
  - uninstall with no owned process left, data and downloads kept, only Downova's registration removed (an
    unrelated registration must survive);
  - reinstall with the retained list.

  Processes are identified by installation path, never by name.
- **`.github/workflows/installer-lifecycle.yml`:** manual, three runners. Each builds the fixture pair, runs the
  harness, uploads `results.json`, and publishes every row as an annotation.
- **Docs:** testing and setup.

## 3. Results

| Evidence | Source | Status |
|---|---|---|
| Unit and integration (local verify, Windows) | `00ce723` tree | 24/24 passed (`cargo test` 613, 0 failed) |
| Real Chrome journey with S4 (local, Windows, real store) | the `3ae2732` tree | passed; the store-off control fails explicitly |
| Seven CI checks for the S4 fix | `e117e2f` or later on this branch | **NOT RUN:** the workflows must be dispatched (no API access from this session) |
| Smoke-feature packages | `69a9068` | 3/3 passed ([run 42455584846](https://github.com/DevSagar007/Downova/actions/runs/42455584846)) |
| Normal packages and kits, three targets (Acceptance kit workflow) | this branch | **NOT RUN:** dispatch needed. Windows only, locally from `6312d2e` (B3.6): kit `f882be31…` validated |
| Installed lifecycle (Installer lifecycle workflow) | this branch | **NOT RUN:** dispatch needed. The harness's isolation guard was checked locally (it refuses here) |
| Human acceptance (context menu, screen reader, Bangla review, tray, sleep/logout/reboot) | — | NOT RUN |

## 4. Remaining work

- **Dispatch needed** (Actions → Run workflow, branch `fix/beta-candidate-readiness`): **CI**, **Package smoke**,
  **Acceptance kit**, **Installer lifecycle**. Opening a draft PR also starts CI.
- **Owner decisions:**
  - the project license and the REL-03 notice gaps;
  - the NSIS install-folder layout (ADR 0031 §4);
  - BRW-10 private windows;
  - production update configuration.
- **Human checks:** the B2 installed pass and the B3 rows.
- **Next batch:** the IDM comparison and the remaining release configuration.
