import { useState } from "react";
import type { DownloadSnapshot } from "@dm/contracts";
import {
  ArrowDown,
  ArrowUp,
  File,
  FileArchive,
  FileCog,
  FileMusic,
  FileText,
  FileVideo,
} from "lucide-react";
import { usePreferences } from "../../app/preferences";
import { format, type MessageKey } from "../../i18n/messages";
import { fileTypeOf, type FileTypeCategory } from "./categories";
import { sizeText, statusText } from "./present";
import "./DownloadTable.css";

export interface DownloadTableProps {
  categoryLabel: string;
  downloads: DownloadSnapshot[];
  connected: boolean;
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
export function DownloadTable({ categoryLabel, downloads, connected }: DownloadTableProps) {
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
                <p>{t("table.empty")}</p>
                {!connected && (
                  <p className="download-table__hint">{t("table.emptyDisconnected")}</p>
                )}
              </td>
            </tr>
          ) : (
            rows.map((d) => {
              const type = fileTypeOf(d.display_name);
              const TypeIcon = type ? TYPE_ICONS[type] : File;
              return (
                <tr key={d.job_id} data-phase={d.phase}>
                  <th scope="row" className="download-table__name">
                    <TypeIcon
                      className={`icon download-table__type download-table__type--${type ?? "other"}`}
                      aria-hidden="true"
                    />
                    <bdi title={d.display_name}>{d.display_name}</bdi>
                  </th>
                  <td className="download-table__num">{sizeText(d, t)}</td>
                  <td>{statusText(d, t)}</td>
                  <td className="download-table__num">—</td>
                  <td className="download-table__num">—</td>
                  <td>—</td>
                  <td className="download-table__desc">{d.error?.detail ?? ""}</td>
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
