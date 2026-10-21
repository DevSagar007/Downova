import type { DownloadSnapshot } from "@dm/contracts";
import type { MessageKey } from "../../i18n/messages";

export type CategoryId =
  | "all"
  | "compressed"
  | "documents"
  | "music"
  | "programs"
  | "video"
  | "unfinished"
  | "finished"
  | "queues"
  | "main-queue";

export interface CategoryNode {
  id: CategoryId;
  labelKey: MessageKey;
  children?: CategoryNode[];
}

/** Category tree shown in the left panel (design.md §1). */
export const CATEGORY_TREE: CategoryNode[] = [
  {
    id: "all",
    labelKey: "categories.all",
    children: [
      { id: "compressed", labelKey: "categories.compressed" },
      { id: "documents", labelKey: "categories.documents" },
      { id: "music", labelKey: "categories.music" },
      { id: "programs", labelKey: "categories.programs" },
      { id: "video", labelKey: "categories.video" },
    ],
  },
  { id: "unfinished", labelKey: "categories.unfinished" },
  { id: "finished", labelKey: "categories.finished" },
  {
    id: "queues",
    labelKey: "categories.queues",
    children: [{ id: "main-queue", labelKey: "categories.mainQueue" }],
  },
];

// Default extension rules (CORE-07). User-editable rules arrive with the rules service (ADV03).
const EXTENSIONS: Partial<Record<CategoryId, readonly string[]>> = {
  compressed: ["zip", "rar", "7z", "tar", "gz", "bz2", "xz", "zst", "iso"],
  documents: ["pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt", "txt", "epub", "csv"],
  music: ["mp3", "flac", "wav", "aac", "m4a", "ogg", "opus", "wma"],
  programs: ["exe", "msi", "msix", "dmg", "pkg", "deb", "rpm", "appimage", "apk"],
  video: ["mp4", "mkv", "webm", "avi", "mov", "wmv", "m4v", "ts"],
};

export function extensionOf(fileName: string): string {
  const dot = fileName.lastIndexOf(".");
  return dot > 0 ? fileName.slice(dot + 1).toLowerCase() : "";
}

export type FileTypeCategory = "compressed" | "documents" | "music" | "programs" | "video";

/** File-type category of a name, or null when no default rule matches. */
export function fileTypeOf(fileName: string): FileTypeCategory | null {
  const ext = extensionOf(fileName);
  const types: FileTypeCategory[] = ["compressed", "documents", "music", "programs", "video"];
  return types.find((type) => EXTENSIONS[type]?.includes(ext)) ?? null;
}

export function matchesCategory(download: DownloadSnapshot, category: CategoryId): boolean {
  switch (category) {
    case "all":
      return true;
    case "unfinished":
      return download.phase !== "completed" && download.phase !== "cancelled";
    case "finished":
      return download.phase === "completed";
    case "queues":
    case "main-queue":
      // Queue membership is not in the v1 snapshot yet; show jobs waiting to run.
      return download.phase === "queued";
    default:
      return EXTENSIONS[category]?.includes(extensionOf(download.display_name)) ?? false;
  }
}

export function findCategory(id: CategoryId, nodes = CATEGORY_TREE): CategoryNode | undefined {
  for (const node of nodes) {
    if (node.id === id) return node;
    const child = node.children && findCategory(id, node.children);
    if (child) return child;
  }
  return undefined;
}
