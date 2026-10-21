# Project workflow and documentation

Applies to every task in this repository.

**Before changing code**
1. Read `CLAUDE.md`, `PROGRESS.md`, the rules that apply to the files you will touch, and the matching skill (`dm-engine`, `dm-ui`, `dm-browser`, `dm-verify`).
2. Name the requirement IDs (`requirements.md`) and their acceptance criteria. If none fit, add or adjust a requirement first.
3. Read the existing implementation and `docs/project-structure.md` to choose the owning module.

**While implementing**
- Do one coherent increment. Reuse existing components, tokens and contracts. Leave unrelated behaviour alone.
- Never delete, overwrite or reformat user files you did not create in this task. Check `git status` first and preserve uncommitted work.
- `docs/research/` is an unmodified copy of the source research. Never edit it.

**Documentation is part of the change**
- Requirement status changes go in `requirements.md` and `docs/traceability.md`, in the same change.
- Architecture or boundary changes need a new ADR in `docs/decisions/` plus updates to every affected document.
- Design changes go in `design.md` and `tokens.css` together.
- Statuses are `documented` → `in-progress` → `implemented` → `verified`. Use `verified` only with recorded evidence.

**Finishing**
Follow `.claude/rules/verification.md`. Then update `PROGRESS.md`: what changed, the commands run and their results, blockers, and the exact next task.
