import { useId, useMemo, useRef, useState } from "react";
import { usePreferences } from "../../app/preferences";
import { format, type MessageKey } from "../../i18n/messages";
import { useServiceActions } from "../../services/ServiceProvider";
import { toCallError } from "../../services/service-client";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { MAX_BATCH, matchesFilter, parseLinks, previewLink } from "./batch";
import { newIdempotencyKey } from "./commands";
import "./BatchAddDialog.css";

type Outcome =
  | { state: "added" }
  | { state: "duplicate" }
  | { state: "uncertain" }
  | { state: "failed"; kind: MessageKey };

/**
 * Add batch download / Import download list (IDM parity). The links come from the clipboard or a
 * list file and can be edited; each is sent like one Add URL (`downloads.v1.add`, duplicates
 * refused). The idempotency key belongs to the link: adding again after "no confirmation" repeats
 * the same request, so the service answers with the same job instead of a second one.
 */
export function BatchAddDialog({
  initialText,
  note,
  onClose,
  onDone,
}: {
  initialText: string;
  /** Why the text is empty or partial (clipboard unreadable, file too large, …). */
  note: MessageKey | null;
  onClose: () => void;
  /** After a run that added at least one download. */
  onDone: (added: number) => void;
}) {
  const { t } = usePreferences();
  const { add } = useServiceActions();
  const textId = useId();
  const hintId = useId();
  const [text, setText] = useState(initialText);
  const [running, setRunning] = useState(false);
  const [outcomes, setOutcomes] = useState<Map<string, Outcome>>(new Map());
  const keys = useRef(new Map<string, string>());
  const filterId = useId();
  const [filter, setFilter] = useState("");
  // Links the user unticked; a link is sent only when ticked and shown by the filter (F04).
  const [excluded, setExcluded] = useState<Set<string>>(new Set());
  const parsed = useMemo(() => parseLinks(text), [text]);
  const items = useMemo(
    () => parsed.links.map((link) => ({ link, preview: previewLink(link) })),
    [parsed],
  );
  const shownItems = items.filter((i) => matchesFilter(i.link, i.preview, filter));
  const shown = shownItems.map((i) => i.link);
  const selected = shown.filter((l) => !excluded.has(l));
  const sites = new Set(shownItems.filter((i) => !excluded.has(i.link)).map((i) => i.preview.site))
    .size;
  const isOpen = (l: string) => {
    const o = outcomes.get(l);
    return o === undefined || o.state === "uncertain" || o.state === "failed";
  };
  const pending = selected.filter(isOpen);
  const setTicked = (links: string[], ticked: boolean) =>
    setExcluded((prev) => {
      const next = new Set(prev);
      for (const l of links) {
        if (ticked) next.delete(l);
        else next.add(l);
      }
      return next;
    });

  async function run() {
    if (running || pending.length === 0) return;
    setRunning(true);
    let added = 0;
    for (const link of pending) {
      let key = keys.current.get(link);
      if (!key) {
        key = newIdempotencyKey();
        keys.current.set(link, key);
      }
      let outcome: Outcome;
      try {
        await add(link, key, { onDuplicate: "refuse" });
        outcome = { state: "added" };
        added += 1;
      } catch (thrown) {
        const failure = toCallError(thrown, true);
        if (failure.outcome === "rejected" && failure.error.kind === "DUPLICATE_DOWNLOAD") {
          outcome = { state: "duplicate" };
        } else if (failure.outcome === "uncertain") {
          outcome = { state: "uncertain" };
        } else {
          outcome = {
            state: "failed",
            kind:
              failure.outcome === "unavailable"
                ? "addUrl.unavailable"
                : (`error.kind.${failure.error.kind}` as MessageKey),
          };
        }
      }
      setOutcomes((prev) => new Map(prev).set(link, outcome));
    }
    setRunning(false);
    if (added > 0) onDone(added);
  }

  const counts = { added: 0, duplicate: 0, other: 0 };
  for (const o of outcomes.values()) {
    if (o.state === "added") counts.added += 1;
    else if (o.state === "duplicate") counts.duplicate += 1;
    else counts.other += 1;
  }
  const outcomeText = (o: Outcome): string =>
    o.state === "added"
      ? t("batch.added")
      : o.state === "duplicate"
        ? t("batch.duplicate")
        : o.state === "uncertain"
          ? t("batch.uncertain")
          : `${t("batch.failed")} ${t(o.kind)}`;

  return (
    <Dialog
      title={t("batch.title")}
      closeLabel={t("options.close")}
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>{t("batch.close")}</Button>
          <Button
            variant="primary"
            onClick={() => void run()}
            {...(running
              ? { unavailableReason: t("batch.adding") }
              : pending.length === 0
                ? { unavailableReason: t("batch.nothing") }
                : {})}
          >
            {running ? t("batch.adding") : format(t("batch.submit"), { count: pending.length })}
          </Button>
        </>
      }
    >
      <div className="batch-add">
        {note && <p className="batch-add__note">{t(note)}</p>}
        <label htmlFor={textId}>{t("batch.label")}</label>
        <textarea
          id={textId}
          data-autofocus
          rows={8}
          dir="ltr"
          spellCheck={false}
          value={text}
          readOnly={running}
          aria-describedby={hintId}
          onChange={(e) => setText(e.target.value)}
        />
        <p id={hintId} className="batch-add__hint">
          {format(t("batch.count"), { valid: parsed.links.length, ignored: parsed.ignored })}
          {parsed.overflow > 0 &&
            ` ${format(t("batch.overflow"), { max: MAX_BATCH, count: parsed.overflow })}`}
        </p>
        {parsed.links.length > 0 && (
          <>
            <label htmlFor={filterId}>{t("batch.filter")}</label>
            <input
              id={filterId}
              type="search"
              dir="ltr"
              spellCheck={false}
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
            />
            <div className="batch-add__selection">
              <p>
                {format(t("batch.selection"), {
                  selected: selected.length,
                  total: parsed.links.length,
                  sites,
                })}
              </p>
              <Button
                variant="ghost"
                onClick={() => setTicked(shown, true)}
                {...(running ? { unavailableReason: t("batch.adding") } : {})}
              >
                {t("batch.selectShown")}
              </Button>
              <Button
                variant="ghost"
                onClick={() => setTicked(shown.filter(isOpen), false)}
                {...(running ? { unavailableReason: t("batch.adding") } : {})}
              >
                {t("batch.clearShown")}
              </Button>
            </div>
          </>
        )}
        {outcomes.size > 0 && (
          <p role="status" className="batch-add__summary">
            {format(t("batch.summary"), {
              added: counts.added,
              duplicate: counts.duplicate,
              other: counts.other,
            })}
          </p>
        )}
        {parsed.links.length > 0 && shown.length === 0 && (
          <p className="batch-add__hint">{t("batch.noMatch")}</p>
        )}
        {shown.length > 0 && (
          <ul className="batch-add__results" aria-label={t("batch.preview")}>
            {shownItems.map(({ link, preview }) => {
              const o = outcomes.get(link);
              const done = o !== undefined && !isOpen(link);
              return (
                <li key={link} data-state={o?.state ?? "pending"}>
                  <label className="batch-add__item" title={link}>
                    <input
                      type="checkbox"
                      checked={!excluded.has(link)}
                      disabled={running || done}
                      onChange={(e) => setTicked([link], e.target.checked)}
                    />
                    <bdi className="batch-add__link">{preview.name ?? t("batch.noName")}</bdi>
                  </label>
                  <bdi dir="ltr" className="batch-add__site">
                    {preview.site}
                  </bdi>
                  {o && <span className="batch-add__outcome">{outcomeText(o)}</span>}
                </li>
              );
            })}
          </ul>
        )}
      </div>
    </Dialog>
  );
}
