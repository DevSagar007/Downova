---
name: dm-ui
description: Use when building or changing the desktop React UI or Tauri shell in apps/desktop — views, components, design tokens, themes, i18n (English/Bangla), accessibility, the service-client boundary, or the visual design in design.md.
---

# dm-ui: desktop interface and design system

## 1. Trigger and scope
Use this skill for anything under `apps/desktop/src/`, `apps/desktop/src-tauri/` (shell behaviour, capabilities, tray, commands) and `design.md`. Service logic belongs to `dm-engine`. Extension popup visuals follow this skill together with `dm-browser`.

## 2. Consult first
- `design.md` (layout, tokens, components, states, resizing, accessibility).
- `apps/desktop/src/ui/tokens.css` and the existing primitives in `src/ui/`.
- `requirements.md`: UI-*, ADV12, ARCH-08/09, CORE-* for the feature, SEC-03/05.
- `.claude/rules/desktop-ui.md`.

## 3. Inputs and boundaries
- **Inputs:** requirement IDs, the contract types (`@dm/contracts`) for the data shown, and the states from design.md §6 that the view must handle.
- **Boundaries:**
  - only `src/services/` touches `@tauri-apps/*`;
  - features use the `ServiceClient` interface;
  - the UI presents service state and never computes or invents transfer state;
  - tokens only.

## 4. Workflow
1. Identify which design.md states and components apply. Reuse before creating. When a new primitive is truly needed, add it to `src/ui/` with a CSS file and a row in design.md §4.
2. Put strings in `src/i18n/messages.ts` for `en` and `bn`, kept short and status-first.
3. Build with semantic HTML: landmarks, headings, labels, `aria-current`, `aria-disabled` with a reason, and live regions only for meaningful changes.
4. Use logical CSS properties. Check the 760px and 560px breakpoints and long or Bangla filenames.
5. Data access: extend the `ServiceClient` interface and implementations in `src/services/`, and inject fakes in tests.
6. Tauri commands: register the minimal capability and validate every argument in Rust (SEC-05).

## 5. Verification
- `pnpm verify --only=lint,typecheck,test,build` (or a full `pnpm verify`).
- Testing Library tests for keyboard flow, accessible names, states and the language switch.
- Visual check at both breakpoints in light and dark themes (the `pnpm dev` browser preview). Manual screen-reader smoke testing before a beta.

## 6. Documentation and progress
Update `design.md` for any new token, component or state. Update traceability for UI requirements and `PROGRESS.md`.

## 7. Completion evidence
Components and tokens added or reused, tests added and their results, themes and breakpoints checked (how), and remaining accessibility gaps.
