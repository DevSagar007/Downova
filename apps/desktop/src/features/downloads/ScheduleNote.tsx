import { useEffect, useState } from "react";
import type { DownloadSnapshot, QueueInfo } from "@dm/contracts";
import { usePreferences } from "../../app/preferences";
import { format } from "../../i18n/messages";
import { useDownloads } from "../../services/ServiceProvider";
import { Banner } from "../../ui/Banner";
import { formatInstant } from "../queues/QueueScheduleEditor";
import { MAIN_QUEUE } from "./categories";

/**
 * Why a download waits for, or runs outside, its queue's schedule (CORE-08). Built only from the
 * service's state: the job's `schedule_window` blocker or `schedule_exemption`, and its queue's
 * evaluated `schedule_state`. Nothing is computed here beyond formatting an instant in the
 * schedule's zone. Manual pause and terminal phases take precedence (nothing is shown).
 */
/** The wall clock, re-read every few seconds (for deciding whether a shown instant has passed). */
function useNow(everyMs = 5000): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), everyMs);
    return () => clearInterval(timer);
  }, [everyMs]);
  return now;
}

export function ScheduleNote({ job }: { job: DownloadSnapshot }) {
  const { t, locale } = usePreferences();
  const { queues } = useDownloads();
  const now = useNow();
  if (job.desired_state === "paused") return null;
  if (["completed", "failed", "cancelled"].includes(job.phase)) return null;
  const queue: QueueInfo | undefined = queues.find(
    (q) => q.queue_id === (job.queue_id ?? MAIN_QUEUE),
  );
  const zone = queue?.schedule?.timezone;
  const name = queue
    ? queue.queue_id === MAIN_QUEUE
      ? t("categories.mainQueue")
      : queue.name
    : null;

  if (job.schedule_exemption === "no_resumable_checkpoint") {
    return (
      <Banner tone="info" title={t("scheduleNote.finishingTitle")}>
        <p>{t("scheduleNote.finishing")}</p>
      </Banner>
    );
  }
  if (!job.blockers.includes("schedule_window")) return null;
  const state = queue?.schedule_state;
  const next = state?.next_change_at ?? null;
  // A next change already in the past means the queue state is being refreshed.
  const stale = next !== null && Date.parse(next) <= now;
  let when: string;
  if (!queue?.schedule || !state) when = t("scheduleNote.unknown");
  else if (stale) when = t("scheduleNote.updating");
  else if (next === null) when = t("scheduleNote.noWindow");
  else
    when = format(t("scheduleNote.next"), {
      time: formatInstant(next, zone ?? "UTC", locale),
      zone: zone ?? "UTC",
    });
  return (
    <Banner
      tone="info"
      title={format(t("scheduleNote.waitingTitle"), { queue: name ?? t("scheduleNote.queue") })}
    >
      <p>{when}</p>
      <p>{t("schedule.asleep")}</p>
    </Banner>
  );
}
