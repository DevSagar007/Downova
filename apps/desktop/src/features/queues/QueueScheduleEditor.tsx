import { useId, useState } from "react";
import type { QueueInfo, QueueSchedule, ScheduleWindow, Weekday } from "@dm/contracts";
import { usePreferences } from "../../app/preferences";
import { useServiceActions } from "../../services/ServiceProvider";
import { Button } from "../../ui/Button";

export const WEEKDAYS: readonly Weekday[] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
/** The contract's limit on windows per schedule. */
const MAX_WINDOWS = 14;
const HHMM = /^([01][0-9]|2[0-3]):[0-5][0-9]$/;

/** The system's IANA zone, preselected for a new schedule. */
export function systemTimeZone(): string {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC";
  } catch {
    return "UTC";
  }
}

/** A UTC RFC 3339 instant shown in the schedule's own zone (falls back to the system zone). */
export function formatInstant(iso: string, timeZone: string, locale: "en" | "bn"): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  const options: Intl.DateTimeFormatOptions = {
    weekday: "short",
    hour: "2-digit",
    minute: "2-digit",
    // 24-hour, like the HH:MM the windows are entered in (bn-BD would otherwise show a Latin "PM").
    hourCycle: "h23",
    timeZone,
  };
  const tag = locale === "bn" ? "bn-BD" : "en-GB";
  try {
    return new Intl.DateTimeFormat(tag, options).format(date);
  } catch {
    return new Intl.DateTimeFormat(tag, { ...options, timeZone: undefined }).format(date);
  }
}

/** What the editor can save: windows need a day and two different valid times. */
export function scheduleProblem(s: QueueSchedule): "days" | "times" | "zone" | null {
  if (s.timezone.trim() === "") return "zone";
  for (const w of s.windows) {
    if ((w.days as Weekday[]).length === 0) return "days";
    if (!HHMM.test(w.start) || !HHMM.test(w.end) || w.start === w.end) return "times";
  }
  return null;
}

const blankWindow = (): ScheduleWindow => ({
  days: ["mon", "tue", "wed", "thu", "fri"],
  start: "01:00",
  end: "07:00",
});

/**
 * One queue's schedule (CORE-08, ADR 0025): weekly windows in one IANA zone, the missed-window
 * policy, and what the service evaluated now. The schedule is a policy: a window never resumes
 * a download the user paused and never starts a stopped queue.
 */
