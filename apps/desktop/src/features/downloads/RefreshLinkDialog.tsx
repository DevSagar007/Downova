import { useId, useRef, useState, type FormEvent } from "react";
import type { DownloadSnapshot, ReplaceMode, ReplaceSourceResult } from "@dm/contracts";
import { usePreferences } from "../../app/preferences";
import { format } from "../../i18n/messages";
import { useServiceActions } from "../../services/ServiceProvider";
import { toCallError, type ServiceCallError } from "../../services/service-client";
import { Banner } from "../../ui/Banner";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { checkUrl, newOperationId, type UrlProblem } from "./commands";
import { formatBytes } from "./present";
import "./AddUrlDialog.css";

type Problem = { kind: "invalid"; why: UrlProblem } | { kind: "call"; failure: ServiceCallError };

/**
 * Refresh link (COR-06, ADR 0020) for a paused or failed download.
 *
 * "Use new link" asks the service to keep the progress only when it can prove the new link serves
 * the same file; otherwise nothing changes and the dialog offers Restart from zero (explicit,
 * discards only this download's partial data) or Keep current download (sends nothing).
 * Each request has its own operation ID, fixed per link and mode, so a repeat after an
 * unconfirmed answer never applies twice. The link is never shown back except in its own field.
 */
export function RefreshLinkDialog({
  job,
  onDone,
  onClose,
}: {
  job: DownloadSnapshot;
  onDone: (result: ReplaceSourceResult) => void;
  onClose: () => void;
}) {
  const { t } = usePreferences();
  const { replaceSource } = useServiceActions();
  const formId = useId();
  const inputId = useId();
  const hintId = useId();
  const problemId = useId();
  const [url, setUrl] = useState("");
  const [pending, setPending] = useState(false);
  const [problem, setProblem] = useState<Problem | null>(null);
  const [choice, setChoice] = useState<ReplaceSourceResult | null>(null);
  const busy = useRef(false);
  // One operation per (link, mode): repeats of the same request reuse it.
  const operations = useRef(new Map<string, string>());

  async function send(mode: ReplaceMode) {
    if (busy.current) return;
    const link = url.trim();
    const why = checkUrl(link);
    if (why) {
      setProblem({ kind: "invalid", why });
      return;
    }
    const key = `${mode}\n${link}`;
    const operationId = operations.current.get(key) ?? newOperationId();
    operations.current.set(key, operationId);
    busy.current = true;
    setPending(true);
    setProblem(null);
    try {
      const result = await replaceSource(job.job_id, operationId, link, mode, job.attempt_id);
      if (result.outcome === "choice_required") setChoice(result);
      else onDone(result);
    } catch (thrown) {
      setProblem({ kind: "call", failure: toCallError(thrown, true) });
    } finally {
      busy.current = false;
      setPending(false);
    }
  }

  function submit(event: FormEvent) {
    event.preventDefault();
    void send("keep_if_verified");
  }

  const message = problem ? problemLines(problem, t) : null;
  return (
    <Dialog
      title={t("refresh.title")}
      closeLabel={t("options.close")}
      onClose={onClose}
      footer={
        choice ? (
          <>
            <Button onClick={onClose}>{t("refresh.keepCurrent")}</Button>
            <Button
              variant="primary"
              onClick={() => void send("restart")}
              {...(pending ? { unavailableReason: t("refresh.pending") } : {})}
            >
              {t("refresh.restart")}
            </Button>
          </>
        ) : (
          <>
            <Button onClick={onClose}>{t("refresh.cancel")}</Button>
            <Button
              variant="primary"
              type="submit"
              form={formId}
              {...(pending ? { unavailableReason: t("refresh.pending") } : {})}
            >
              {pending ? t("refresh.pending") : t("refresh.submit")}
            </Button>
          </>
        )
      }
    >
      <p>
        <bdi>{job.display_name}</bdi>
      </p>
      <form id={formId} className="add-url" onSubmit={submit} noValidate>
        <label htmlFor={inputId}>{t("refresh.label")}</label>
        <input
          id={inputId}
          data-autofocus
          type="url"
          inputMode="url"
          autoComplete="off"
          spellCheck={false}
          dir="ltr"
          value={url}
          readOnly={choice !== null}
          aria-invalid={problem?.kind === "invalid" || undefined}
          aria-describedby={message ? `${hintId} ${problemId}` : hintId}
          onChange={(e) => {
            setUrl(e.target.value);
            setChoice(null);
            if (problem?.kind === "invalid") setProblem(null);
          }}
        />
        <p id={hintId} className="add-url__hint">
          {t("refresh.hint")}
        </p>
        {message && (
          <div id={problemId} role="alert" className="add-url__problem">
            {message.map((line) => (
              <p key={line}>{line}</p>
            ))}
          </div>
        )}
      </form>
      {choice && (
        <div role="alert">
          <Banner tone="warning" title={t("refresh.choiceTitle")}>
            <p>{t(`refresh.reason.${choice.reason ?? "unverifiable"}`)}</p>
            <p>
              {format(t("refresh.restartNote"), {
                size: formatBytes(choice.job.downloaded_bytes),
              })}
            </p>
          </Banner>
        </div>
      )}
    </Dialog>
  );
}

function problemLines(problem: Problem, t: ReturnType<typeof usePreferences>["t"]): string[] {
  if (problem.kind === "invalid") return [t(`addUrl.invalid.${problem.why}`)];
  const { outcome, error } = problem.failure;
  if (outcome === "uncertain") return [t("refresh.uncertain")];
  if (outcome === "unavailable") return [t("command.unavailable")];
  const lines = [t("refresh.failed"), t(`error.kind.${error.kind}`)];
  if (error.next_action !== "none") lines.push(t(`error.action.${error.next_action}`));
  return lines;
}
