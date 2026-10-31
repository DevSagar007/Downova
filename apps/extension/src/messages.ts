// Messages between the popup and the background worker (same extension only).
export type PopupRequest =
  | { type: "retry"; id: string }
  | { type: "dismiss"; id: string }
  | { type: "clear" }
  | { type: "hello" }
  // Automatic capture (B2.13): the options page saves settings it already got permissions for.
  | { type: "captureSave"; settings: unknown }
  | { type: "captureClear" };

export const STORAGE_KEY = "handoffs";
/** Capture settings in storage.local (B2.13). */
export const CAPTURE_SETTINGS_KEY = "captureSettings";
/** Request evidence in storage.session (B2.13; memory-only). */
export const EVIDENCE_KEY = "captureEvidence";

export function isPopupRequest(m: unknown): m is PopupRequest {
  if (typeof m !== "object" || m === null) return false;
  const r = m as Record<string, unknown>;
  if (r.type === "clear" || r.type === "hello" || r.type === "captureClear") return true;
  if (r.type === "captureSave") return typeof r.settings === "object" && r.settings !== null;
  return (r.type === "retry" || r.type === "dismiss") && typeof r.id === "string";
}
