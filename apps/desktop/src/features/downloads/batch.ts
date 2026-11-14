// Batch add (IDM parity, 2026-11-14): links from the clipboard or an imported list file.
// Each link is checked like a single Add URL (`checkUrl`) and sent by the dialog one at a time.
import { checkUrl } from "./commands";

/** Most links one batch sends; the rest are reported, never silently dropped. */
export const MAX_BATCH = 500;

/** Largest list file read (1 MiB): a list of links, not a data file. */
export const MAX_LIST_BYTES = 1024 * 1024;

export interface ParsedLinks {
  /** Valid http(s) links, first occurrence order, duplicates removed, at most MAX_BATCH. */
  links: string[];
  /** Non-empty tokens that are not addable links (other schemes, text, credentials, …). */
  ignored: number;
  /** Valid links beyond MAX_BATCH. */
  overflow: number;
}

/** Characters often wrapped around a pasted link (quotes, brackets, list punctuation). */
const WRAPPERS = /^[<("'[`]+|[>)"'\]`,;]+$/g;

/**
 * Finds the links in pasted text or a list file: one per line or separated by spaces. Query
 * strings are kept exactly (signed links are never decoded or reordered).
 */
export function parseLinks(text: string): ParsedLinks {
  const seen = new Set<string>();
  const links: string[] = [];
  let ignored = 0;
  let overflow = 0;
  for (const raw of text.split(/\s+/u)) {
    const token = raw.replace(WRAPPERS, "");
    if (token === "") continue;
    if (checkUrl(token) !== null) {
      ignored += 1;
      continue;
    }
    if (seen.has(token)) continue;
    seen.add(token);
    if (links.length >= MAX_BATCH) overflow += 1;
    else links.push(token);
  }
  return { links, ignored, overflow };
}
