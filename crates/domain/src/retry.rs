//! Automatic retry policy (COR-04), owned by the service, not the engine.
//!
//! - Only transient kinds are retried: `NETWORK_OFFLINE` and `SERVER_BUSY` (429, 503, 5xx).
//!   Authentication failures, link refresh, remote change and capability refusals never are.
//! - A server-supplied `Retry-After` is honoured exactly (never earlier) when it is within
//!   [`RetryPolicy::max_retry_after`]; a longer one is not waited for automatically.
//! - Otherwise the delay is bounded exponential backoff with "equal jitter": half the backoff
//!   is fixed, the other half is random, so clients do not retry in lockstep.
//!
//! Pure: the caller supplies the jitter sample and, for HTTP dates, the current time.

use std::time::Duration;

use crate::ErrorKind;

/// Retry limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Automatic retries after the first attempt.
    pub max_retries: u32,
    /// Backoff for the first retry.
    pub base: Duration,
    /// Upper bound for computed backoff.
    pub cap: Duration,
    /// Longest server-requested wait honoured automatically.
    pub max_retry_after: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base: Duration::from_secs(2),
            cap: Duration::from_secs(60),
            max_retry_after: Duration::from_secs(15 * 60),
        }
    }
}

/// What to do after a failed attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryDecision {
    /// Start a new attempt after this delay.
    RetryAfter(Duration),
    /// Leave the job failed.
    GiveUp,
}

/// Whether a failure of this kind is retried automatically at all.
pub fn is_auto_retryable(kind: ErrorKind) -> bool {
    matches!(kind, ErrorKind::NetworkOffline | ErrorKind::ServerBusy)
}

/// Decides the next step after attempt failure number `retries_done + 1`.
///
/// `jitter` is a uniform sample in `0..=1000` (per mille of the random half).
pub fn decide_retry(
    policy: &RetryPolicy,
    kind: ErrorKind,
    retries_done: u32,
    retry_after: Option<Duration>,
    jitter: u16,
) -> RetryDecision {
    if !is_auto_retryable(kind) || retries_done >= policy.max_retries {
        return RetryDecision::GiveUp;
    }
    if let Some(wait) = retry_after {
        return if wait <= policy.max_retry_after {
            RetryDecision::RetryAfter(wait)
        } else {
            RetryDecision::GiveUp
        };
    }
    let factor = 1u32.checked_shl(retries_done).unwrap_or(u32::MAX);
    let backoff = policy
        .base
        .checked_mul(factor)
        .map_or(policy.cap, |b| b.min(policy.cap));
    let half = backoff.checked_div(2).unwrap_or_default();
    let random = half
        .checked_mul(u32::from(jitter.min(1000)))
        .and_then(|d| d.checked_div(1000))
        .unwrap_or(half);
    RetryDecision::RetryAfter(half.saturating_add(random))
}

/// Parses a `Retry-After` field value (RFC 9110 §10.2.3): delta-seconds or an IMF-fixdate
/// (`Sun, 06 Nov 1994 08:49:37 GMT`). `now_unix` is the current time in Unix seconds. A date in
/// the past yields zero. Anything else is `None` (the caller falls back to backoff).
pub fn parse_retry_after(value: &str, now_unix: u64) -> Option<Duration> {
    let value = value.trim();
    if !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) {
        return value.parse::<u64>().ok().map(Duration::from_secs);
    }
    let at = parse_imf_fixdate(value)?;
    Some(Duration::from_secs(at.saturating_sub(now_unix)))
}

fn parse_imf_fixdate(value: &str) -> Option<u64> {
    // "Sun, 06 Nov 1994 08:49:37 GMT"
    let mut parts = value.split_whitespace();
    let weekday = parts.next()?;
    if weekday.len() != 4 || !weekday.ends_with(',') {
        return None;
    }
    let day: u64 = parts.next().filter(|d| d.len() == 2)?.parse().ok()?;
    let month = match parts.next()? {
        "Jan" => 1,
        "Feb" => 2,
        "Mar" => 3,
        "Apr" => 4,
        "May" => 5,
        "Jun" => 6,
        "Jul" => 7,
        "Aug" => 8,
        "Sep" => 9,
        "Oct" => 10,
        "Nov" => 11,
        "Dec" => 12,
        _ => return None,
    };
    let year: i64 = parts.next().filter(|y| y.len() == 4)?.parse().ok()?;
    let mut hms = parts.next()?.split(':');
    let mut field = |max: u64| -> Option<u64> {
        let v: u64 = hms.next().filter(|f| f.len() == 2)?.parse().ok()?;
        (v <= max).then_some(v)
    };
    let (h, m, s) = (field(23)?, field(59)?, field(60)?);
    if parts.next()? != "GMT" || parts.next().is_some() || !(1..=31).contains(&day) {
        return None;
    }
    let days = days_from_civil(year, month, i64::try_from(day).ok()?)?;
    let secs = days.checked_mul(86_400)?.checked_add(
        i64::try_from(
            h.checked_mul(3600)?
                .checked_add(m.checked_mul(60)?)?
                .checked_add(s)?,
        )
        .ok()?,
    )?;
    u64::try_from(secs).ok()
}

