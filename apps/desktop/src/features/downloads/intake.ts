// Links that reach the window without the Add URL dialog (B4): dropped onto it (F03) or copied to
// the clipboard while the monitor is on (F02). Both only prefill a dialog; nothing starts by itself.
import { parseLinks, MAX_LIST_BYTES } from "./batch";
import { fileTypeOf } from "./categories";

/** What a drop carried, as far as the window can tell. */
export type DropResult =
  { kind: "links"; links: string[] } | { kind: "listFile"; file: File } | { kind: "unsupported" };

/** The text types a drop is read from, most specific first. */
const URI_LIST = "text/uri-list";
const PLAIN = "text/plain";

/** Whether a drag can carry something the window accepts (shown before the drop). */
export function acceptsDrag(types: readonly string[]): boolean {
  return types.includes(URI_LIST) || types.includes(PLAIN) || types.includes("Files");
}

function isListFile(file: File): boolean {
  const name = file.name.toLowerCase();
  return (
    file.size <= MAX_LIST_BYTES &&
    (file.type === "text/plain" || [".txt", ".lst", ".csv"].some((ext) => name.endsWith(ext)))
  );
}

/**
 * Reads a drop: an http(s) link list (\`text/uri-list\`, comment lines skipped) or text holding
 * links, else one text list file. Anything else (images, other files, text without links) is
 * unsupported and the window offers paste or import instead.
 */
export function readDrop(data: Pick<DataTransfer, "getData" | "files" | "types">): DropResult {
  const types = Array.from(data.types);
  if (types.includes(URI_LIST)) {
    const text = data
      .getData(URI_LIST)
      .split(/\r?\n/)
      .filter((l) => !l.startsWith("#"))
      .join("\n");
    const { links } = parseLinks(text);
    if (links.length > 0) return { kind: "links", links };
  }
  if (types.includes(PLAIN)) {
    const { links } = parseLinks(data.getData(PLAIN));
    if (links.length > 0) return { kind: "links", links };
  }
  const files = Array.from(data.files ?? []);
  const only = files.length === 1 ? files[0] : undefined;
  if (only && isListFile(only)) return { kind: "listFile", file: only };
  return { kind: "unsupported" };
}

/** Lower-case host of an http(s) link, or null. */
export function hostOf(link: string): string | null {
  try {
    return new URL(link).hostname.toLowerCase();
  } catch {
    return null;
  }
}

/** A link that names a file of a known download type (the monitor's default ignore rule). */
export function looksLikeFile(link: string): boolean {
  try {
    const last = new URL(link).pathname.split("/").pop() ?? "";
    return fileTypeOf(decodeURIComponent(last)) !== null;
  } catch {
    return false;
  }
}

export interface MonitorMemory {
  /** The clipboard text last seen; the same text is never looked at twice. */
  lastText: string | null;
  /** Links already offered (or deliberately passed over) in this session. */
  offered: Set<string>;
}

/**
 * F02: the links a new clipboard text offers. Only http(s) links naming a known file type, not
 * from an ignored site, not offered before in this session. Unchanged text offers nothing.
 */
export function clipboardOffers(
  text: string | null,
  memory: MonitorMemory,
  ignoredHosts: readonly string[],
): string[] {
  if (text === null || text === memory.lastText) return [];
  memory.lastText = text;
  const { links } = parseLinks(text);
  const fresh = links.filter((link) => {
    const host = hostOf(link);
    return (
      host !== null &&
      !ignoredHosts.includes(host) &&
      looksLikeFile(link) &&
      !memory.offered.has(link)
    );
  });
  for (const link of fresh) memory.offered.add(link);
  return fresh;
}
