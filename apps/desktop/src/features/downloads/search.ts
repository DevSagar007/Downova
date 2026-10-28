import type { DownloadSnapshot } from "@dm/contracts";

/** Case-insensitive, Unicode-normalised text used for matching (CORE-10). */
export function normalizeSearchText(value: string): string {
  return value.normalize("NFKC").toLowerCase();
}

/**
 * The source hostname only: never the path, query, fragment or credentials. The service already
 * redacts the URL, but the search must not depend on that.
 */
export function sourceHostname(sourceDisplay: string): string {
  try {
    return new URL(sourceDisplay).hostname.toLowerCase().replace(/\.$/, "");
  } catch {
    return "";
  }
}

/** Match by display name or source hostname. A blank query matches every download. */
export function matchesSearch(download: DownloadSnapshot, query: string): boolean {
  const needle = normalizeSearchText(query.trim());
  if (needle === "") return true;
  if (normalizeSearchText(download.display_name).includes(needle)) return true;
  const host = sourceHostname(download.source_display);
  return host !== "" && host.includes(needle);
}
