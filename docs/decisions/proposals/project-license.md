# Proposal: Downova's project license

- **Status:** **Proposed: awaiting the owner's choice.** Nothing here adds a `LICENSE` file or a `license` field to
  any `Cargo.toml` or `package.json`. ADR 0001 keeps the decision open until the owner chooses. Then an ADR records
  it, and the files are added in one change.
- **Date:** 2026-11-08
- **Requirement:** REL-03 ("the project license is decided after a dependency audit")
- **Evidence:** the dependency audit in the [beta-candidate readiness report](../../reports/beta-candidate-readiness.md)
  §4, and the generated inventory `apps/desktop/src/generated/third-party.notices.json`.
- **This is not legal advice.** It is a technical summary of the obligations in the shipped dependency set. Legal
  clearance is a separate step for the owner.

## What the shipped dependencies require, whatever Downova chooses

- **Permissive licenses** (MIT, Apache-2.0, BSD-3-Clause, ISC, Zlib, 0BSD, Unicode-3.0, Unlicense, CC0-1.0) cover
  most components. They require their copyright and license texts to ship. `THIRD-PARTY-NOTICES.txt` and Help →
  Third-party notices… do that.
- **MPL-2.0** (`selectors` and three other crates): file-level copyleft. Downova may use any license, but the
  source of those files, as shipped, must stay available. The notices record the exact upstream revision for this.
- **MS-RL** (WiX resources in the MSI): reciprocal only for the WiX files themselves.
- **NSIS** (zlib/libpng; bzip2 and CPL-1.0 for its other compression modules): notice only.
- **aria2** (GPL-2.0-or-later) is **not bundled** (ADR 0005). If it is bundled later, its corresponding source must
  be offered, and the project license must allow that combination in the shipped package.
- **No distributed component forces a copyleft license on Downova's own code.**

## Candidates

| Option | SPDX | What it means for Downova | Tradeoffs |
|---|---|---|---|
| **A. Dual MIT or Apache-2.0** | `MIT OR Apache-2.0` | Anyone may use, change and redistribute, including in closed products. Contributors grant a patent licence under Apache-2.0 | The Rust ecosystem's norm (most dependencies use it); simple. Others may ship closed forks |
| **B. Apache-2.0 only** | `Apache-2.0` | As A, with explicit patent terms and a NOTICE convention | One text. Not GPL-2.0-only compatible (irrelevant unless such code is added) |
| **C. GPL-3.0-or-later** | `GPL-3.0-or-later` | Modified versions that are distributed must stay open source | Keeps forks open; compatible with bundling aria2 (GPL-2.0-or-later) later. Closed commercial reuse is excluded; some stores and companies avoid GPL apps |
| **D. MPL-2.0** | `MPL-2.0` | Changes to Downova's own files stay open; combining with other code is free | A middle ground: file-level copyleft, compatible with A-licensed dependencies |

Authoritative texts: https://spdx.org/licenses/MIT.html, https://spdx.org/licenses/Apache-2.0.html,
https://spdx.org/licenses/GPL-3.0-or-later.html, https://spdx.org/licenses/MPL-2.0.html.

## What the owner needs to decide

1. **One of A–D**, or another license, and whether contributions need a DCO or CLA.
2. **Whether aria2 may ever be bundled.** That decision favours C if bundling is wanted.

## After the choice (one change)

- Add `LICENSE` (or `LICENSE-MIT` and `LICENSE-APACHE` for A) with the exact SPDX text.
- Set `license` in the workspace `Cargo.toml` and in each `package.json`.
- Record the choice in an ADR that supersedes ADR 0001's open item, and update REL-03.
- The inventory's remaining unresolved items (readiness report §4) stay a separate REL-03 gate, whatever license is
  chosen.
