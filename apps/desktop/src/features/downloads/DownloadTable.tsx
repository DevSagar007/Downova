import {
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
} from "react";
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
  statusSortText,
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
  /** Receives the rows in their current visual order (sorting applied), for callers that need
   * a neighbour that may not be rendered (B2.24: the row after a removed one). */
  orderRef?: { current: readonly DownloadSnapshot[] };
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

/** Rows rendered beyond the viewport on each side (ADV12). */
export const OVERSCAN = 8;
/** Used until a row has been measured, and where there is no layout (tests). */
const FALLBACK_ROW_PX = 32;
const FALLBACK_VIEWPORT_PX = 640;

/**
 * Which rows to render (ADV12): the viewport plus `OVERSCAN` rows on each side, and always the
 * pinned rows (the selected and the focused one), so selection and keyboard focus survive
 * scrolling. Returns sorted indices; the gaps between them become spacer rows.
 */
export function visibleIndices(
  count: number,
  scrollTop: number,
  viewport: number,
  rowHeight: number,
  pinned: readonly number[],
): number[] {
  if (count === 0) return [];
  const first = Math.max(0, Math.floor(scrollTop / rowHeight) - OVERSCAN);
  const last = Math.min(count - 1, Math.ceil((scrollTop + viewport) / rowHeight) + OVERSCAN);
  const out = new Set<number>();
  for (let i = first; i <= last; i++) out.add(i);
  for (const p of pinned) if (p >= 0 && p < count) out.add(p);
  return [...out].sort((a, b) => a - b);
}

