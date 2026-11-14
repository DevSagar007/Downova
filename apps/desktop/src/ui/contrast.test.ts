// B2.14: WCAG 2.2 AA contrast of the desktop colour tokens the affected UI uses, in light and dark
// (the extension checks its own colours in apps/extension/src/contrast.test.ts).
// Text needs 4.5:1; focus indicators and control borders that identify a control need 3:1.
// Automated arithmetic only: it does not replace a human check with real screen content.
import tokensCss from "./tokens.css?raw";
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

/** F16: the high-contrast token set, layered over the light tokens like the CSS cascade. */
function highContrast(css: string, light: Record<string, string>) {
  const marker = ':root[data-theme="high-contrast"] {';
  const at = css.indexOf(marker);
  if (at < 0) throw new Error("no high-contrast token set");
  const block = css.slice(at, css.indexOf("\n}\n", at) + 3);
  return { ...light, ...tokens(block) };
}

// WCAG AAA text (7:1); focus, borders and grid lines that identify controls 4.5:1. Disabled text is
// exempt in WCAG but held to 4.5:1 here: people who need high contrast still need to read it.
const AAA_TEXT = 7;
const STRONG_NON_TEXT = 4.5;

describe("desktop tokens (high contrast, F16)", () => {
  const t = highContrast(tokensCss, desktop.light);
  const pairs: [string, string, number][] = [
    ["text", "surface", AAA_TEXT],
    ["text", "bg", AAA_TEXT],
    ["text", "surface-hover", AAA_TEXT],
    ["text", "chrome", AAA_TEXT],
    ["text", "table-header", AAA_TEXT],
    ["text-muted", "surface", AAA_TEXT],
    ["text-muted", "surface-sunken", AAA_TEXT],
    ["text-disabled", "surface", STRONG_NON_TEXT],
    ["status-warning", "status-warning-bg", AAA_TEXT],
    ["status-danger", "status-danger-bg", AAA_TEXT],
    ["status-success", "status-success-bg", AAA_TEXT],
    ["status-info", "status-info-bg", AAA_TEXT],
    ["status-neutral", "status-neutral-bg", AAA_TEXT],
    ["accent-text", "accent", AAA_TEXT],
    ["row-selected-text", "row-selected", AAA_TEXT],
    ["focus", "surface", STRONG_NON_TEXT],
    ["focus", "bg", STRONG_NON_TEXT],
    ["border", "surface", STRONG_NON_TEXT],
    ["border-strong", "surface", STRONG_NON_TEXT],
    ["chrome-border", "chrome", STRONG_NON_TEXT],
    ["grid", "surface", STRONG_NON_TEXT],
    ["accent", "surface", STRONG_NON_TEXT],
    ["progress-track", "surface", 3],
    ["accent", "progress-track", 3],
    ["icon-add", "chrome", STRONG_NON_TEXT],
    ["icon-resume", "chrome", STRONG_NON_TEXT],
    ["icon-stop", "chrome", STRONG_NON_TEXT],
    ["icon-delete", "chrome", STRONG_NON_TEXT],
    ["icon-options", "chrome", STRONG_NON_TEXT],
    ["icon-schedule", "chrome", STRONG_NON_TEXT],
    ["icon-queue", "chrome", STRONG_NON_TEXT],
  ];
  it.each(pairs)("--color-%s on --color-%s", (fg, bg, min) => {
    expect(ratio(color(t, `color-${fg}`), color(t, `color-${bg}`))).toBeGreaterThanOrEqual(min);
  });

  it("overrides every colour token of the light set (nothing leaks from light)", () => {
    const own = tokens(
      tokensCss.slice(tokensCss.indexOf(':root[data-theme="high-contrast"] {')).split("\n}\n")[0] ??
        "",
    );
    const lightColours = Object.keys(desktop.light).filter(
      (k) => k.startsWith("color-") && k !== "color-progress-fill",
    );
    expect(lightColours.filter((k) => !(k in own))).toEqual([]);
  });

  it("is a token set only: no selectors other than the theme root, no url() or expressions", () => {
    const start = tokensCss.indexOf(':root[data-theme="high-contrast"] {');
    const block = tokensCss.slice(start, tokensCss.indexOf("\n}\n", start) + 3);
    const declarations = block
      .slice(block.indexOf("{") + 1, block.lastIndexOf("}"))
      .split(";")
      .map((d) => d.trim())
      .filter(Boolean);
    for (const d of declarations) {
      expect(d).toMatch(/^(--[\w-]+|color-scheme)\s*:/);
      expect(d).not.toMatch(/url\(|expression\(|@import/i);
    }
  });
});

it("computes known reference ratios", () => {
  expect(ratio("#000000", "#ffffff")).toBeCloseTo(21, 0);
  expect(ratio("#777777", "#ffffff")).toBeCloseTo(4.48, 1);
});
