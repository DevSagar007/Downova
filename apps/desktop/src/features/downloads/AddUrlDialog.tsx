import { useId, useRef, useState, type FormEvent } from "react";
import type { DownloadSnapshot, DuplicateSummary, FileCategory } from "@dm/contracts";
import { usePreferences } from "../../app/preferences";
import { format, type MessageKey } from "../../i18n/messages";
import { useDownloads, useServiceActions } from "../../services/ServiceProvider";
import { toCallError, type AddOptions, type ServiceCallError } from "../../services/service-client";
import { Banner } from "../../ui/Banner";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { FILE_CATEGORIES, MAIN_QUEUE } from "./categories";
import { checkUrl, newIdempotencyKey, type UrlProblem } from "./commands";
import "./AddUrlDialog.css";

type Problem = { kind: "invalid"; why: UrlProblem } | { kind: "call"; failure: ServiceCallError };

/**
 * Add URL (CORE-01, CORE-07). One link per submission through `downloads.v1.add`.
 *
 * - The idempotency key belongs to the request: retrying an unacknowledged ("uncertain")
 *   submission repeats the same link, options and key, so the service returns the same job
 *   instead of adding a second one. Nothing is resent without the user selecting Add again. The
 *   unconfirmed request lives in the window (`unconfirmed`), so closing and reopening the dialog
 *   keeps it.
 * - Duplicates (B2.8): the dialog first asks the service for jobs with the same link and, if
 *   there are any, offers to show the existing one, download again or cancel. The add itself is
 *   sent with `on_duplicate: refuse`, so a second window adding the same link at the same moment
 *   is answered with the existing job instead of a silent second download. "Download again" is a
 *   new request (new key, `allow`).
 */
export type PendingAdd = { url: string; key: string; options: AddOptions } | null;

