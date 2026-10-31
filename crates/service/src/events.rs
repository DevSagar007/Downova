//! Service event stream (B2.9, ARCH-08, ADR 0016).
//!
//! The hub lives inside the coordinator's state, so subscribing and publishing are serialized by
//! the same lock as every job change:
//! - **Race-free start.** [`EventHub::subscribe`] returns the current list together with the
//!   sequence it reflects; every later event the subscriber receives has exactly the next
//!   sequence. No change can fall between the snapshot and the first event.
//! - **Diff-based, coalesced.** [`EventHub::publish`] compares each listed job's snapshot with
//!   the last one published: a new job is `job_added`, a change of anything but the byte count
//!   is `job_changed` (sent at once, since mutations publish when they finish), a byte count
//!   alone is `progress` (the poll loop publishes every 400 ms, inside the 250–500 ms target), and
//!   a job leaving the list (removed) is `job_removed`. Unchanged jobs produce nothing, so normal
//!   coalescing never skips a sequence number.
//! - **One sequence for everyone.** Sequences are assigned once per event, before delivery, and
//!   every subscriber receives every event after its watermark.
//! - **Bounded, never blocking.** Each subscriber has a buffer of [`SUBSCRIBER_BUFFER`] events.
//!   Delivery is `try_send`: a full buffer drops that subscriber (marked overflowed) and its
//!   connection then sends `snapshot_required` and closes; a closed connection is dropped. A slow
//!   or vanished client can never delay a transfer or a mutation.
//! - **Precision.** Sequences and byte counts travel as decimal strings ([`ByteCount`]).

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use dm_domain::api::{DownloadSnapshot, EventKind, ServiceEvent, SubscribeResult};
use dm_domain::{ByteCount, JobId};
use tokio::sync::mpsc;

/// Events buffered per subscriber before it is dropped as too slow.
pub const SUBSCRIBER_BUFFER: usize = 1024;

#[derive(Debug)]
struct Subscriber {
    tx: mpsc::Sender<ServiceEvent>,
    overflowed: Arc<AtomicBool>,
}

/// What a subscriber gets: the authoritative start, its event channel and its overflow flag.
#[derive(Debug)]
pub struct Subscription {
    pub start: SubscribeResult,
    pub events: mpsc::Receiver<ServiceEvent>,
    /// Set when the hub dropped this subscriber because its buffer was full.
    pub overflowed: Arc<AtomicBool>,
}

#[derive(Debug)]
pub struct EventHub {
    instance_id: String,
    sequence: u64,
    last: HashMap<JobId, DownloadSnapshot>,
    /// Acceptance order of the published jobs (so events for several jobs keep list order).
    order: Vec<JobId>,
    subscribers: Vec<Subscriber>,
}

/// Everything but the byte count: a difference here is a semantic change.
fn semantic(s: &DownloadSnapshot) -> DownloadSnapshot {
    DownloadSnapshot {
        downloaded_bytes: ByteCount(0),
        ..s.clone()
    }
}

impl EventHub {
    pub fn new(instance_id: &str) -> Self {
        Self {
            instance_id: instance_id.to_owned(),
            sequence: 0,
            last: HashMap::new(),
            order: Vec::new(),
            subscribers: Vec::new(),
        }
    }

    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    pub fn subscribers(&self) -> usize {
        self.subscribers.len()
    }

    /// Registers a subscriber at the current sequence. `listed` must be the current list (the
    /// caller holds the coordinator lock), so it is consistent with the watermark.
    pub fn subscribe(&mut self, listed: Vec<DownloadSnapshot>) -> Subscription {
        // Bring the hub's view up to date first, so the next published event follows this list.
        self.publish(listed.iter().cloned());
        let (tx, rx) = mpsc::channel(SUBSCRIBER_BUFFER);
        let overflowed = Arc::new(AtomicBool::new(false));
        self.subscribers.push(Subscriber {
            tx,
            overflowed: Arc::clone(&overflowed),
        });
        Subscription {
            start: SubscribeResult {
                service_instance_id: self.instance_id.clone(),
                sequence: ByteCount(self.sequence),
                items: listed,
            },
            events: rx,
            overflowed,
        }
    }

    /// Publishes the differences between `listed` (every job in the list, removed ones excluded)
    /// and the last published state.
    pub fn publish(&mut self, listed: impl Iterator<Item = DownloadSnapshot>) {
        let mut seen: HashMap<JobId, DownloadSnapshot> = HashMap::new();
        let mut events = Vec::new();
        for snap in listed {
            let kind = match self.last.get(&snap.job_id) {
                None => Some(EventKind::JobAdded),
                Some(prev) if semantic(prev) != semantic(&snap) => Some(EventKind::JobChanged),
                Some(prev) if prev.downloaded_bytes != snap.downloaded_bytes => {
                    Some(EventKind::Progress)
                }
                Some(_) => None,
            };
            if let Some(kind) = kind {
                events.push((kind, snap.job_id.clone(), Some(snap.clone())));
            }
            seen.insert(snap.job_id.clone(), snap);
        }
        for id in &self.order {
            if !seen.contains_key(id) {
                events.push((EventKind::JobRemoved, id.clone(), None));
            }
        }
        // Keep acceptance order: existing jobs in their order, new ones appended.
        let mut order: Vec<JobId> = self
            .order
            .iter()
            .filter(|id| seen.contains_key(*id))
            .cloned()
            .collect();
        for (_, id, _) in &events {
            if seen.contains_key(id) && !order.contains(id) {
                order.push(id.clone());
            }
        }
        self.order = order;
        self.last = seen;
        for (kind, job_id, snapshot) in events {
            self.sequence = self.sequence.saturating_add(1);
            let event = ServiceEvent {
                service_instance_id: self.instance_id.clone(),
                sequence: ByteCount(self.sequence),
                kind,
                attempt_id: snapshot.as_ref().and_then(|s| s.attempt_id.clone()),
                job_id: Some(job_id),
                snapshot,
            };
            self.deliver(&event);
        }
    }

