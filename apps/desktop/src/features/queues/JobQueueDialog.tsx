import { useId, useState } from "react";
import type { DownloadSnapshot, MoveTarget, Priority } from "@dm/contracts";
import { usePreferences } from "../../app/preferences";
import type { MessageKey } from "../../i18n/messages";
import { useDownloads, useServiceActions } from "../../services/ServiceProvider";
import { toCallError, type ServiceCallError } from "../../services/service-client";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { MAIN_QUEUE, queueOf } from "../downloads/categories";
import "./queues.css";

const PRIORITIES: Priority[] = ["high", "normal", "low"];
const MOVES: { to: MoveTarget; label: MessageKey }[] = [
  { to: "top", label: "menu.file.moveTop" },
  { to: "up", label: "menu.file.moveUp" },
  { to: "down", label: "menu.file.moveDown" },
  { to: "bottom", label: "menu.file.moveBottom" },
];

/**
 * File → Queue and priority (CORE-05, ADR 0017) for the selected download: its queue, its
 * priority and its place in the queue's execution order. Every change is one service command;
 * the dialog shows the service's answer (the live list updates by itself).
 */
export function JobQueueDialog({ job, onClose }: { job: DownloadSnapshot; onClose: () => void }) {
  const { t } = usePreferences();
  const { queues, items } = useDownloads();
  const actions = useServiceActions();
  const queueId = useId();
  const priorityId = useId();
  const noteId = useId();
  const orderId = useId();
  // The live snapshot of the job (queue, priority and position change while the dialog is open).
  const current = items.find((d) => d.job_id === job.job_id) ?? job;
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<ServiceCallError | null>(null);

  async function run(work: () => Promise<unknown>) {
    if (busy) return;
    setBusy(true);
    setFailure(null);
    try {
      await work();
    } catch (thrown) {
      setFailure(toCallError(thrown, true));
    } finally {
      setBusy(false);
    }
  }

  const busyReason = busy ? { unavailableReason: t("unavailable.busy") } : {};
  return (
    <Dialog
      title={t("jobQueue.title")}
      closeLabel={t("options.close")}
      onClose={onClose}
      footer={
        <Button variant="primary" onClick={onClose}>
          {t("options.done")}
        </Button>
      }
    >
      <div className="queues">
        <p>
          <bdi>{current.display_name}</bdi>
        </p>
        <div className="queues__field">
          <label htmlFor={queueId}>{t("jobQueue.queue")}</label>
          <select
            id={queueId}
            value={queueOf(current)}
            disabled={busy}
            onChange={(e) => {
              const to = e.target.value;
              void run(() => actions.setJobQueue(current.job_id, to));
            }}
          >
            {queues.map((q) => (
              <option key={q.queue_id} value={q.queue_id}>
                {q.queue_id === MAIN_QUEUE ? t("categories.mainQueue") : q.name}
              </option>
            ))}
          </select>
        </div>
        <div className="queues__field">
          <label htmlFor={priorityId}>{t("jobQueue.priority")}</label>
          <select
            id={priorityId}
            value={current.priority ?? "normal"}
            aria-describedby={noteId}
            disabled={busy}
            onChange={(e) => {
              const priority = e.target.value as Priority;
              void run(() => actions.setPriority(current.job_id, priority));
            }}
          >
            {PRIORITIES.map((p) => (
              <option key={p} value={p}>
                {t(`priority.${p}`)}
              </option>
            ))}
          </select>
        </div>
        <p id={noteId} className="queues__note">
          {t("jobQueue.priorityNote")}
        </p>
        <div role="group" aria-labelledby={orderId} className="queues__field">
          <span id={orderId}>{t("jobQueue.order")}</span>
          {MOVES.map((m) => (
            <Button
              key={m.to}
              onClick={() => void run(() => actions.moveJob(current.job_id, m.to))}
              {...busyReason}
            >
              {t(m.label)}
            </Button>
          ))}
        </div>
        {failure && (
          <div role="alert" className="queues__problem">
            <p>{t("jobQueue.failed")}</p>
            <p>
              {failure.outcome === "rejected"
                ? t(`error.kind.${failure.error.kind}`)
                : t("command.uncertain")}
            </p>
          </div>
        )}
      </div>
    </Dialog>
  );
}
