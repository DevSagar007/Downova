import type { DownloadSnapshot, FileCategory, QueueInfo } from "@dm/contracts";
import type { MessageKey } from "../../i18n/messages";

type FixedId =
  | "all"
  | "compressed"
  | "documents"
  | "music"
  | "programs"
  | "video"
  | "unfinished"
  | "finished"
  | "queues";

/** A fixed node, or one of the service's queues (`queue:<queue_id>`, B2.11). */
export type CategoryId = FixedId | `queue:${string}`;

export interface CategoryNode {
  id: CategoryId;
  /** Fixed nodes are translated; queue nodes carry the service's name instead. */
  labelKey?: MessageKey;
  label?: string;
  children?: CategoryNode[];
}

/** The default queue's ID; its name is shown translated. */
export const MAIN_QUEUE = "main";

export function queueNodeId(queueId: string): CategoryId {
  return `queue:${queueId}`;
}

/** The queue a node stands for, if it is a queue node. */
export function queueOfNode(id: CategoryId): string | null {
  return id.startsWith("queue:") ? id.slice("queue:".length) : null;
}

/** The queue a job belongs to (snapshots from before B2.11 omit it: the main queue). */
export function queueOf(download: DownloadSnapshot): string {
  return download.queue_id ?? MAIN_QUEUE;
}

/** Category tree shown in the left panel (design.md §1); queues come from the service. */
export function categoryTree(queues: QueueInfo[]): CategoryNode[] {
  const queueNodes: CategoryNode[] =
    queues.length > 0
      ? queues.map((q) =>
          q.queue_id === MAIN_QUEUE
            ? { id: queueNodeId(q.queue_id), labelKey: "categories.mainQueue" }
            : { id: queueNodeId(q.queue_id), label: q.name },
        )
      : [{ id: queueNodeId(MAIN_QUEUE), labelKey: "categories.mainQueue" }];
  return [
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
    { id: "queues", labelKey: "categories.queues", children: queueNodes },
  ];
}

/** The tree without service queues (only the main queue), for code that has no service. */
export const CATEGORY_TREE: CategoryNode[] = categoryTree([]);

// Default extension rules; the service applies the same table (crates/domain policy, ADR 0015).
const EXTENSIONS: Partial<Record<CategoryId, readonly string[]>> = {
  compressed: ["zip", "rar", "7z", "tar", "gz", "bz2", "xz", "zst", "iso"],
  documents: ["pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt", "txt", "epub", "csv"],
  music: ["mp3", "flac", "wav", "aac", "m4a", "ogg", "opus", "wma"],
  programs: ["exe", "msi", "msix", "dmg", "pkg", "deb", "rpm", "appimage", "apk"],
  video: ["mp4", "mkv", "webm", "avi", "mov", "wmv", "m4v", "ts"],
};

/** Every destination category, in the order Options and Add URL show them. */
export const FILE_CATEGORIES: readonly FileCategory[] = [
  "compressed",
  "documents",
  "music",
  "programs",
  "video",
  "other",
];

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

const isFinishedPhase = (d: DownloadSnapshot) =>
  d.phase === "completed" || d.phase === "failed" || d.phase === "cancelled";

export function matchesCategory(download: DownloadSnapshot, category: CategoryId): boolean {
  const queue = queueOfNode(category);
  if (queue !== null) return !isFinishedPhase(download) && queueOf(download) === queue;
  switch (category) {
    case "all":
      return true;
    case "unfinished":
      return download.phase !== "completed" && download.phase !== "cancelled";
    case "finished":
      return download.phase === "completed";
    case "queues":
      // Every unfinished download belongs to a queue.
      return !isFinishedPhase(download);
    default: {
      // The service's category when it reports one, otherwise the name's default rule.
      if (download.category) return download.category === category;
      return EXTENSIONS[category]?.includes(extensionOf(download.display_name)) ?? false;
    }
  }
}

/**
 * Queue execution order (B2.11): the service's `queue_position` inside a queue. This is the
 * order jobs start in; column sorting in the table only changes presentation.
 */
export function byQueuePosition(a: DownloadSnapshot, b: DownloadSnapshot): number {
  const pa = BigInt(a.queue_position ?? "0");
  const pb = BigInt(b.queue_position ?? "0");
  return pa < pb ? -1 : pa > pb ? 1 : 0;
}

export function findCategory(id: CategoryId, nodes = CATEGORY_TREE): CategoryNode | undefined {
  for (const node of nodes) {
    if (node.id === id) return node;
    const child = node.children && findCategory(id, node.children);
    if (child) return child;
  }
  return undefined;
}