export function QueueScheduleEditor({
  queue,
  busyReason,
  run,
}: {
  queue: QueueInfo;
  busyReason: { unavailableReason?: string };
  run: (work: () => Promise<unknown>) => Promise<void>;
}) {
  const { t, locale } = usePreferences();
  const actions = useServiceActions();
  const ids = useId();
  const saved = queue.schedule;
  const [draft, setDraft] = useState<QueueSchedule>(
    () =>
      saved ?? {
        enabled: true,
        timezone: systemTimeZone(),
        windows: [blankWindow()],
        missed_window: "skip",
      },
  );
  const [problem, setProblem] = useState<ReturnType<typeof scheduleProblem>>(null);
  const windows = draft.windows as ScheduleWindow[];
  const setWindows = (next: ScheduleWindow[]) =>
    setDraft({ ...draft, windows: next as QueueSchedule["windows"] });
  const setWindow = (i: number, w: ScheduleWindow) =>
    setWindows(windows.map((x, j) => (j === i ? w : x)));

  const state = queue.schedule_state;
  const zone = saved?.timezone ?? draft.timezone;
  let now: string | null = null;
  if (saved && state) {
    if (!saved.enabled) now = t("schedule.state.disabled");
    else if (state.catch_up_until)
      now = t("schedule.state.catchUp").replace(
        "{time}",
        formatInstant(state.catch_up_until, zone, locale),
      );
    else if (state.in_window)
      now = state.next_change_at
        ? t("schedule.state.inWindowUntil").replace(
            "{time}",
            formatInstant(state.next_change_at, zone, locale),
          )
        : t("schedule.state.inWindow");
    else
      now = state.next_change_at
        ? t("schedule.state.waitingUntil").replace(
            "{time}",
            formatInstant(state.next_change_at, zone, locale),
          )
        : t("schedule.state.waiting");
  }

  function save() {
    const cleaned: QueueSchedule = { ...draft, timezone: draft.timezone.trim() };
    const p = scheduleProblem(cleaned);
    setProblem(p);
    if (p) return;
    void run(() => actions.setQueueSchedule(queue.queue_id, cleaned));
  }

  return (
    <fieldset className="queues__schedule">
      <legend>{t("schedule.title")}</legend>
      {now && (
        <p className="queues__schedule-state" role="status" aria-label={t("schedule.title")}>
          {now}
        </p>
      )}
      <label className="queues__check">
        <input
          type="checkbox"
          checked={draft.enabled}
          onChange={(e) => setDraft({ ...draft, enabled: e.target.checked })}
        />
        {t("schedule.enabled")}
      </label>
      <div className="queues__field">
        <label htmlFor={`${ids}-zone`}>{t("schedule.zone")}</label>
        <input
          id={`${ids}-zone`}
          value={draft.timezone}
          maxLength={64}
          spellCheck={false}
          aria-describedby={`${ids}-zone-hint`}
          onChange={(e) => setDraft({ ...draft, timezone: e.target.value })}
        />
        <span id={`${ids}-zone-hint`} className="queues__note">
          {t("schedule.zoneHint")}
        </span>
      </div>
      <ol className="queues__windows" aria-label={t("schedule.windows")}>
        {windows.map((w, i) => (
          <li key={i} className="queues__window">
            <fieldset>
              <legend>{t("schedule.window").replace("{n}", String(i + 1))}</legend>
              <div className="queues__days">
                {WEEKDAYS.map((d) => (
                  <label key={d} className="queues__check">
                    <input
                      type="checkbox"
                      checked={w.days.includes(d)}
                      onChange={(e) => {
                        const days = e.target.checked
                          ? WEEKDAYS.filter((x) => x === d || w.days.includes(x))
                          : w.days.filter((x) => x !== d);
                        setWindow(i, { ...w, days: days as ScheduleWindow["days"] });
                      }}
                    />
                    {t(`schedule.day.${d}`)}
                  </label>
                ))}
              </div>
              <div className="queues__field">
                <label htmlFor={`${ids}-s${i}`}>{t("schedule.start")}</label>
                <input
                  id={`${ids}-s${i}`}
                  type="time"
                  value={w.start}
                  onChange={(e) => setWindow(i, { ...w, start: e.target.value })}
                />
                <label htmlFor={`${ids}-e${i}`}>{t("schedule.end")}</label>
                <input
                  id={`${ids}-e${i}`}
                  type="time"
                  value={w.end}
                  onChange={(e) => setWindow(i, { ...w, end: e.target.value })}
                />
                {windows.length > 1 && (
                  <Button onClick={() => setWindows(windows.filter((_, j) => j !== i))}>
                    {t("schedule.removeWindow").replace("{n}", String(i + 1))}
                  </Button>
                )}
              </div>
            </fieldset>
          </li>
        ))}
      </ol>
      <p className="queues__note">{t("schedule.overnight")}</p>
      {windows.length < MAX_WINDOWS && (
        <div className="queues__field">
          <Button onClick={() => setWindows([...windows, blankWindow()])}>
            {t("schedule.addWindow")}
          </Button>
        </div>
      )}
      <div className="queues__field">
        <label htmlFor={`${ids}-missed`}>{t("schedule.missed")}</label>
        <select
          id={`${ids}-missed`}
          value={draft.missed_window ?? "skip"}
          aria-describedby={`${ids}-missed-hint`}
          onChange={(e) =>
            setDraft({ ...draft, missed_window: e.target.value as "skip" | "catch_up" })
          }
        >
          <option value="skip">{t("schedule.missed.skip")}</option>
          <option value="catch_up">{t("schedule.missed.catchUp")}</option>
        </select>
        <span id={`${ids}-missed-hint`} className="queues__note">
          {t("schedule.missedHint")}
        </span>
      </div>
      <p className="queues__note">{t("schedule.asleep")}</p>
      <p className="queues__note">{t("schedule.policy")}</p>
      {problem && (
        <p role="alert" className="queues__problem">
          {t(`schedule.problem.${problem}`)}
        </p>
      )}
      <div className="queues__field">
        <Button onClick={save} {...busyReason}>
          {t("schedule.save")}
        </Button>
        {saved && (
          <Button
            onClick={() => void run(() => actions.setQueueSchedule(queue.queue_id, null))}
            {...busyReason}
          >
            {t("schedule.remove")}
          </Button>
        )}
      </div>
    </fieldset>
  );
}
