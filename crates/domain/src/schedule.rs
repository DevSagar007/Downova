//! Queue schedules (CORE-08, ADR 0025): pure evaluation of recurring local-time windows.
//!
//! No I/O: the caller resolves the IANA zone (`jiff::tz::TimeZone`) and passes the instant, so
//! every rule is deterministic under test.
//!
//! - A window is `days` × `start`–`end` in the schedule's zone; `end < start` crosses midnight and
//!   belongs to the day it starts on. `start == end` is refused.
//! - Windows are half-open: in at `start`, out at `end`. Overlapping or touching windows merge.
//! - **DST gap** (a local time that does not exist): a boundary inside it moves forward to the
//!   first instant after the gap (02:30 on a spring-forward night starts at 03:00).
//! - **DST fold** (a repeated hour): a start takes the earlier occurrence, an end the later one,
//!   so a window is never shorter than its wall-clock length.
//! - Missed windows (for `catch_up`): a window that started and ended while the service was not
//!   evaluating. Its identity is its start instant, stable across restarts.

use jiff::civil::{Date, DateTime, Time, Weekday as JWeekday};
use jiff::tz::{AmbiguousOffset, TimeZone};
use jiff::{SignedDuration, Timestamp, ToSpan};
use serde::{Deserialize, Serialize};

/// Most windows one schedule may hold.
pub const MAX_WINDOWS: usize = 14;
/// A catch-up window lasts at most this long (ADR 0025).
pub const CATCH_UP_CAP: SignedDuration = SignedDuration::from_hours(6);
/// Only a window that ended within this long before now can be caught up.
pub const CATCH_UP_HORIZON: SignedDuration = SignedDuration::from_hours(24);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Weekday {
    Mon,
    Tue,
    Wed,
    Thu,
    Fri,
    Sat,
    Sun,
}

impl Weekday {
    fn of(day: JWeekday) -> Self {
        match day {
            JWeekday::Monday => Self::Mon,
            JWeekday::Tuesday => Self::Tue,
            JWeekday::Wednesday => Self::Wed,
            JWeekday::Thursday => Self::Thu,
            JWeekday::Friday => Self::Fri,
            JWeekday::Saturday => Self::Sat,
            JWeekday::Sunday => Self::Sun,
        }
    }
}

/// One recurring window: on `days`, from `start` to `end` (local `HH:MM`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScheduleWindow {
    pub days: Vec<Weekday>,
    pub start: String,
    pub end: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissedWindowPolicy {
    /// Wait for the next window (default).
    #[default]
    Skip,
    /// One bounded catch-up window for the most recent missed one (ADR 0025 §5).
    CatchUp,
}

/// A queue's schedule (`queues.v1.set_schedule`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueueSchedule {
    pub enabled: bool,
    /// IANA zone name, e.g. `Asia/Dhaka`; resolved by the service.
    pub timezone: String,
    pub windows: Vec<ScheduleWindow>,
    #[serde(default)]
    pub missed_window: MissedWindowPolicy,
}

/// `HH:MM` on the minute grid.
pub fn parse_hhmm(s: &str) -> Option<Time> {
    let (h, m) = s.split_once(':')?;
    if h.len() != 2 || m.len() != 2 || !h.bytes().chain(m.bytes()).all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (h, m) = (h.parse::<i8>().ok()?, m.parse::<i8>().ok()?);
    Time::new(h, m, 0, 0).ok()
}

impl QueueSchedule {
    /// Shape and value checks (the zone name is resolved by the service).
    pub fn validate(&self) -> Result<(), &'static str> {
        let tz = self.timezone.trim();
        if tz.is_empty() || tz.len() > 64 || tz != self.timezone {
            return Err("timezone must be an IANA zone name such as Asia/Dhaka");
        }
        if self.windows.is_empty() || self.windows.len() > MAX_WINDOWS {
            return Err("a schedule has 1 to 14 windows");
        }
        for w in &self.windows {
            if w.days.is_empty() {
                return Err("every window needs at least one day");
            }
            let mut seen = std::collections::HashSet::new();
            if !w.days.iter().all(|d| seen.insert(*d)) {
                return Err("a window lists a day twice");
            }
            let (Some(a), Some(b)) = (parse_hhmm(&w.start), parse_hhmm(&w.end)) else {
                return Err("window times are HH:MM (00:00 to 23:59)");
            };
            if a == b {
                return Err("a window's start and end must differ");
            }
        }
        Ok(())
    }
}

/// The result of evaluating a schedule at one instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Evaluation {
    pub in_window: bool,
    /// When `in_window` next flips; `None` for a disabled schedule.
    pub next_change: Option<Timestamp>,
}

