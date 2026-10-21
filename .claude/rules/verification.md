# Verification and completion reporting

Use the `dm-verify` skill. The strategy is in `docs/testing.md`.

- Run `pnpm verify`, or the relevant `--only=` groups plus targeted tests, before saying a task is done. Fix failures your change caused. Do not weaken, skip or delete checks to make them pass.
- Report each check exactly: command, result (PASSED, FAILED or UNAVAILABLE) and OS. "UNAVAILABLE" or "not run" is never "passed".
- Code that was written but not compiled or executed is reported as unverified.
- Platform support is claimed only for OSes where the check actually ran. Windows results say nothing about macOS or Linux.
- A requirement becomes `verified` only when `docs/traceability.md` names the passing check and the run.
- Update `PROGRESS.md` with the checks run, the results, blockers and the exact next task.
- Final report: what changed, what was verified, what remains unverified or blocked, and what comes next.
