/**
 * What this build offers (ADR 0029), decided by the build target, never by guessing from which
 * APIs exist. The Firefox build ships explicit public hand-offs only: automatic capture relies on
 * request evidence (Chromium's `extraHeaders` view of cookies and credentials) that Firefox does
 * not give the same way, and signed-in downloads and reconnect are Chrome/Edge flows (BRW-05).
 * Neither is offered in Firefox until it is implemented and tested there.
 */
export const FEATURES = {
  capture: __DOWNOVA_TARGET__ === "chromium",
  signIn: __DOWNOVA_TARGET__ === "chromium",
} as const;

export const TARGET = __DOWNOVA_TARGET__;
