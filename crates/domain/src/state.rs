//! Job state and the run-eligibility policy.
//!
//! Three independent dimensions (research §17.4):
//! * [`DesiredState`] — what the user asked for. Only user commands change it.
//! * [`Phase`] — where the job is in its lifecycle.
//! * [`Blocker`] — automatic policy conditions (network, schedule, disk, …).
//!
//! A manual pause is therefore never cleared by a policy change such as a network reconnect:
//! clearing blockers cannot change `DesiredState::Paused`.

use serde::{Deserialize, Serialize};

/// What the user asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesiredState {
    Running,
    Paused,
    Cancelled,
}

/// Lifecycle phase. `Completed` is reached only after verification and finalization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Queued,
    Probing,
    Transferring,
    Verifying,
    Scanning,
    Finalizing,
    Completed,
    Failed,
    Cancelled,
}

impl Phase {
    /// Terminal phases never transition again within the same job.
    pub fn is_terminal(self) -> bool {
        matches!(self, Phase::Completed | Phase::Failed | Phase::Cancelled)
    }
}

/// Automatic policy condition holding a job that the user wants running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Blocker {
    Offline,
    MeteredNetwork,
    ScheduleWindow,
    RateLimitCooldown,
    DiskUnavailable,
    VolumeMissing,
    QueueLimit,
    /// The job's queue was stopped (B2.11, ADR 0017). Starting the queue clears it; it never
    /// overrides a user pause, a disk hold or a retry cool-down.
    QueueStopped,
    /// The final name would collide and the collision policy is "ask" (B2.8, ADR 0015): the
    /// verified file waits before placement until the user confirms the free name or cancels.
    NameConflict,
}

impl Blocker {
    /// Lower rank = higher precedence (research §17.1): disk → network/schedule → queue.
    fn rank(self) -> u8 {
        match self {
            Blocker::DiskUnavailable | Blocker::VolumeMissing => 0,
            Blocker::Offline
            | Blocker::MeteredNetwork
            | Blocker::ScheduleWindow
            | Blocker::RateLimitCooldown
            | Blocker::QueueStopped
            | Blocker::NameConflict => 1,
            Blocker::QueueLimit => 2,
        }
    }
}

/// Why a job that is not terminal is not running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoldReason {
    /// The user paused it. Only a user resume releases it.
    UserPaused,
    /// An automatic policy holds it; it becomes eligible again when the blocker clears.
    Policy(Blocker),
}

/// Outcome of the run-eligibility policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunDecision {
    /// The user cancelled; the job must stop and never complete.
    Stop,
    /// The job must not run now.
    Hold(HoldReason),
    /// The job may be started or continued.
    Eligible,
    /// The job is in a terminal phase; nothing to schedule.
    Terminal,
}

/// Decides whether a job may run.
///
/// Precedence: terminal phase → user cancel → user pause → highest-precedence policy blocker →
/// eligible. `blockers` may be in any order and may contain duplicates.
pub fn decide_run(desired: DesiredState, phase: Phase, blockers: &[Blocker]) -> RunDecision {
    if phase.is_terminal() {
        return RunDecision::Terminal;
    }
    match desired {
        DesiredState::Cancelled => RunDecision::Stop,
        DesiredState::Paused => RunDecision::Hold(HoldReason::UserPaused),
        DesiredState::Running => blockers
            .iter()
            .copied()
            .min_by_key(|b| (b.rank(), *b))
            .map_or(RunDecision::Eligible, |b| {
                RunDecision::Hold(HoldReason::Policy(b))
            }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_pause_survives_network_reconnect() {
        // Offline while paused, then the network returns (blocker cleared): still user-paused.
        let offline = decide_run(
            DesiredState::Paused,
            Phase::Transferring,
            &[Blocker::Offline],
        );
        let reconnected = decide_run(DesiredState::Paused, Phase::Transferring, &[]);
        assert_eq!(offline, RunDecision::Hold(HoldReason::UserPaused));
        assert_eq!(reconnected, RunDecision::Hold(HoldReason::UserPaused));
    }

    #[test]
    fn policy_hold_releases_when_blocker_clears() {
        let held = decide_run(
            DesiredState::Running,
            Phase::Queued,
            &[Blocker::MeteredNetwork],
        );
        assert_eq!(
            held,
            RunDecision::Hold(HoldReason::Policy(Blocker::MeteredNetwork))
        );
        assert_eq!(
            decide_run(DesiredState::Running, Phase::Queued, &[]),
            RunDecision::Eligible
        );
    }

    #[test]
    fn disk_blocker_outranks_network_and_queue() {
        let blockers = [
            Blocker::QueueLimit,
            Blocker::Offline,
            Blocker::VolumeMissing,
        ];
        assert_eq!(
            decide_run(DesiredState::Running, Phase::Transferring, &blockers),
            RunDecision::Hold(HoldReason::Policy(Blocker::VolumeMissing))
        );
    }

    #[test]
    fn cancel_wins_over_everything_and_terminal_is_final() {
        assert_eq!(
            decide_run(
                DesiredState::Cancelled,
                Phase::Transferring,
                &[Blocker::Offline]
            ),
            RunDecision::Stop
        );
        // A late event cannot revive a cancelled job: the phase is terminal.
        assert_eq!(
            decide_run(DesiredState::Running, Phase::Cancelled, &[]),
            RunDecision::Terminal
        );
    }
}