export function AddUrlDialog({
  onClose,
  onAdded,
  onShowExisting,
  unconfirmed,
  onUnconfirmed,
}: {
  onClose: () => void;
  onAdded: (job: DownloadSnapshot) => void;
  /** Selects an existing job instead of adding another one. */
  onShowExisting: (jobId: string) => void;
  unconfirmed: PendingAdd;
  onUnconfirmed: (next: PendingAdd) => void;
}) {
  const { t } = usePreferences();
  const { add, checkLink } = useServiceActions();
  const { queues } = useDownloads();
  const formId = useId();
  const inputId = useId();
  const hintId = useId();
  const problemId = useId();
  const categoryId = useId();
  const queueId = useId();
  const [url, setUrl] = useState(unconfirmed?.url ?? "");
  const [category, setCategory] = useState<FileCategory | "">(unconfirmed?.options.category ?? "");
  const [queue, setQueue] = useState(unconfirmed?.options.queueId ?? MAIN_QUEUE);
  const [pending, setPending] = useState<"checking" | "adding" | null>(null);
  const [problem, setProblem] = useState<Problem | null>(null);
  const [duplicates, setDuplicates] = useState<DuplicateSummary[] | null>(null);
  // Synchronous guard: React state can lag behind a fast double submit.
  const busy = useRef(false);

  function options(onDuplicate: NonNullable<AddOptions["onDuplicate"]>): AddOptions {
    const chosen: AddOptions = { onDuplicate };
    if (category !== "") chosen.category = category;
    if (queue !== MAIN_QUEUE) chosen.queueId = queue;
    return chosen;
  }

  async function send(request: NonNullable<PendingAdd>) {
    onUnconfirmed(request);
    setPending("adding");
    try {
      const job = await add(request.url, request.key, request.options);
      onUnconfirmed(null);
      onAdded(job);
    } catch (thrown) {
      const failure = toCallError(thrown, true);
      if (failure.outcome === "rejected" && failure.error.kind === "DUPLICATE_DOWNLOAD") {
        // Someone added the same link meanwhile: nothing was created; show it instead.
        onUnconfirmed(null);
        await showDuplicates(request.url);
      } else {
        setProblem({ kind: "call", failure });
      }
    }
  }

  async function showDuplicates(link: string): Promise<boolean> {
    setPending("checking");
    try {
      const found = await checkLink(link);
      setDuplicates(found.length > 0 ? found : null);
      return found.length > 0;
    } catch (thrown) {
      setProblem({ kind: "call", failure: toCallError(thrown) });
      return true;
    }
  }

  async function guarded(work: () => Promise<void>) {
    if (busy.current) return;
    busy.current = true;
    setProblem(null);
    try {
      await work();
    } finally {
      busy.current = false;
      setPending(null);
    }
  }

  function submit(event: FormEvent) {
    event.preventDefault();
    const link = url.trim();
    const why = checkUrl(link);
    if (why) {
      setProblem({ kind: "invalid", why });
      return;
    }
    void guarded(async () => {
      // An unconfirmed request for this link is repeated exactly (same key and options).
      if (unconfirmed?.url === link) {
        await send(unconfirmed);
        return;
      }
      if (await showDuplicates(link)) return;
      await send({ url: link, key: newIdempotencyKey(), options: options("refuse") });
    });
  }

  function addAgain() {
    const link = url.trim();
    void guarded(async () => {
      setDuplicates(null);
      await send({ url: link, key: newIdempotencyKey(), options: options("allow") });
    });
  }

  const message = problem ? problemText(problem, t) : null;
  const first = duplicates?.[0];
  return (
    <Dialog
      title={t("addUrl.title")}
      closeLabel={t("options.close")}
      onClose={onClose}
      footer={
        first ? null : (
          <>
            <Button onClick={onClose}>{t("addUrl.cancel")}</Button>
            <Button
              variant="primary"
              type="submit"
              form={formId}
              {...(pending
                ? {
                    unavailableReason: t(
                      pending === "checking" ? "addUrl.checking" : "addUrl.adding",
                    ),
                  }
                : {})}
            >
              {pending === "checking"
                ? t("addUrl.checking")
                : pending === "adding"
                  ? t("addUrl.adding")
                  : t("addUrl.submit")}
            </Button>
          </>
        )
      }
    >
      <form id={formId} className="add-url" onSubmit={submit} noValidate>
        <label htmlFor={inputId}>{t("addUrl.label")}</label>
        <input
          id={inputId}
          data-autofocus
          type="url"
          inputMode="url"
          autoComplete="off"
          spellCheck={false}
          dir="ltr"
          value={url}
          aria-invalid={problem?.kind === "invalid" || undefined}
          aria-describedby={message ? `${hintId} ${problemId}` : hintId}
          onChange={(e) => {
            setUrl(e.target.value);
            setDuplicates(null);
            if (problem?.kind === "invalid") setProblem(null);
          }}
        />
        <p id={hintId} className="add-url__hint">
          {t("addUrl.hint")}
        </p>
        <label htmlFor={categoryId}>{t("addUrl.category")}</label>
        <select
          id={categoryId}
          value={category}
          onChange={(e) => setCategory(e.target.value as FileCategory | "")}
        >
          <option value="">{t("addUrl.category.auto")}</option>
          {FILE_CATEGORIES.map((c) => (
            <option key={c} value={c}>
              {t(`categories.${c}`)}
            </option>
          ))}
        </select>
        <label htmlFor={queueId}>{t("addUrl.queue")}</label>
        <select id={queueId} value={queue} onChange={(e) => setQueue(e.target.value)}>
          {(queues.length > 0
            ? queues
            : [{ queue_id: MAIN_QUEUE, name: "", position: 0, max_active: 0, running: true }]
          ).map((q) => (
            <option key={q.queue_id} value={q.queue_id}>
              {q.queue_id === MAIN_QUEUE ? t("categories.mainQueue") : q.name}
            </option>
          ))}
        </select>
        {message && (
          <div id={problemId} role="alert" className="add-url__problem">
            {message.map((line) => (
              <p key={line}>{line}</p>
            ))}
          </div>
        )}
      </form>
      {first && duplicates && (
        <div role="alert" className="add-url__duplicate">
          <Banner tone="warning" title={t("duplicate.title")}>
            <p>
              <bdi>{first.display_name}</bdi>
              {duplicates.length > 1
                ? ` ${format(t("duplicate.more"), { count: duplicates.length - 1 })}`
                : ""}
            </p>
            <p>{t("duplicate.body")}</p>
          </Banner>
          <div className="add-url__choices">
            <Button variant="primary" onClick={() => onShowExisting(first.job_id)}>
              {t("duplicate.show")}
            </Button>
            <Button
              onClick={addAgain}
              {...(pending ? { unavailableReason: t("addUrl.adding") } : {})}
            >
              {t("duplicate.again")}
            </Button>
            <Button onClick={onClose}>{t("duplicate.cancel")}</Button>
          </div>
        </div>
      )}
    </Dialog>
  );
}

function problemText(problem: Problem, t: (key: MessageKey) => string): string[] {
  if (problem.kind === "invalid") return [t(`addUrl.invalid.${problem.why}`)];
  const { outcome, error } = problem.failure;
  if (outcome === "uncertain") return [t("addUrl.uncertain")];
  if (outcome === "unavailable") return [t("addUrl.unavailable")];
  const lines = [t("addUrl.rejected"), t(`error.kind.${error.kind}`)];
  if (error.next_action !== "none") lines.push(t(`error.action.${error.next_action}`));
  return lines;
}
