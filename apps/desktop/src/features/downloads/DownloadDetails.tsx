import { useState } from "react";
import type { DownloadSnapshot } from "@dm/contracts";
import { usePreferences } from "../../app/preferences";
import { format } from "../../i18n/messages";
import { useServiceActions } from "../../services/ServiceProvider";
import { toCallError, type ServiceCallError } from "../../services/service-client";
import { Button } from "../../ui/Button";
import { Banner } from "../../ui/Banner";
import { DownloadSpeedLimit } from "./DownloadSpeedLimit";
import { ScheduleNote } from "./ScheduleNote";
import { ScanNote } from "./ScanNote";
import { SignInNote } from "./SignInNote";
import { errorText } from "./present";

export interface CommandProblem {
  command: "pause" | "resume";
  failure: ServiceCallError;
}

/** A one-off note about the last folder command (B2.19). */
export interface DetailsNotice {
  tone: "warning" | "danger";
  title: string;
  lines: string[];
}

/** Row actions the details offer for the selected download (B2.16–B2.19). Each is present only
 * while it applies to the selection. */
export interface DetailActions {
  onRetry?: (() => void) | undefined;
  onRefreshLink?: (() => void) | undefined;
  onOpenFolder?: (() => void) | undefined;
}

/**
 * Below the list: why the last Stop/Resume did not complete, otherwise the selected download's
 * error with what the user can do (CORE-11 copy, never the service's English detail).
 */
export function DownloadDetails({
  selected,
  problem,
  onCancel,
  notice = null,
  actions = {},
}: {
  selected: DownloadSnapshot | undefined;
  problem: CommandProblem | null;
  /** Opens the Cancel confirmation for the selected job. */
  onCancel?: (() => void) | undefined;
  notice?: DetailsNotice | null;
  actions?: DetailActions;
}) {
  const { t } = usePreferences();
  if (problem) {
    const { outcome, error } = problem.failure;
    const lines =
      outcome === "uncertain"
        ? [t("command.uncertain")]
        : outcome === "unavailable"
          ? [t("command.unavailable")]
          : [
              t(`error.kind.${error.kind}`),
              ...(error.next_action === "none" ? [] : [t(`error.action.${error.next_action}`)]),
            ];
    return (
      <div className="download-details">
        <Banner tone="danger" title={t(`command.${problem.command}Failed`)}>
          {lines.map((line) => (
            <p key={line}>{line}</p>
          ))}
        </Banner>
      </div>
    );
  }
  if (notice) {
    return (
      <div className="download-details">
        <Banner tone={notice.tone} title={notice.title}>
          {notice.lines.map((line) => (
            <p key={line}>{line}</p>
          ))}
        </Banner>
      </div>
    );
  }
  if (selected?.blockers.includes("name_conflict")) {
    return <NameConflict job={selected} onCancel={onCancel} />;
  }
  if (!selected) return null;
  const buttons = (
    <ActionButtons
      retry={actions.onRetry}
      refresh={actions.onRefreshLink}
      folder={selected.phase === "completed" ? actions.onOpenFolder : undefined}
    />
  );
  // CORE-06: unfinished downloads offer their own speed limit (the service refuses finished ones).
  // Keyed by job: another selection gets a fresh control, so a late answer for the previous job
  // can never show on this one.
  const finished = ["completed", "failed", "cancelled"].includes(selected.phase);
  // Siblings of `auth` below, so each key names its role as well as the job.
  const speed = finished ? null : (
    <DownloadSpeedLimit key={`speed-${selected.job_id}`} job={selected} />
  );
  // CORE-08: why it waits for its queue's schedule, or why it runs outside it.
  const note = finished ? null : <ScheduleNote job={selected} />;
  // F07/E06: the attachment check (while it runs, and its durable outcome afterwards).
  const scan = <ScanNote job={selected} />;
  // BRW-05/SEC-02: the sign-in it uses (metadata only), keyed so answers stay with their job.
  const auth = <SignInNote key={`auth-${selected.job_id}`} job={selected} />;
  const text = errorText(selected, t);
  if (text) {
    const lines = [text.action, text.followUp].filter((l): l is string => l !== null);
    if (selected.error?.kind === "CHECKSUM_MISMATCH") lines.push(t("details.checksumMismatch"));
    return (
      <div className="download-details">
        <Banner tone={selected.phase === "failed" ? "danger" : "warning"} title={text.summary}>
          {lines.length > 0 && lines.map((line) => <p key={line}>{line}</p>)}
          {buttons}
        </Banner>
        {auth}
        {scan}
        {note}
        {speed}
      </div>
    );
  }
  // Completed: how the file was checked (B2.18). A locally computed hash is never presented as
  // a publisher verification.
  const verification = selected.verification;
  if (selected.phase === "completed" && verification) {
    const publisher = verification.source === "publisher_checksum";
    return (
      <div className="download-details">
        <Banner
          tone={publisher ? "success" : "info"}
          title={t(`verification.${verification.source}`)}
        >
          <p>{t(publisher ? "details.checksumPublisher" : "details.checksumLocal")}</p>
          <p>
            {t("details.sha256")}:{" "}
            <code className="download-details__hash" dir="ltr">
              {verification.sha256}
            </code>
          </p>
          {buttons}
        </Banner>
        {auth}
        {scan}
      </div>
    );
  }
  if (selected.checksum_expected && selected.phase !== "cancelled") {
    return (
      <div className="download-details">
        <Banner tone="info" title={t("addUrl.sha256")}>
          <p>{t("details.checksumPending")}</p>
          {buttons}
        </Banner>
        {auth}
        {note}
        {speed}
      </div>
    );
  }
  const any =
    actions.onRetry ??
    actions.onRefreshLink ??
    (selected.phase === "completed" ? actions.onOpenFolder : undefined);
  return any || speed || selected.scan || selected.auth ? (
    <div className="download-details">
      {buttons}
      {auth}
      {scan}
      {note}
      {speed}
    </div>
  ) : null;
}

