import { useState, type KeyboardEvent } from "react";
import type { DownloadSnapshot } from "@dm/contracts";
import {
  ArrowDown,
  ArrowDownToLine,
  ArrowUp,
  Ban,
  CircleCheck,
  CirclePause,
  CircleX,
  Clock,
  File,
  FileCheck,
  FileArchive,
  FileCog,
  FileMusic,
  FileText,
  FileVideo,
  Hourglass,
  PlugZap,
  ScanSearch,
  ShieldCheck,
} from "lucide-react";
import { usePreferences } from "../../app/preferences";
import { format, type MessageKey } from "../../i18n/messages";
import { fileTypeOf, type FileTypeCategory } from "./categories";
import {
  descriptionText,
  percentOf,
  progressPermille,
  sizeText,
  statusKind,
  statusText,
  STATUS_TONE,
  type StatusKind,
} from "./present";
import "./DownloadTable.css";

export interface DownloadTableProps {
  categoryLabel: string;
  downloads: DownloadSnapshot[];
  connected: boolean;
  /** The first list has not arrived yet. */
  loading?: boolean;
  /** A local search is filtering the list. */
  searchActive?: boolean;
  /** How many rows the category holds before the search (to tell no matches from an empty list). */
  categoryCount?: number;
  /** Selection is by job ID, so a row stays selected when its file name changes. */
  selectedId?: string | null;
  onSelect?: (jobId: string) => void;
}

type SortKey = "name" | "size" | "status";
type Sort = { key: SortKey; direction: "ascending" | "descending" } | null;

interface Column {
  labelKey: MessageKey;
  className: string;
  /** Only columns backed by service data can be sorted. */
  sortKey?: SortKey;
}

const COLUMNS: Column[] = [
  { labelKey: "table.fileName", className: "name", sortKey: "name" },
  { labelKey: "table.size", className: "size", sortKey: "size" },
  { labelKey: "table.status", className: "status", sortKey: "status" },
  { labelKey: "table.timeLeft", className: "time" },
  { labelKey: "table.rate", className: "rate" },
  { labelKey: "table.lastTry", className: "last" },
  { labelKey: "table.description", className: "desc" },
];

const STATUS_ICONS: Record<StatusKind, typeof File> = {
  completed: CircleCheck,
  failed: CircleX,
  cancelled: Ban,
  paused: CirclePause,
  waiting: Hourglass,
  queued: Clock,
  connecting: PlugZap,
  transferring: ArrowDownToLine,
  verifying: ShieldCheck,
  scanning: ScanSearch,
  finalizing: FileCheck,
};

const TYPE_ICONS: Record<FileTypeCategory, typeof File> = {
  compressed: FileArchive,
  documents: FileText,
  music: FileMusic,
  programs: FileCog,
  video: FileVideo,
};

/**
 * Columned download list. Columns follow the classic layout; values that the v1 service contract
 * does not provide yet (time left, transfer rate, last try) show "—" rather than an estimate.
 */
