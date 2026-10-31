// B2.14: WCAG 2.2 AA contrast of the extension's colours (popup.css, shared by the options page) in
// light and dark: text 4.5:1, the focus ring 3:1. Automated arithmetic only; a human check with
// real screen content is separate evidence.
import popupCss from "./popup.css?raw";
import { describe, expect, it } from "vitest";

function luminance(hex: string): number {
  const h = hex.replace("#", "");
  const full = h.length === 3 ? [...h].map((c) => c + c).join("") : h;
  const lin = (c: number) => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
  const channel = (i: number) => lin(parseInt(full.slice(i, i + 2), 16) / 255);
  return 0.2126 * channel(0) + 0.7152 * channel(2) + 0.0722 * channel(4);
}

function ratio(a: string, b: string): number {
  const la = luminance(a);
  const lb = luminance(b);
  return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05);
}

function tokens(css: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [, name, value] of css.matchAll(/--([\w-]+):\s*(#[0-9a-fA-F]{3,8})\s*;/g)) {
    if (name && value) out[name] = value;
  }
  return out;
}

const marker = "@media (prefers-color-scheme: dark)";
const at = popupCss.indexOf(marker);
const light = tokens(popupCss.slice(0, at));
const dark = {
  ...light,
  ...tokens(
    popupCss.slice(
      at,
      popupCss.indexOf("\n}\n", popupCss.indexOf("{", at + marker.length) + 1) + 3,
    ),
  ),
};

function color(set: Record<string, string>, name: string): string {
  const value = set[name];
  if (value === undefined) throw new Error(`missing --${name}`);
  return value;
}

describe.each([
  ["light", light],
  ["dark", dark],
] as const)("popup and options colours (%s)", (_, t) => {
  it.each([
    ["fg", 4.5],
    ["muted", 4.5],
    ["ok", 4.5],
    ["warn", 4.5],
    ["bad", 4.5],
    ["focus", 3],
  ] as const)("--%s on --bg", (fg, min) => {
    expect(ratio(color(t, fg), color(t, "bg"))).toBeGreaterThanOrEqual(min);
  });
});

it("reads distinct dark colours", () => {
  expect(dark.bg).not.toBe(light.bg);
});
