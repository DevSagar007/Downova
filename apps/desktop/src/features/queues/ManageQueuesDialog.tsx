import { useId, useState } from "react";
import type { QueueInfo } from "@dm/contracts";
import { usePreferences } from "../../app/preferences";
import { useDownloads, useServiceActions } from "../../services/ServiceProvider";
import { toCallError, type ServiceCallError } from "../../services/service-client";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { MAIN_QUEUE } from "../downloads/categories";
import "./queues.css";

type Outcome = { kind: "saved" } | { kind: "failed"; failure: ServiceCallError } | null;

/** The longest queue name the service accepts. */
const MAX_NAME = 64;

/**
 * Downloads → Manage queues (CORE-05, ADR 0017). Create, rename, limit, start/stop and delete
 * queues. Deleting never deletes downloads or files: the queue's downloads (finished ones
 * included) move to the queue the user picks; the main queue cannot be deleted.
 */
export function ManageQueuesDialog({ onClose }: { onClose: () => void }) {
  const { t } = usePreferences();
  const { queues } = useDownloads();
  const actions = useServiceActions();
  const newId = useId();
  const [newName, setNewName] = useState("");
  const [outcome, setOutcome] = useState<Outcome>(null);
  const [busy, setBusy] = useState(false);

  async function run(work: () => Promise<unknown>) {
    if (busy) return;
    setBusy(true);
    setOutcome(null);
    try {
      await work();
      setOutcome({ kind: "saved" });
    } catch (thrown) {
      setOutcome({ kind: "failed", failure: toCallError(thrown, true) });
    } finally {
      setBusy(false);
    }
  }

  const name = (q: QueueInfo) => (q.queue_id === MAIN_QUEUE ? t("categories.mainQueue") : q.name);
  const busyReason = busy ? { unavailableReason: t("unavailable.busy") } : {};

  return (
    <Dialog
      title={t("queues.title")}
      closeLabel={t("options.close")}
      onClose={onClose}
      footer={
        <Button variant="primary" onClick={onClose}>
          {t("options.done")}
        </Button>
      }
    >
      <div className="queues">
        <ul className="queues__list" aria-label={t("queues.list")}>
          {queues.map((q) => (
            <QueueRow
              key={q.queue_id}
              queue={q}
              label={name(q)}
              others={queues.filter((o) => o.queue_id !== q.queue_id)}
              nameOf={name}
              busyReason={busyReason}
              run={run}
            />
          ))}
        </ul>
        <form
          className="queues__create"
          onSubmit={(e) => {
            e.preventDefault();
            const trimmed = newName.trim();
            if (trimmed === "") return;
            void run(async () => {
              await actions.createQueue(trimmed);
              setNewName("");
            });
          }}
        >
          <label htmlFor={newId}>{t("queues.new")}</label>
          <input
            id={newId}
            value={newName}
            maxLength={MAX_NAME}
            onChange={(e) => setNewName(e.target.value)}
          />
          <Button type="submit" {...busyReason}>
            {t("queues.create")}
          </Button>
        </form>
        <p className="queues__note">{t("queues.deleteNote")}</p>
        {outcome?.kind === "saved" && <p role="status">{t("queues.saved")}</p>}
        {outcome?.kind === "failed" && (
          <div role="alert" className="queues__problem">
            <p>{t("queues.failed")}</p>
            {outcome.failure.outcome === "rejected" ? (
              <p>{t(`error.kind.${outcome.failure.error.kind}`)}</p>
            ) : (
              <p>{t("command.uncertain")}</p>
            )}
          </div>
        )}
      </div>
    </Dialog>
  );
}

function QueueRow({
  queue,
  label,
  others,
  nameOf,
  busyReason,
  run,
}: {
  queue: QueueInfo;
  label: string;
  others: QueueInfo[];
  nameOf: (q: QueueInfo) => string;
  busyReason: { unavailableReason?: string };
  run: (work: () => Promise<unknown>) => Promise<void>;
}) {
  const { t } = usePreferences();
  const actions = useServiceActions();
  const nameId = useId();
  const limitId = useId();
  const moveId = useId();
  const headingId = useId();
  const main = queue.queue_id === MAIN_QUEUE;
  const [name, setName] = useState(queue.name);
  const [limit, setLimit] = useState(String(queue.max_active));
  const [moveTo, setMoveTo] = useState(others[0]?.queue_id ?? "");
  const status = t(queue.running ? "queues.status.running" : "queues.status.stopped");

  return (
    <li className="queues__item" aria-labelledby={headingId}>
      <h3 id={headingId} className="queues__name">
        <bdi>{label}</bdi> <span className="queues__status">({status})</span>
      </h3>
      {!main && (
        <div className="queues__field">
          <label htmlFor={nameId}>{t("queues.name")}</label>
          <input
            id={nameId}
            value={name}
            maxLength={MAX_NAME}
            onChange={(e) => setName(e.target.value)}
          />
          <Button
            onClick={() => void run(() => actions.renameQueue(queue.queue_id, name.trim()))}
            {...busyReason}
          >
            {t("queues.rename")}
          </Button>
        </div>
      )}
      <div className="queues__field">
        <label htmlFor={limitId}>{t("queues.limit")}</label>
        <input
          id={limitId}
          type="number"
          inputMode="numeric"
          min={1}
          max={16}
          value={limit}
          onChange={(e) => setLimit(e.target.value)}
        />
        <Button
          onClick={() =>
            void run(() => actions.setQueueLimit(queue.queue_id, Number.parseInt(limit, 10)))
          }
          {...busyReason}
        >
          {t("queues.saveLimit")}
        </Button>
      </div>
      <div className="queues__field">
        <Button
          onClick={() =>
            void run(() =>
              queue.running
                ? actions.stopQueue(queue.queue_id)
                : actions.startQueue(queue.queue_id),
            )
          }
          {...busyReason}
        >
          {queue.running ? t("toolbar.stopQueue") : t("toolbar.startQueue")}
        </Button>
      </div>
      {main ? (
        <p className="queues__note">{t("queues.mainNote")}</p>
      ) : (
        <div className="queues__field">
          {/* The service counts every download of the queue, finished and removed ones too, so
              a target is always sent; the main queue always exists. */}
          <>
            <label htmlFor={moveId}>{t("queues.deleteTo")}</label>
            <select id={moveId} value={moveTo} onChange={(e) => setMoveTo(e.target.value)}>
              {others.map((o) => (
                <option key={o.queue_id} value={o.queue_id}>
                  {nameOf(o)}
                </option>
              ))}
            </select>
          </>
          <Button
            onClick={() => void run(() => actions.deleteQueue(queue.queue_id, moveTo))}
            {...busyReason}
          >
            {t("queues.delete")}
          </Button>
        </div>
      )}
    </li>
  );
}
