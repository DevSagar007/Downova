// Messages between the popup and the background worker (same extension only).
export type PopupRequest =
  | { type: "retry"; id: string }
  | { type: "dismiss"; id: string }
  | { type: "clear" }
  | { type: "hello" };

export const STORAGE_KEY = "handoffs";

export function isPopupRequest(m: unknown): m is PopupRequest {
  if (typeof m !== "object" || m === null) return false;
  const r = m as Record<string, unknown>;
  if (r.type === "clear" || r.type === "hello") return true;
  return (r.type === "retry" || r.type === "dismiss") && typeof r.id === "string";
}