/// One concrete window occurrence, half-open `[start, end)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Interval {
    pub start: Timestamp,
    pub end: Timestamp,
}

#[derive(Clone, Copy)]
enum Edge {
    Start,
    End,
}

/// A local boundary as an instant, with the ADR's gap and fold rules.
fn instant(tz: &TimeZone, dt: DateTime, edge: Edge) -> Option<Timestamp> {
    match tz.to_ambiguous_timestamp(dt).offset() {
        AmbiguousOffset::Unambiguous { offset } => offset.to_timestamp(dt).ok(),
        AmbiguousOffset::Gap { after, .. } => {
            // Read with the post-gap offset the instant falls before the transition; the next
            // transition after it is the first instant that exists after the gap.
            let probe = after.to_timestamp(dt).ok()?;
            tz.following(probe).next().map(|t| t.timestamp())
        }
        AmbiguousOffset::Fold { before, after } => match edge {
            Edge::Start => before.to_timestamp(dt).ok(),
            Edge::End => after.to_timestamp(dt).ok(),
        },
    }
}

/// Every occurrence whose start date is within `[from - 1 day, from + days]`, merged.
pub fn intervals(
    schedule: &QueueSchedule,
    tz: &TimeZone,
    around: Timestamp,
    days: i64,
) -> Vec<Interval> {
    let today: Date = around.to_zoned(tz.clone()).date();
    let mut out = Vec::new();
    for offset in -1..=days {
        let Ok(day) = today.checked_add(offset.days()) else {
            continue;
        };
        let wd = Weekday::of(day.weekday());
        for w in &schedule.windows {
            if !w.days.contains(&wd) {
                continue;
            }
            let (Some(a), Some(b)) = (parse_hhmm(&w.start), parse_hhmm(&w.end)) else {
                continue;
            };
            let end_day = if b <= a {
                match day.checked_add(1.day()) {
                    Ok(d) => d,
                    Err(_) => continue,
                }
            } else {
                day
            };
            let (Some(start), Some(end)) = (
                instant(tz, day.to_datetime(a), Edge::Start),
                instant(tz, end_day.to_datetime(b), Edge::End),
            ) else {
                continue;
            };
            if start < end {
                out.push(Interval { start, end });
            }
        }
    }
    out.sort_by_key(|i| i.start);
    let mut merged: Vec<Interval> = Vec::new();
    for i in out {
        match merged.last_mut() {
            Some(last) if i.start <= last.end => last.end = last.end.max(i.end),
            _ => merged.push(i),
        }
    }
    merged
}

/// Whether `now` is inside a window and when that next changes.
pub fn evaluate(schedule: &QueueSchedule, now: Timestamp, tz: &TimeZone) -> Evaluation {
    if !schedule.enabled {
        return Evaluation {
            in_window: true,
            next_change: None,
        };
    }
    let all = intervals(schedule, tz, now, 8);
    if let Some(cur) = all.iter().find(|i| i.start <= now && now < i.end) {
        return Evaluation {
            in_window: true,
            next_change: Some(cur.end),
        };
    }
    Evaluation {
        in_window: false,
        next_change: all.iter().map(|i| i.start).find(|s| *s > now),
    }
}

/// The most recent window that started at or after `last_evaluated` and ended at or before
/// `now`, ended within [`CATCH_UP_HORIZON`] and was not already caught up (`already` = the start
/// of the last caught-up window). Its start instant is its identity.
pub fn last_missed_window(
    schedule: &QueueSchedule,
    tz: &TimeZone,
    last_evaluated: Timestamp,
    now: Timestamp,
    already: Option<Timestamp>,
) -> Option<Interval> {
    if !schedule.enabled
        || schedule.missed_window != MissedWindowPolicy::CatchUp
        || last_evaluated >= now
    {
        return None;
    }
    let horizon = now.checked_sub(CATCH_UP_HORIZON).ok()?;
    let look_back = (now.duration_since(last_evaluated).as_secs() / 86_400).saturating_add(2);
    let from = last_evaluated
        .checked_sub(SignedDuration::from_hours(24))
        .ok()?;
    intervals(schedule, tz, from, look_back)
        .into_iter()
        .filter(|i| i.start >= last_evaluated && i.end <= now && i.end >= horizon)
        .filter(|i| already.is_none_or(|a| i.start > a))
        .max_by_key(|i| i.start)
}