    fn deliver(&mut self, event: &ServiceEvent) {
        self.subscribers
            .retain(|s| match s.tx.try_send(event.clone()) {
                Ok(()) => true,
                Err(mpsc::error::TrySendError::Full(_)) => {
                    s.overflowed.store(true, Ordering::SeqCst);
                    false
                }
                Err(mpsc::error::TrySendError::Closed(_)) => false,
            });
    }

    /// The event a dropped (overflowed) subscriber receives last, before its stream closes.
    pub fn snapshot_required(instance_id: &str, after: u64) -> ServiceEvent {
        ServiceEvent {
            service_instance_id: instance_id.to_owned(),
            sequence: ByteCount(after.saturating_add(1)),
            kind: EventKind::SnapshotRequired,
            job_id: None,
            attempt_id: None,
            snapshot: None,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use dm_domain::{DesiredState, Phase};

    fn snap(id: &str, bytes: u64, phase: Phase) -> DownloadSnapshot {
        DownloadSnapshot {
            job_id: JobId::parse(id).unwrap(),
            attempt_id: None,
            display_name: "f".into(),
            source_display: "https://h/f".into(),
            desired_state: DesiredState::Running,
            phase,
            blockers: vec![],
            downloaded_bytes: ByteCount(bytes),
            total_bytes: None,
            error: None,
            verification: None,
            removed: false,
            queue_id: None,
            priority: None,
            queue_position: None,
            category: None,
        }
    }

    fn drain(rx: &mut mpsc::Receiver<ServiceEvent>) -> Vec<(u64, EventKind)> {
        let mut out = Vec::new();
        while let Ok(e) = rx.try_recv() {
            out.push((e.sequence.0, e.kind));
        }
        out
    }

    #[test]
    fn a_subscriber_sees_every_change_after_its_watermark_without_gaps() {
        let mut hub = EventHub::new("svc_t");
        hub.publish([snap("job_a", 0, Phase::Queued)].into_iter());
        let mut sub = hub.subscribe(vec![snap("job_a", 0, Phase::Queued)]);
        assert_eq!(sub.start.sequence.0, 1);
        // Unchanged: nothing, no sequence consumed.
        hub.publish([snap("job_a", 0, Phase::Queued)].into_iter());
        hub.publish([snap("job_a", 10, Phase::Queued)].into_iter());
        hub.publish(
            [
                snap("job_a", 10, Phase::Transferring),
                snap("job_b", 0, Phase::Queued),
            ]
            .into_iter(),
        );
        hub.publish([snap("job_b", 0, Phase::Queued)].into_iter());
        assert_eq!(
            drain(&mut sub.events),
            vec![
                (2, EventKind::Progress),
                (3, EventKind::JobChanged),
                (4, EventKind::JobAdded),
                (5, EventKind::JobRemoved),
            ]
        );
    }

    #[test]
    fn subscribing_reflects_changes_made_before_it_and_starts_after_them() {
        let mut hub = EventHub::new("svc_t");
        hub.publish([snap("job_a", 0, Phase::Queued)].into_iter());
        // A change not yet published when a client subscribes is part of its list, not an event.
        let mut sub = hub.subscribe(vec![snap("job_a", 50, Phase::Transferring)]);
        assert_eq!(sub.start.sequence.0, 2);
        assert_eq!(sub.start.items[0].downloaded_bytes.0, 50);
        hub.publish([snap("job_a", 50, Phase::Transferring)].into_iter());
        assert!(drain(&mut sub.events).is_empty());
    }

    #[test]
    fn a_full_buffer_drops_only_that_subscriber_and_marks_it() {
        let mut hub = EventHub::new("svc_t");
        let slow = hub.subscribe(vec![]);
        let mut fast = hub.subscribe(vec![]);
        for i in 0..=u64::try_from(SUBSCRIBER_BUFFER).unwrap() {
            hub.publish([snap("job_a", i, Phase::Transferring)].into_iter());
            // The fast one reads as it goes.
            let _ = drain(&mut fast.events);
        }
        assert!(slow.overflowed.load(Ordering::SeqCst));
        assert!(!fast.overflowed.load(Ordering::SeqCst));
        assert_eq!(hub.subscribers(), 1);
        // A closed subscriber is dropped without blocking.
        drop(fast);
        hub.publish([snap("job_a", 9_999, Phase::Transferring)].into_iter());
        assert_eq!(hub.subscribers(), 0);
    }

    #[test]
    fn counts_beyond_javascript_precision_are_exact_strings() {
        let mut hub = EventHub::new("svc_t");
        let mut sub = hub.subscribe(vec![]);
        let big = (1_u64 << 53) + 1;
        hub.publish([snap("job_a", big, Phase::Transferring)].into_iter());
        let e = sub.events.try_recv().unwrap();
        let json = serde_json::to_value(&e).unwrap();
        assert_eq!(json["snapshot"]["downloaded_bytes"], "9007199254740993");
        assert_eq!(json["sequence"], "1");
        assert_eq!(
            serde_json::to_value(EventHub::snapshot_required("svc_t", u64::MAX)).unwrap()["sequence"],
            u64::MAX.to_string()
        );
    }
}