/**
 * Columned download list. Columns follow the classic layout; values that the v1 service contract
 * does not provide yet (time left, transfer rate, last try) show "—" rather than an estimate.
 *
 * Virtualized (ADV12): only the rows in view (plus overscan, the selected and the focused row)
 * are in the DOM; the rest are represented by spacer rows that are hidden from assistive
 * technology. The table states its full size (`aria-rowcount`) and every row its position
 * (`aria-rowindex`), so "row 9,412 of 10,001" stays understandable. Search, filters and sorting
 * work on the whole list before this; nothing offscreen leaves the business state.
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
  orderRef,
}: DownloadTableProps) {
  const { t, locale } = usePreferences();
  const [sort, setSort] = useState<Sort>(null);

  const toggleSort = (key: SortKey) =>
    setSort((prev) =>
      prev?.key === key && prev.direction === "ascending"
        ? { key, direction: "descending" }
        : { key, direction: "ascending" },
    );

  // Sorting is a presentation order only. The order is recomputed only when a sorted field (or
  // the set of rows) changes: progress alone never re-sorts — status sorts by its text without
  // the percentage — so 10,000 rows are not re-sorted on every progress event.
  const sortSignature = sort
    ? JSON.stringify([
        sort.key,
        sort.direction,
        locale,
        downloads.map((d) => [
          d.job_id,
          sort.key === "name"
            ? d.display_name
            : sort.key === "size"
              ? d.total_bytes
              : statusSortText(d, t),
        ]),
      ])
    : "";
  const order = useMemo(() => (sortSignature ? sortedIds(sortSignature) : null), [sortSignature]);
  const rows = useMemo(() => {
    if (!order) return downloads;
    const byId = new Map(downloads.map((d) => [d.job_id, d]));
    return order.map((id) => byId.get(id)).filter((d): d is DownloadSnapshot => d !== undefined);
  }, [downloads, order]);
  const empty = rows.length === 0;

  // ---------------------------------------------------------------- windowing (ADV12)
  const scrollRef = useRef<HTMLDivElement>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [viewport, setViewport] = useState(FALLBACK_VIEWPORT_PX);
  const [rowHeight, setRowHeight] = useState(FALLBACK_ROW_PX);
  const [headHeight, setHeadHeight] = useState(0);
  const [focusedId, setFocusedId] = useState<string | null>(null);
  // A row the keyboard moved to before it was rendered: focused once it is in the DOM.
  const pendingFocus = useRef<string | null>(null);

  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const measure = () => {
      if (el.clientHeight > 0) setViewport(el.clientHeight);
      // Heights in the scroller's own layout pixels, the unit of scrollTop and clientHeight.
      // getBoundingClientRect() is in zoomed pixels under CSS zoom or a transform, which put the
      // rendered window above the viewport at 200 % (B2.24); its fractional precision is kept.
      const box = el.getBoundingClientRect().height;
      const scale = el.offsetHeight > 0 && box > 0 ? box / el.offsetHeight : 1;
      const row = el.querySelector<HTMLElement>("tbody tr[data-row]");
      const h = (row?.getBoundingClientRect().height ?? 0) / scale;
      if (h > 0) setRowHeight(h);
      const head = (el.querySelector("thead")?.getBoundingClientRect().height ?? 0) / scale;
      setHeadHeight(head);
    };
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, [empty]);

  const indexOf = useMemo(() => new Map(rows.map((d, i) => [d.job_id, i])), [rows]);
  useEffect(() => {
    if (orderRef) orderRef.current = rows;
  }, [orderRef, rows]);
  const pinned = [selectedId, focusedId]
    .map((id) => (id === null ? -1 : (indexOf.get(id) ?? -1)))
    .filter((i) => i >= 0);
  const shown = visibleIndices(
    rows.length,
    Math.max(0, scrollTop - headHeight),
    viewport,
    rowHeight,
    pinned,
  );

  /** Scrolls row `index` into view (no-op when it is already fully visible). */
  const reveal = useCallback(
    (index: number) => {
      const el = scrollRef.current;
      if (!el) return;
      const top = headHeight + index * rowHeight;
      const visibleTop = el.scrollTop + headHeight;
      const height = el.clientHeight || viewport;
      if (top < visibleTop) el.scrollTop = top - headHeight;
      else if (top + rowHeight > el.scrollTop + height) el.scrollTop = top + rowHeight - height;
      setScrollTop(el.scrollTop);
    },
    [headHeight, rowHeight, viewport],
  );

  // Focus a row the keyboard moved to once it has been rendered.
  useEffect(() => {
    const id = pendingFocus.current;
    if (!id) return;
    const button = scrollRef.current?.querySelector<HTMLButtonElement>(
      `[data-job-id="${CSS.escape(id)}"]`,
    );
    if (button) {
      pendingFocus.current = null;
      button.focus();
    }
  });

  // The focused row left the list while it had focus (removed by another client, a clean-up or
  // the browser extension): focus would fall to the page, so it moves to the row now at the same
  // position (or the last one), like a removal from the window itself (B2.24).
  const focusedIndex = useRef(-1);
  useEffect(() => {
    if (focusedId === null) return;
    const at = indexOf.get(focusedId);
    if (at !== undefined) {
      focusedIndex.current = at;
      return;
    }
    const lostFocus = document.activeElement === null || document.activeElement === document.body;
    const fallback = rows[Math.min(focusedIndex.current, rows.length - 1)];
    setFocusedId(lostFocus ? (fallback?.job_id ?? null) : null);
    if (lostFocus && fallback) {
      pendingFocus.current = fallback.job_id;
      reveal(Math.min(focusedIndex.current, rows.length - 1));
    }
  }, [focusedId, indexOf, rows, reveal]);

  // ↑/↓, Page Up/Down and Home/End on a row's select button select and focus another row in
  // visual order, scrolling it into view even when it was not rendered yet. Alt+↑/↓ is left to
  // the window: it moves the selected download in its queue (B2.11).
  function moveSelection(event: KeyboardEvent<HTMLButtonElement>, index: number) {
    if (event.altKey) return;
    const page = Math.max(1, Math.floor(viewport / rowHeight) - 1);
    const target =
      event.key === "ArrowDown"
        ? index + 1
        : event.key === "ArrowUp"
          ? index - 1
          : event.key === "PageDown"
            ? Math.min(rows.length - 1, index + page)
            : event.key === "PageUp"
              ? Math.max(0, index - page)
              : event.key === "Home"
                ? 0
                : event.key === "End"
                  ? rows.length - 1
                  : null;
    if (target === null) return;
    const next = rows[target];
    if (!next || target === index) return;
    event.preventDefault();
    onSelect?.(next.job_id);
    pendingFocus.current = next.job_id;
    setFocusedId(next.job_id);
    reveal(target);
  }

  const columns = COLUMNS.length;
  const spacer = (key: string, count: number) =>
    count > 0 ? (
      <tr key={key} className="download-table__spacer" aria-hidden="true">
        <td colSpan={columns} style={{ height: count * rowHeight }} />
      </tr>
    ) : null;
  const body: React.ReactNode[] = [];
  let cursor = 0;
  for (const i of shown) {
    const gap = spacer(`gap-${cursor}`, i - cursor);
    if (gap) body.push(gap);
    const d = rows[i];
    if (d)
      body.push(
        <Row
          key={d.job_id}
          download={d}
          index={i}
          selected={d.job_id === selectedId}
          onSelect={onSelect}
          onMove={moveSelection}
          onFocusRow={setFocusedId}
        />,
      );
    cursor = i + 1;
  }
  const tail = spacer("gap-end", rows.length - cursor);
  if (tail) body.push(tail);

  return (
    <div
      className="download-table__scroll"
      ref={scrollRef}
      onScroll={(e) => setScrollTop(e.currentTarget.scrollTop)}
    >
      <table
        className={`download-table${downloads.length === 0 ? " download-table--empty" : ""}`}
        aria-rowcount={rows.length + 1}
      >
        <caption className="visually-hidden">
          {format(t("table.caption"), { category: categoryLabel })}
        </caption>
        <colgroup>
          {COLUMNS.map((c) => (
            <col key={c.className} className={`download-table__col--${c.className}`} />
          ))}
        </colgroup>
        <thead>
          <tr aria-rowindex={1}>
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
            body
          )}
        </tbody>
      </table>
    </div>
  );
}