/// Days since 1970-01-01 for a proleptic Gregorian date (H. Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> Option<i64> {
    let y = if month <= 2 {
        year.checked_sub(1)?
    } else {
        year
    };
    let era = y.div_euclid(400);
    let yoe = y.checked_sub(era.checked_mul(400)?)?;
    let mp = month.checked_add(9)?.checked_rem(12)?;
    let doy = mp
        .checked_mul(153)?
        .checked_add(2)?
        .checked_div(5)?
        .checked_add(day)?
        .checked_sub(1)?;
    let doe = yoe
        .checked_mul(365)?
        .checked_add(yoe.checked_div(4)?)?
        .checked_sub(yoe.checked_div(100)?)?
        .checked_add(doy)?;
    era.checked_mul(146_097)?
        .checked_add(doe)?
        .checked_sub(719_468)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(d: RetryDecision) -> u64 {
        match d {
            RetryDecision::RetryAfter(d) => d.as_secs(),
            RetryDecision::GiveUp => u64::MAX,
        }
    }

    #[test]
    fn only_transient_kinds_are_retried() {
        let p = RetryPolicy::default();
        for kind in [
            ErrorKind::AuthRequired,
            ErrorKind::LinkRefreshRequired,
            ErrorKind::RemoteChanged,
            ErrorKind::BackendCapabilityUnavailable,
            ErrorKind::ServiceUnavailable,
            ErrorKind::TlsHandshakeFailed,
        ] {
            assert_eq!(
                decide_retry(&p, kind, 0, None, 500),
                RetryDecision::GiveUp,
                "{kind:?}"
            );
        }
        assert_ne!(
            decide_retry(&p, ErrorKind::ServerBusy, 0, None, 500),
            RetryDecision::GiveUp
        );
        assert_ne!(
            decide_retry(&p, ErrorKind::NetworkOffline, 0, None, 500),
            RetryDecision::GiveUp
        );
    }

    #[test]
    fn retries_are_bounded() {
        let p = RetryPolicy::default();
        assert_ne!(
            decide_retry(&p, ErrorKind::ServerBusy, 2, None, 0),
            RetryDecision::GiveUp
        );
        assert_eq!(
            decide_retry(&p, ErrorKind::ServerBusy, 3, None, 0),
            RetryDecision::GiveUp
        );
    }

    #[test]
    fn retry_after_is_honoured_exactly_and_capped() {
        let p = RetryPolicy::default();
        let thirty = Some(Duration::from_secs(30));
        assert_eq!(
            secs(decide_retry(&p, ErrorKind::ServerBusy, 0, thirty, 0)),
            30
        );
        assert_eq!(
            secs(decide_retry(&p, ErrorKind::ServerBusy, 0, thirty, 1000)),
            30
        );
        let day = Some(Duration::from_secs(86_400));
        assert_eq!(
            decide_retry(&p, ErrorKind::ServerBusy, 0, day, 0),
            RetryDecision::GiveUp
        );
    }

    #[test]
    fn backoff_grows_exponentially_with_equal_jitter_and_a_cap() {
        let p = RetryPolicy::default();
        // base 2 s: retry 0 → [1, 2] s, retry 1 → [2, 4] s, retry 2 → [4, 8] s.
        assert_eq!(
            secs(decide_retry(&p, ErrorKind::NetworkOffline, 0, None, 0)),
            1
        );
        assert_eq!(
            secs(decide_retry(&p, ErrorKind::NetworkOffline, 0, None, 1000)),
            2
        );
        assert_eq!(
            secs(decide_retry(&p, ErrorKind::NetworkOffline, 2, None, 0)),
            4
        );
        assert_eq!(
            secs(decide_retry(&p, ErrorKind::NetworkOffline, 2, None, 1000)),
            8
        );
        let big = RetryPolicy {
            max_retries: 100,
            ..p
        };
        assert_eq!(
            secs(decide_retry(
                &big,
                ErrorKind::NetworkOffline,
                40,
                None,
                1000
            )),
            60
        );
        assert_eq!(
            secs(decide_retry(&big, ErrorKind::NetworkOffline, 40, None, 0)),
            30
        );
    }

    #[test]
    fn retry_after_values_parse() {
        let now = 784_111_777; // Sun, 06 Nov 1994 08:49:37 GMT
        assert_eq!(
            parse_retry_after("120", now),
            Some(Duration::from_secs(120))
        );
        assert_eq!(parse_retry_after(" 0 ", now), Some(Duration::ZERO));
        assert_eq!(
            parse_retry_after("Sun, 06 Nov 1994 08:51:37 GMT", now),
            Some(Duration::from_secs(120))
        );
        assert_eq!(
            parse_retry_after("Sun, 06 Nov 1994 08:00:00 GMT", now),
            Some(Duration::ZERO),
            "a past date means now"
        );
        for bad in [
            "",
            "-5",
            "1.5",
            "soon",
            "Sunday, 06-Nov-94 08:49:37 GMT",
            "Sun, 06 Nov 1994 25:00:00 GMT",
        ] {
            assert_eq!(parse_retry_after(bad, now), None, "{bad}");
        }
    }
}