export function DownloadTable({
  categoryLabel,
  downloads,
  connected,
  loading = false,
  searchActive = false,
  categoryCount = downloads.length,
  selectedId = null,
  onSelect,
}: DownloadTableProps) {
  const { t, locale } = usePreferences();
  const [sort, setSort] = useState<Sort>(null);

  const toggleSort = (key: SortKey) =>
    setSort((prev) =>
      prev?.key === key && prev.direction === "ascending"
        ? { key, direction: "descending" }
        : { key, direction: "ascending" },
    );

  const rows = sort
    ? [...downloads].sort(comparator(sort, (d) => statusText(d, t), locale))
    : downloads;

  // ↑/↓ on a row's select button selects and focuses the neighbouring row (visual order).
  function moveSelection(event: KeyboardEvent<HTMLButtonElement>, index: number) {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    const next = rows[index + (event.key === "ArrowDown" ? 1 : -1)];
    if (!next) return;
    event.preventDefault();
    onSelect?.(next.job_id);
    const table = event.currentTarget.closest("table");
    table?.querySelector<HTMLButtonElement>(`[data-job-id="${CSS.escape(next.job_id)}"]`)?.focus();
  }

  return (
    <div className="download-table__scroll">
      <table className={`download-table${downloads.length === 0 ? " download-table--empty" : ""}`}>
        <caption className="visually-hidden">
          {format(t("table.caption"), { category: categoryLabel })}
        </caption>
        <colgroup>
          {COLUMNS.map((c) => (
            <col key={c.className} className={`download-table__col--${c.className}`} />
          ))}
        </colgroup>
        <thead>
          <tr>
            {COLUMNS.map((c) => {
              const label = t(c.labelKey);
              const active = sort !== null && sort.key === c.sortKey;
              return (
                <th
                  key={c.className}
                  scope="col"
                  title={label}
                  aria-sort={active ? sort.direction : undefined}
                >
                  {c.sortKey ? (
                    <button
                      type="button"
                      className="download-table__sort"
                      onClick={() => c.sortKey && toggleSort(c.sortKey)}
                    >
                      <span className="download-table__sort-label">{label}</span>
                      {active &&
                        (sort.direction === "ascending" ? (
                          <ArrowUp className="icon" aria-hidden="true" />
                        ) : (
                          <ArrowDown className="icon" aria-hidden="true" />
                        ))}
                    </button>
                  ) : (
                    label
                  )}
                </th>
              );
            })}
          </tr>
        </thead>
        <tbody>
          {rows.length === 0 ? (
            <tr className="download-table__empty">
              <td colSpan={COLUMNS.length}>
                {loading ? (
                  <p>{t("table.loading")}</p>
                ) : searchActive && categoryCount > 0 ? (
                  <>
                    <p>{t("table.noMatches")}</p>
                    <p className="download-table__hint">{t("table.noMatchesHint")}</p>
                  </>
                ) : (
                  <>
                    <p>{t("table.empty")}</p>
                    <p className="download-table__hint">
                      {connected ? t("table.emptyHint") : t("table.emptyDisconnected")}
                    </p>
                  </>
                )}
              </td>
            </tr>
          ) : (
            rows.map((d, index) => {
              const type = fileTypeOf(d.display_name);
              const TypeIcon = type ? TYPE_ICONS[type] : File;
              const selected = d.job_id === selectedId;
              return (
                <tr
                  key={d.job_id}
                  data-phase={d.phase}
                  className={selected ? "download-table__row--selected" : undefined}
                  onClick={() => onSelect?.(d.job_id)}
                >
                  <th scope="row" className="download-table__name">
                    <button
                      type="button"
                      className="download-table__select"
                      aria-pressed={selected}
                      data-job-id={d.job_id}
                      onClick={(e) => {
                        e.stopPropagation();
                        onSelect?.(d.job_id);
                      }}
                      onKeyDown={(e) => moveSelection(e, index)}
                    >
                      <TypeIcon
                        className={`icon download-table__type download-table__type--${type ?? "other"}`}
                        aria-hidden="true"
                      />
                      <bdi title={d.display_name}>{d.display_name}</bdi>
                    </button>
                  </th>
                  <td className="download-table__num">{sizeText(d, t)}</td>
                  <StatusCell download={d} />
                  <td className="download-table__num">—</td>
                  <td className="download-table__num">—</td>
                  <td>—</td>
                  <td className="download-table__desc">{descriptionText(d, t)}</td>
                </tr>
              );
            })
          )}
        </tbody>
      </table>
    </div>
  );
}

function comparator(
  sort: NonNullable<Sort>,
  status: (d: DownloadSnapshot) => string,
  locale: string,
): (a: DownloadSnapshot, b: DownloadSnapshot) => number {
  const sign = sort.direction === "ascending" ? 1 : -1;
  const collator = new Intl.Collator(locale, { numeric: true, sensitivity: "base" });
  return (a, b) => {
    switch (sort.key) {
      case "name":
        return sign * collator.compare(a.display_name, b.display_name);
      case "status":
        return sign * collator.compare(status(a), status(b));
      case "size": {
        // Unknown sizes always sort last, whatever the direction.
        if (a.total_bytes === null || b.total_bytes === null) {
          return a.total_bytes === b.total_bytes ? 0 : a.total_bytes === null ? 1 : -1;
        }
        const diff = BigInt(a.total_bytes) - BigInt(b.total_bytes);
        return sign * (diff > 0n ? 1 : diff < 0n ? -1 : 0);
      }
    }
  };
}

/**
 * Status: an icon and its tone (never colour alone) with the status text, and a compact progress
 * bar from the service's byte counts while unfinished with a known total. Full bytes still read
 * "Downloading 100.0%" or "Verifying" until the service reports completion.
 */
function StatusCell({ download }: { download: DownloadSnapshot }) {
  const { t } = usePreferences();
  const kind = statusKind(download);
  const Icon = STATUS_ICONS[kind];
  const permille = progressPermille(download);
  const text = statusText(download, t);
  return (
    <td className="download-table__status" data-tone={STATUS_TONE[kind]}>
      <span className="download-status">
        <Icon className="icon download-status__icon" aria-hidden="true" />
        <span className="download-status__text">{text}</span>
      </span>
      {permille !== null && (
        <progress
          className={
            kind === "paused" || kind === "waiting"
              ? "download-status__bar download-status__bar--held"
              : "download-status__bar"
          }
          max={1000}
          value={permille}
          aria-label={format(t("table.progressOf"), { name: download.display_name })}
          aria-valuetext={percentOf(download.downloaded_bytes, download.total_bytes) ?? undefined}
        />
      )}
    </td>
  );
}