/**
 * One row. Memoized: a progress event changes one snapshot object, so only that row re-renders.
 */
const Row = memo(function Row({
  download: d,
  index,
  selected,
  onSelect,
  onMove,
  onFocusRow,
}: {
  download: DownloadSnapshot;
  index: number;
  selected: boolean;
  onSelect: ((jobId: string) => void) | undefined;
  onMove: (event: KeyboardEvent<HTMLButtonElement>, index: number) => void;
  onFocusRow: (jobId: string | null) => void;
}) {
  const { t } = usePreferences();
  const type = fileTypeOf(d.display_name);
  const TypeIcon = type ? TYPE_ICONS[type] : File;
  return (
    <tr
      data-row=""
      data-phase={d.phase}
      aria-rowindex={index + 2}
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
          onFocus={() => onFocusRow(d.job_id)}
          onBlur={() => onFocusRow(null)}
          onKeyDown={(e) => onMove(e, index)}
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
});

/** Job IDs in sorted order, from a sort signature (see `DownloadTable`). */
function sortedIds(signature: string): string[] {
  const [key, direction, locale, entries] = JSON.parse(signature) as [
    SortKey,
    "ascending" | "descending",
    string,
    [string, string | null][],
  ];
  const sign = direction === "ascending" ? 1 : -1;
  const collator = new Intl.Collator(locale, { numeric: true, sensitivity: "base" });
  const compare = (a: string | null, b: string | null): number => {
    if (key !== "size") return sign * collator.compare(a ?? "", b ?? "");
    // Unknown sizes always sort last, whatever the direction.
    if (a === null || b === null) return a === b ? 0 : a === null ? 1 : -1;
    const diff = BigInt(a) - BigInt(b);
    return sign * (diff > 0n ? 1 : diff < 0n ? -1 : 0);
  };
  return [...entries].sort((x, y) => compare(x[1], y[1])).map(([id]) => id);
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