/// The catch-up window opened at `now` for `missed`: `[now, now + min(length, cap))`.
pub fn catch_up_until(missed: Interval, now: Timestamp) -> Option<Timestamp> {
    let length = missed.end.duration_since(missed.start).min(CATCH_UP_CAP);
    now.checked_add(length).ok()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn tz(name: &str) -> TimeZone {
        TimeZone::get(name).unwrap()
    }
    fn at(name: &str, local: &str) -> Timestamp {
        local
            .parse::<DateTime>()
            .unwrap()
            .to_zoned(tz(name))
            .unwrap()
            .timestamp()
    }
    fn win(days: &[Weekday], start: &str, end: &str) -> ScheduleWindow {
        ScheduleWindow {
            days: days.to_vec(),
            start: start.into(),
            end: end.into(),
        }
    }
    fn sched(zone: &str, windows: Vec<ScheduleWindow>) -> QueueSchedule {
        QueueSchedule {
            enabled: true,
            timezone: zone.into(),
            windows,
            missed_window: MissedWindowPolicy::Skip,
        }
    }
    use Weekday::*;
    const ALL: [Weekday; 7] = [Mon, Tue, Wed, Thu, Fri, Sat, Sun];

    #[test]
    fn validation_refuses_malformed_schedules() {
        assert!(
            sched("Asia/Dhaka", vec![win(&[Mon], "01:00", "07:00")])
                .validate()
                .is_ok()
        );
        for bad in [
            sched("", vec![win(&[Mon], "01:00", "07:00")]),
            sched("Asia/Dhaka", vec![]),
            sched("Asia/Dhaka", vec![win(&[], "01:00", "07:00")]),
            sched("Asia/Dhaka", vec![win(&[Mon, Mon], "01:00", "07:00")]),
            sched("Asia/Dhaka", vec![win(&[Mon], "1:00", "07:00")]),
            sched("Asia/Dhaka", vec![win(&[Mon], "24:00", "07:00")]),
            sched("Asia/Dhaka", vec![win(&[Mon], "07:00", "07:00")]),
            sched("Asia/Dhaka", vec![win(&[Mon], "01:00", "07:00"); 15]),
        ] {
            assert!(bad.validate().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn boundaries_are_inclusive_at_start_and_exclusive_at_end() {
        // 2026-11-02 is a Monday.
        let s = sched("Asia/Dhaka", vec![win(&[Mon], "01:00", "07:00")]);
        let z = tz("Asia/Dhaka");
        let e = |t: &str| evaluate(&s, at("Asia/Dhaka", t), &z);
        assert!(!e("2026-11-02T00:59:59").in_window);
        assert!(e("2026-11-02T01:00:00").in_window);
        assert!(e("2026-11-02T06:59:59").in_window);
        assert!(!e("2026-11-02T07:00:00").in_window);
        assert_eq!(
            e("2026-11-02T03:00:00").next_change,
            Some(at("Asia/Dhaka", "2026-11-02T07:00:00"))
        );
        // After Monday's window: next Monday.
        assert_eq!(
            e("2026-11-02T08:00:00").next_change,
            Some(at("Asia/Dhaka", "2026-11-09T01:00:00"))
        );
    }

    #[test]
    fn overnight_windows_belong_to_their_start_day_and_overlaps_merge() {
        let s = sched(
            "Asia/Dhaka",
            vec![win(&[Sat], "22:00", "08:00"), win(&[Sun], "07:00", "09:00")],
        );
        let z = tz("Asia/Dhaka");
        // 2026-11-07 is a Saturday.
        let e = |t: &str| evaluate(&s, at("Asia/Dhaka", t), &z);
        assert!(e("2026-11-07T23:00:00").in_window);
        assert!(e("2026-11-08T07:30:00").in_window);
        // Merged: Saturday 22:00 to Sunday 09:00 is one window, no flip at 08:00.
        assert_eq!(
            e("2026-11-07T23:00:00").next_change,
            Some(at("Asia/Dhaka", "2026-11-08T09:00:00"))
        );
        assert!(!e("2026-11-08T09:00:00").in_window);
        // Friday night is not Saturday's window.
        assert!(!e("2026-11-06T23:00:00").in_window);
    }

    #[test]
    fn a_dst_gap_moves_a_boundary_to_the_first_instant_after_it() {
        // New York springs forward on 2027-03-14: 02:00 → 03:00 local. A 02:30 start begins at 03:00.
        let s = sched("America/New_York", vec![win(&ALL, "02:30", "05:00")]);
        let z = tz("America/New_York");
        let starts = intervals(&s, &z, at("America/New_York", "2027-03-14T00:00:00"), 0);
        let gap_day = starts
            .iter()
            .find(|i| i.end == at("America/New_York", "2027-03-14T05:00:00"))
            .unwrap();
        assert_eq!(gap_day.start, at("America/New_York", "2027-03-14T03:00:00"));
        assert_eq!(
            gap_day.end.duration_since(gap_day.start),
            SignedDuration::from_hours(2)
        );
    }

    #[test]
    fn a_dst_fold_keeps_the_full_wall_clock_length() {
        // London falls back on 2026-10-25: 01:00–02:00 local happens twice. A 01:30–01:45 window
        // starts at the first 01:30 and ends at the second 01:45 (75 minutes).
        let s = sched("Europe/London", vec![win(&ALL, "01:30", "01:45")]);
        let z = tz("Europe/London");
        let found = intervals(&s, &z, "2026-10-25T00:00:00Z".parse().unwrap(), 0);
        let fold_day = found
            .iter()
            .find(|i| i.start.to_zoned(z.clone()).date().to_string() == "2026-10-25")
            .unwrap();
        assert_eq!(
            fold_day.end.duration_since(fold_day.start),
            SignedDuration::from_mins(75)
        );
        // New York's fold (2026-11-01): the same rule.
        let s = sched("America/New_York", vec![win(&ALL, "00:30", "01:30")]);
        let z = tz("America/New_York");
        let found = intervals(&s, &z, "2026-11-01T12:00:00Z".parse().unwrap(), 0);
        let fold_day = found
            .iter()
            .find(|i| i.start.to_zoned(z.clone()).date().to_string() == "2026-11-01")
            .unwrap();
        assert_eq!(
            fold_day.end.duration_since(fold_day.start),
            SignedDuration::from_hours(2)
        );
    }

    #[test]
    fn a_zone_without_dst_and_the_week_end_rollover() {
        let s = sched("Asia/Dhaka", vec![win(&[Sun], "23:00", "01:00")]);
        let z = tz("Asia/Dhaka");
        // Sunday 2026-11-08 23:30 is in; it ends Monday 01:00.
        let e = evaluate(&s, at("Asia/Dhaka", "2026-11-08T23:30:00"), &z);
        assert!(e.in_window);
        assert_eq!(e.next_change, Some(at("Asia/Dhaka", "2026-11-09T01:00:00")));
        // Monday 00:30 is still the Sunday window.
        assert!(evaluate(&s, at("Asia/Dhaka", "2026-11-09T00:30:00"), &z).in_window);
    }

    #[test]
    fn a_disabled_schedule_never_holds_anything() {
        let mut s = sched("Asia/Dhaka", vec![win(&[Mon], "01:00", "02:00")]);
        s.enabled = false;
        let e = evaluate(
            &s,
            at("Asia/Dhaka", "2026-11-03T12:00:00"),
            &tz("Asia/Dhaka"),
        );
        assert_eq!(
            e,
            Evaluation {
                in_window: true,
                next_change: None
            }
        );
    }

    #[test]
    fn catch_up_takes_only_the_latest_fully_missed_window_once_and_bounded() {
        let mut s = sched("Asia/Dhaka", vec![win(&ALL, "01:00", "09:00")]);
        let z = tz("Asia/Dhaka");
        let last = at("Asia/Dhaka", "2026-11-02T00:00:00");
        let now = at("Asia/Dhaka", "2026-11-03T12:00:00");
        // Skip: never.
        assert_eq!(last_missed_window(&s, &z, last, now, None), None);
        s.missed_window = MissedWindowPolicy::CatchUp;
        // Both 2026-11-02 and 2026-11-03 windows were missed; only the latest counts.
        let missed = last_missed_window(&s, &z, last, now, None).unwrap();
        assert_eq!(missed.start, at("Asia/Dhaka", "2026-11-03T01:00:00"));
        // 8 h window, capped at 6 h from now.
        assert_eq!(
            catch_up_until(missed, now),
            Some(at("Asia/Dhaka", "2026-11-03T18:00:00"))
        );
        // Already caught up: not again (a restart must not repeat it).
        assert_eq!(
            last_missed_window(&s, &z, last, now, Some(missed.start)),
            None
        );
        // A window that ended more than 24 h ago is too old: a weekly Monday window checked on
        // Tuesday at noon ended 27 h earlier.
        let mut weekly = sched("Asia/Dhaka", vec![win(&[Mon], "01:00", "09:00")]);
        weekly.missed_window = MissedWindowPolicy::CatchUp;
        assert_eq!(last_missed_window(&weekly, &z, last, now, None), None);
        // …while the same window checked within 24 h of its end is caught up.
        let soon = at("Asia/Dhaka", "2026-11-02T20:00:00");
        assert_eq!(
            last_missed_window(&weekly, &z, last, soon, None).map(|i| i.start),
            Some(at("Asia/Dhaka", "2026-11-02T01:00:00"))
        );
        // A window the service was running through is not missed.
        let during = at("Asia/Dhaka", "2026-11-03T02:00:00");
        assert_eq!(last_missed_window(&s, &z, during, now, None), None);
        // A clock that went backwards (last evaluation in the future) finds nothing.
        assert_eq!(last_missed_window(&s, &z, now, last, None), None);
    }
}
