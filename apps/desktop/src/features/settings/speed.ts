// Bandwidth-limit values for the UI (CORE-06, ADR 0024). The contract carries bytes per second as
// a canonical decimal string (ByteCount); the UI shows and accepts decimal KB/s and MB/s (×1000),
// the same convention as the CLI. Conversion is exact integer arithmetic: never a float.
import type { BackendInfo } from "@dm/contracts";

export type RateUnit = "kb" | "mb";
export const RATE_UNITS: readonly RateUnit[] = ["kb", "mb"];
const UNIT_BYTES: Record<RateUnit, bigint> = { kb: 1000n, mb: 1_000_000n };
/** The contract's bounds (`MIN_SPEED_LIMIT`, `MAX_SPEED_LIMIT`). */
export const MIN_BYTES_PER_SECOND = 1024n;
export const MAX_BYTES_PER_SECOND = 1_000_000_000_000n;

const BANGLA_DIGITS = "০১২৩৪৫৬৭৮৯";

/** Bangla digits to ASCII; everything else unchanged. */
function asciiDigits(s: string): string {
  return s.replace(/[০-৯]/g, (d) => String(BANGLA_DIGITS.indexOf(d)));
}

/**
 * `text` in `unit` as a canonical bytes-per-second string within the contract's bounds, or null.
 * Accepts a whole number or up to three decimals (`.` or `,`), so the result is always an integer.
 */
export function toBytesPerSecond(text: string, unit: RateUnit): string | null {
  const m = /^(\d{1,13})(?:[.,](\d{1,3}))?$/.exec(asciiDigits(text.trim()));
  if (!m) return null;
  const whole = BigInt(m[1] ?? "0");
  const frac = m[2] ?? "";
  const scale = 10n ** BigInt(frac.length);
  const bytes = whole * UNIT_BYTES[unit] + (BigInt(frac || "0") * UNIT_BYTES[unit]) / scale;
  if (bytes < MIN_BYTES_PER_SECOND || bytes > MAX_BYTES_PER_SECOND) return null;
  return bytes.toString();
}

/** A stored value split for editing: MB/s when it is a whole number of MB/s, else KB/s. */
export function splitRate(bytesPerSecond: string): { value: string; unit: RateUnit } {
  const n = BigInt(bytesPerSecond);
  const unit: RateUnit = n >= UNIT_BYTES.mb && n % UNIT_BYTES.mb === 0n ? "mb" : "kb";
  const size = UNIT_BYTES[unit];
  const whole = n / size;
  const rest = n % size;
  if (rest === 0n) return { value: whole.toString(), unit };
  // Up to three decimals; a remainder below that shows rounded down (the saved value is exact).
  const digits = ((rest * 1000n) / size).toString().padStart(3, "0").replace(/0+$/, "");
  return { value: digits ? `${whole}.${digits}` : whole.toString(), unit };
}

/** For reading: `512 KB/s`, `2.5 MB/s`, `1000 GB/s` (decimal units, one decimal at most). */
export function formatRate(bytesPerSecond: string): string {
  const n = BigInt(bytesPerSecond);
  for (const [size, label] of [
    [1_000_000_000n, "GB/s"],
    [1_000_000n, "MB/s"],
    [1000n, "KB/s"],
  ] as const) {
    if (n >= size) {
      const whole = n / size;
      const tenth = ((n % size) * 10n) / size;
      return tenth === 0n || whole >= 100n ? `${whole} ${label}` : `${whole}.${tenth} ${label}`;
    }
  }
  return `${n} B/s`;
}

/** Whether the connected engine reports `capability` as supported (ARCH-04: no silent fallback). */
export function engineSupports(
  backend: BackendInfo | undefined,
  capability: "speed_limit_global" | "speed_limit_per_job" | "http_proxy",
): boolean {
  return (
    backend?.capabilities.some((c) => c.capability === capability && c.support === "supported") ??
    false
  );
}
