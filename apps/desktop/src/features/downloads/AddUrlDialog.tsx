import { useId, useRef, useState, type FormEvent } from "react";
import type { DownloadSnapshot } from "@dm/contracts";
import { usePreferences } from "../../app/preferences";
import type { MessageKey } from "../../i18n/messages";
import { useServiceActions } from "../../services/ServiceProvider";
import { toCallError, type ServiceCallError } from "../../services/service-client";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { checkUrl, newIdempotencyKey, type UrlProblem } from "./commands";
import "./AddUrlDialog.css";

type Problem = { kind: "invalid"; why: UrlProblem } | { kind: "call"; failure: ServiceCallError };

/**
 * Add URL (CORE-01). One link per submission through `downloads.v1.add`. The idempotency key
 * belongs to the link text: retrying an unacknowledged ("uncertain") submission of the same link
 * reuses it, so the service returns the same job instead of adding a second one. Nothing is ever
 * resent without the user selecting Add again. The unconfirmed request is `unconfirmed` (state of the
 * window), so closing and reopening the dialog after an uncertain answer keeps the same key.
 */
export type PendingAdd = { url: string; key: string } | null;

export function AddUrlDialog({
  onClose,
  onAdded,
  unconfirmed,
  onUnconfirmed,
}: {
  onClose: () => void;
  onAdded: (job: DownloadSnapshot) => void;
  unconfirmed: PendingAdd;
  onUnconfirmed: (next: PendingAdd) => void;
}) {
  const { t } = usePreferences();
  const { add } = useServiceActions();
  const formId = useId();
  const inputId = useId();
  const hintId = useId();
  const problemId = useId();
  const [url, setUrl] = useState(unconfirmed?.url ?? "");
  const [pending, setPending] = useState(false);
  const [problem, setProblem] = useState<Problem | null>(null);
  // Synchronous guards: React state can lag behind a fast double submit.
  const busy = useRef(false);

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (busy.current) return;
    const link = url.trim();
    const why = checkUrl(link);
    if (why) {
      setProblem({ kind: "invalid", why });
      return;
    }
    const request =
      unconfirmed?.url === link ? unconfirmed : { url: link, key: newIdempotencyKey() };
    onUnconfirmed(request);
    busy.current = true;
    setPending(true);
    setProblem(null);
    try {
      const job = await add(link, request.key);
      onUnconfirmed(null);
      onAdded(job);
    } catch (thrown) {
      setProblem({ kind: "call", failure: toCallError(thrown, true) });
    } finally {
      busy.current = false;
      setPending(false);
    }
  }

  const message = problem ? problemText(problem, t) : null;
  return (
    <Dialog
      title={t("addUrl.title")}
      closeLabel={t("options.close")}
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>{t("addUrl.cancel")}</Button>
          <Button
            variant="primary"
            type="submit"
            form={formId}
            {...(pending ? { unavailableReason: t("addUrl.adding") } : {})}
          >
            {pending ? t("addUrl.adding") : t("addUrl.submit")}
          </Button>
        </>
      }
    >
      <form id={formId} className="add-url" onSubmit={(e) => void submit(e)} noValidate>
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
            if (problem?.kind === "invalid") setProblem(null);
          }}
        />
        <p id={hintId} className="add-url__hint">
          {t("addUrl.hint")}
        </p>
        {message && (
          <div id={problemId} role="alert" className="add-url__problem">
            {message.map((line) => (
              <p key={line}>{line}</p>
            ))}
          </div>
        )}
      </form>
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
