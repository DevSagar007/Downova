# ADR 0038 — v1.0 owner decisions: license, private windows, objc2 notices

- **Status:** Accepted
- **Date:** 2026-11-16
- **Decided by:** the owner, choosing among the options in the two proposals and the PROGRESS decision table
- **Supersedes:** the open license item of ADR 0001 (Assumptions)
- **Amends:** ADR 0019 §8 (BRW-10 scope)
- **Requirements:** REL-03, BRW-10
- **Proposals:** [project license](proposals/project-license.md), [BRW-10 private capture](proposals/brw-10-private-capture-scope.md)

## Context

The v1.0 scope freeze (roadmap, 2026-11-16) left release work only. Three owner decisions blocked it: the project
license (REL-03), whether private-window capture must exist before the B2 exit (BRW-10), and how to ship the 13
objc2-family crates whose upstream revisions carry a licensing statement but no license text (REL-03).

## Decision

1. **License: `MIT OR Apache-2.0`** (proposal option A).
   - `LICENSE-MIT` and `LICENSE-APACHE` are in the repository root. The copyright line is "The Downova
     contributors".
   - `license = "MIT OR Apache-2.0"` is set in the workspace `Cargo.toml` (every crate inherits it) and in every
     `package.json`.
   - Contributions are accepted under the same terms (inbound = outbound). There is no CLA or DCO.
   - aria2 stays unbundled (ADR 0005). Bundling it later would need its own decision.
2. **BRW-10: option A.** Capture never acts on private windows.
   - The extension declares `"incognito": "not_allowed"`, ignores private download items, and says so on its options
     page in English and Bangla.
   - Offering private capture needs a separate ADR covering extension storage, service history, idempotency records
     and recovery. It is not part of v1.0.
3. **objc2 notices: option 1.** For each of the 13 crates, the notices ship:
   - the upstream licensing statement, pinned to the published revision as before;
   - the standard texts of the licenses that statement declares (MIT, and Zlib, Apache-2.0 and MIT where it is
     tri-licensed), from `third_party/notices/spdx/`.

   No copyright line is invented: upstream has none at those revisions. The overrides record this
   (`declared_texts`), and the notice is no longer unresolved.

## Consequences

- **REL-03:**
  - the license is decided;
  - the unresolved notices drop from 14 to 1 (the WiX `-ext` set, still inferred from the Tauri CLI binary);
  - REL-03 stays `in-progress` until that item is confirmed.
- **BRW-10:** the acceptance is reworded to the shipped behaviour and becomes `implemented`. It becomes `verified`
  once a real-browser run shows a private download staying in the browser.
- **Distribution:** with a license chosen, packages and store listings can be published once signing and accounts
  exist (PROGRESS owner decisions).
- **Not legal advice:** legal review of the dependency set remains the owner's step.
