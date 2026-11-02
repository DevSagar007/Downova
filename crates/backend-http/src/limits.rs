//! Per-host connection budget (B1.5, CORE-02/E03; ADR 0010).
//!
//! Every HTTP request this backend sends (a probe, a whole-file or resumed transfer, each segment
//! worker, and each redirect hop) first takes a permit for its destination host and keeps it until
//! that request and its body are finished (dropped). At most [`HttpConfig::max_connections_per_host`]
//! permits exist per host, shared by all attempts of the backend, so concurrent requests to one host
//! never exceed it.
//!
//! The client keeps **no idle pooled connections** (`pool_max_idle_per_host(0)`): each request's
//! connection closes when its permit is released, so open TCP connections per host also stay within
//! the budget (plus connections the OS is still tearing down).
//!
//! **Host key:** the URL's host as parsed (ASCII-lowercased; internationalised names in punycode),
//! without a trailing dot; IPv6 literals as written. The port and scheme are ignored, so
//! `http://h:80` and `https://h:8443` share one budget: the conservative choice for a server.
//! A redirect hop releases the previous hop's permit before taking one for its own host.
//!
//! [`HttpConfig::max_connections_per_host`]: crate::HttpConfig::max_connections_per_host

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use dm_backend_api::BackendError;
use dm_domain::ErrorKind;
use reqwest::Url;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// A held connection slot for one host; released when dropped.
pub(crate) type Permit = OwnedSemaphorePermit;

#[derive(Debug)]
struct HostBudget {
    slots: Arc<Semaphore>,
    waiting: AtomicUsize,
}

/// Cancellation of acquire also clears the pressure signal.
#[derive(Debug)]
struct Waiting(Arc<HostBudget>);

impl Drop for Waiting {
    fn drop(&mut self) {
        self.0.waiting.fetch_sub(1, Ordering::SeqCst);
    }
}

/// The shared per-host budget.
#[derive(Debug)]
pub(crate) struct HostLimiter {
    per_host: usize,
    hosts: Mutex<HashMap<String, Arc<HostBudget>>>,
}

impl HostLimiter {
    pub(crate) fn new(per_host: usize) -> Self {
        Self {
            per_host: per_host.max(1),
            hosts: Mutex::new(HashMap::new()),
        }
    }

    /// The canonical host key (see the module documentation).
    pub(crate) fn key(url: &Url) -> String {
        url.host_str()
            .unwrap_or_default()
            .trim_end_matches('.')
            .to_ascii_lowercase()
    }

    fn semaphore(&self, key: String) -> Arc<HostBudget> {
        let mut hosts = crate::lock(&self.hosts);
        Arc::clone(hosts.entry(key).or_insert_with(|| {
            Arc::new(HostBudget {
                slots: Arc::new(Semaphore::new(self.per_host)),
                waiting: AtomicUsize::new(0),
            })
        }))
    }

    /// Waits for a slot to `url`'s host. Callers wrap this in their stop/halt select, so a paused
    /// or cancelled attempt never waits forever.
    pub(crate) async fn acquire(&self, url: &Url) -> Result<Permit, BackendError> {
        let budget = self.semaphore(Self::key(url));
        budget.waiting.fetch_add(1, Ordering::SeqCst);
        let waiting = Waiting(Arc::clone(&budget));
        let result = Arc::clone(&budget.slots)
            .acquire_owned()
            .await
            .map_err(|_| BackendError::new(ErrorKind::Internal, "connection budget closed"));
        drop(waiting);
        result
    }

    /// Validated segments yield their request when another request is waiting (ADR 0023).
    pub(crate) fn has_waiters(&self, url: &Url) -> bool {
        self.semaphore(Self::key(url))
            .waiting
            .load(Ordering::SeqCst)
            > 0
    }

    /// Slots of `url`'s host in use right now (tests, diagnostics).
    #[cfg(test)]
    pub(crate) fn in_use(&self, url: &Url) -> usize {
        let sem = self.semaphore(Self::key(url));
        self.per_host.saturating_sub(sem.slots.available_permits())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancelling_a_waiter_clears_pressure_without_releasing_another_requests_slot() {
        let l = HostLimiter::new(1);
        let url = Url::parse("http://a.example/f").unwrap();
        let permit = l.acquire(&url).await.unwrap();
        {
            let pending = l.acquire(&url);
            tokio::pin!(pending);
            tokio::select! {
                result = &mut pending => panic!("unexpected permit: {result:?}"),
                _ = tokio::time::sleep(std::time::Duration::from_millis(10)) => {},
            }
            assert!(l.has_waiters(&url));
        }
        assert!(!l.has_waiters(&url));
        assert_eq!(l.in_use(&url), 1);
        drop(permit);
        assert_eq!(l.in_use(&url), 0);
    }

    #[test]
    fn host_keys_ignore_case_ports_schemes_and_a_trailing_dot() {
        let k = |u: &str| HostLimiter::key(&Url::parse(u).unwrap());
        assert_eq!(k("http://Example.ORG/a"), "example.org");
        assert_eq!(k("https://example.org:8443/b"), "example.org");
        assert_eq!(k("http://example.org./c"), "example.org");
        assert_eq!(k("http://bücher.example/"), "xn--bcher-kva.example");
        assert_eq!(k("http://[::1]:8080/"), "[::1]");
        assert_ne!(k("http://localhost/"), k("http://127.0.0.1/"));
    }

    #[tokio::test]
    async fn at_most_the_budget_is_handed_out_per_host_and_released_on_drop() {
        let l = HostLimiter::new(2);
        let a = Url::parse("http://a.example/").unwrap();
        let b = Url::parse("http://b.example/").unwrap();
        let p1 = l.acquire(&a).await.unwrap();
        let _p2 = l.acquire(&a).await.unwrap();
        assert_eq!(l.in_use(&a), 2);
        // A third request to the same host waits…
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), l.acquire(&a))
                .await
                .is_err()
        );
        // …another host does not.
        let _q = l.acquire(&b).await.unwrap();
        drop(p1);
        let _p3 = tokio::time::timeout(std::time::Duration::from_millis(500), l.acquire(&a))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(l.in_use(&a), 2);
    }
}
