// B2.14: WCAG 2.2 AA contrast of the colour tokens the affected UI uses, in light and dark, for
// the desktop (tokens.css) and the browser extension (popup.css, shared by its options page).
// Text needs 4.5:1; focus indicators and control borders that identify a control need 3:1.
// Automated arithmetic only: it does not replace a human check with real screen content.
import tokensCss from "./tokens.css?raw";
import popupCss from "../../../extension/src/popup.css?raw";
import { describe, expect, it } from "vitest";

function luminance(hex: string): number {
  const h = hex.replace("#", "");
  const full = h.length === 3 ? [...h].map((c) => c + c).join("") : h;
  const lin = (c: number) => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
  const channel = (i: number) => lin(parseInt(full.slice(i, i + 2), 16) / 255);
  return 0.2126 * channel(0) + 0.7152 * channel(2) + 0.0722 * channel(4);
}

export function ratio(a: string, b: string): number {
  const la = luminance(a);
  const lb = luminance(b);
  return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05);
}

/** `--name: #hex` declarations of a CSS fragment (later declarations win). */
function tokens(css: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [, name, value] of css.matchAll(/--([\w-]+):\s*(#[0-9a-fA-F]{3,8})\s*;/g)) {
    if (name && value) out[name] = value;
  }
  return out;
}

/** A token that must exist (a missing one fails the test by name). */
function color(set: Record<string, string>, name: string): string {
  const value = set[name];
  if (value === undefined) throw new Error(`missing token --${name}`);
  return value;
}

/** Light tokens, and dark tokens layered over them. */
function themes(css: string, darkMarker: string) {
  const at = css.indexOf(darkMarker);
  const light = tokens(css.slice(0, at));
  const darkBlock = css.slice(
    at,
    css.indexOf("\n}\n", css.indexOf("{", at + darkMarker.length) + 1) + 3,
  );
  return { light, dark: { ...light, ...tokens(darkBlock) } };
}

const desktop = themes(tokensCss, "@media (prefers-color-scheme: dark)");
const extension = themes(popupCss, "@media (prefers-color-scheme: dark)");

const TEXT = 4.5;
const NON_TEXT = 3;

describe.each(["light", "dark"] as const)("desktop tokens (%s)", (theme) => {
  const t = desktop[theme];
  const pairs: [string, string, number][] = [
    ["text", "surface", TEXT],
    ["text", "bg", TEXT],
    ["text-muted", "surface", TEXT],
    ["text-muted", "bg", TEXT],
    ["text-muted", "surface-sunken", TEXT],
    ["status-warning", "status-warning-bg", TEXT],
    ["status-danger", "status-danger-bg", TEXT],
    ["status-success", "status-success-bg", TEXT],
    ["status-info", "status-info-bg", TEXT],
    ["status-danger", "surface", TEXT],
    ["accent-text", "accent", TEXT],
    ["row-selected-text", "row-selected", TEXT],
    ["focus", "surface", NON_TEXT],
    ["focus", "bg", NON_TEXT],
    ["border-strong", "surface", 1.0],
  ];
  it.each(pairs)("--color-%s on --color-%s", (fg, bg, min) => {
    expect(ratio(color(t, `color-${fg}`), color(t, `color-${bg}`))).toBeGreaterThanOrEqual(min);
  });
});

describe.each(["light", "dark"] as const)("extension popup and options (%s)", (theme) => {
  const t = extension[theme];
  it.each([
    ["fg", TEXT],
    ["muted", TEXT],
    ["ok", TEXT],
    ["warn", TEXT],
    ["bad", TEXT],
    ["focus", NON_TEXT],
  ] as const)("--%s on --bg", (fg, min) => {
    expect(ratio(color(t, fg), color(t, "bg"))).toBeGreaterThanOrEqual(min);
  });
});

it("computes known reference ratios", () => {
  expect(ratio("#000000", "#ffffff")).toBeCloseTo(21, 0);
  expect(ratio("#777777", "#ffffff")).toBeCloseTo(4.48, 1);
});
