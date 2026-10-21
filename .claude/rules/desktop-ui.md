---
paths:
  - "apps/desktop/src/**"
  - "apps/desktop/index.html"
  - "design.md"
---

# Desktop UI and design consistency

Use the `dm-ui` skill. `design.md` is the canonical design system.

- Use only tokens from `src/ui/tokens.css`. If a needed value is missing, add a semantic token there and document it in `design.md`. Never use raw hex, px spacing or ad-hoc radii in components.
- Reuse primitives in `src/ui/` (Button, Banner, EmptyState, …) before creating new ones. A new shared primitive goes in `src/ui/` with its own CSS file and a row in design.md §4.
- Feature code lives in `src/features/<feature>/`. Only `src/services/` may import `@tauri-apps/*` (ESLint enforces this). Features use the `ServiceClient` interface and `@dm/contracts` types.
- Show service truth only: no invented progress, sample data or "connected" states. Unavailable actions use `unavailableReason`.
- Every string goes in `src/i18n/messages.ts` for both `en` and `bn`.
- Accessibility: semantic elements, labelled controls, a visible focus ring, keyboard operation, and colour never the only status cue. Use logical CSS properties for future RTL.
- Tests: Testing Library queries by role and name. Test behaviour (keyboard, states, language), not markup.
- Run `pnpm verify` (at least lint, typecheck, test and build) before reporting.
