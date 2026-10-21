---
name: dm-verify
description: Use before reporting any task as complete, when choosing which checks prove a requirement, when updating requirement status or traceability evidence, or when recording platform limitations and blockers in PROGRESS.md.
---

# dm-verify: meaningful checks and honest reporting

## 1. Trigger and scope
Use this skill at the end of every increment, and whenever a requirement status changes, a platform-support statement is made, or a check fails or is unavailable.

## 2. Consult first
- `docs/testing.md` (strategy per area, entry points).
- `docs/traceability.md` (evidence rules).
- `requirements.md` (the acceptance criteria being claimed).
- `PROGRESS.md` (known blockers and environment).
- `.claude/rules/verification.md`.

## 3. Inputs and boundaries
- **Inputs:** the requirement IDs touched, the files changed, and the environment (OS, available tools: node, pnpm, cargo, aria2, browsers).
- **Boundaries:**
  - never mark a check passed that did not run;
  - never weaken or skip a check to get green;
  - never generalise OS or browser results.

## 4. Workflow
1. Map each acceptance criterion touched to a concrete check. If no check can fail for the behaviour, write one or record the gap.
2. Run `pnpm verify` (or `--only=` groups) plus the targeted tests. Keep the summary output.
3. For any failure: decide whether this change caused it. Fix it if so; otherwise record it as pre-existing, with evidence.
4. For any UNAVAILABLE check: record the missing tool or OS as a blocker.
5. Update the statuses:
   - `implemented` requires the criteria met by the code;
   - `verified` requires the evidence recorded (check name, date, OS).
6. Update `docs/traceability.md` evidence cells and `PROGRESS.md` (checks run, results, blockers, exact next task).

## 5. Verification of the verification
- `pnpm check:structure` passes: traceability is consistent, frontmatter is valid, links resolve.
- The evidence cells name real test files or commands that exist.

## 6. Documentation and progress
`PROGRESS.md` is the handoff. It must let a new session continue without this conversation.

## 7. Completion evidence (report format)
1. What changed (files and modules).
2. Checks: `command → PASSED/FAILED/UNAVAILABLE (OS)`.
3. Requirements whose status changed, and why.
4. Unverified code, platform limitations and blockers.
5. The exact next task.
