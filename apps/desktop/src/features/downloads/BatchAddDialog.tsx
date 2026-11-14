import { useId, useMemo, useRef, useState } from "react";
import { usePreferences } from "../../app/preferences";
import { format, type MessageKey } from "../../i18n/messages";
import { useServiceActions } from "../../services/ServiceProvider";
import { toCallError } from "../../services/service-client";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { MAX_BATCH, parseLinks } from "./batch";
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
  const parsed = useMemo(() => parseLinks(text), [text]);
  const pending = parsed.links.filter((l) => {
    const o = outcomes.get(l);
    return o === undefined || o.state === "uncertain" || o.state === "failed";
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
        {outcomes.size > 0 && (
          <>
            <p role="status" className="batch-add__summary">
              {format(t("batch.summary"), {
                added: counts.added,
                duplicate: counts.duplicate,
                other: counts.other,
              })}
            </p>
            <ul className="batch-add__results" aria-label={t("batch.results")}>
              {parsed.links
                .filter((l) => outcomes.has(l))
                .map((link) => {
                  const o = outcomes.get(link) as Outcome;
                  return (
                    <li key={link} data-state={o.state}>
                      <bdi dir="ltr" className="batch-add__link">
                        {link}
                      </bdi>
                      <span>{outcomeText(o)}</span>
                    </li>
                  );
                })}
            </ul>
          </>
        )}
      </div>
    </Dialog>
  );
}