function ActionButtons({
  retry,
  refresh,
  folder,
}: {
  retry: (() => void) | undefined;
  refresh: (() => void) | undefined;
  folder: (() => void) | undefined;
}) {
  const { t } = usePreferences();
  if (!retry && !refresh && !folder) return null;
  return (
    <div className="download-details__actions">
      {retry && (
        <Button variant="primary" onClick={retry}>
          {t("details.retry")}
        </Button>
      )}
      {refresh && <Button onClick={refresh}>{t("details.refreshLink")}</Button>}
      {folder && <Button onClick={folder}>{t("details.openFolder")}</Button>}
    </div>
  );
}

/**
 * Ask-on-collision (ADR 0015): the verified file waits under its staging name until the user
 * accepts the free name the service chose, or cancels. Nothing is ever overwritten.
 */
function NameConflict({
  job,
  onCancel,
}: {
  job: DownloadSnapshot;
  onCancel?: (() => void) | undefined;
}) {
  const { t } = usePreferences();
  const { confirmName } = useServiceActions();
  const [pending, setPending] = useState(false);
  const [failure, setFailure] = useState<ServiceCallError | null>(null);
  const name = job.display_name;
  async function confirm() {
    if (pending) return;
    setPending(true);
    setFailure(null);
    try {
      await confirmName(job.job_id);
    } catch (thrown) {
      setFailure(toCallError(thrown, true));
    } finally {
      setPending(false);
    }
  }
  return (
    <div className="download-details">
      <Banner tone="warning" title={t("nameConflict.title")}>
        <p>{format(t("nameConflict.body"), { name })}</p>
        {failure && (
          <p role="alert">
            {t("nameConflict.failed")}{" "}
            {failure.outcome === "rejected"
              ? t(`error.kind.${failure.error.kind}`)
              : t("command.uncertain")}
          </p>
        )}
        <div className="download-details__actions">
          <Button
            variant="primary"
            onClick={() => void confirm()}
            {...(pending ? { unavailableReason: t("nameConflict.pending") } : {})}
          >
            {pending ? t("nameConflict.pending") : format(t("nameConflict.confirm"), { name })}
          </Button>
          {onCancel && <Button onClick={onCancel}>{t("cancel.confirm")}</Button>}
        </div>
      </Banner>
    </div>
  );
}
