//! Scriptable in-memory backend for service tests. Never compiled into the service binary.

use std::collections::HashMap;
use std::sync::Mutex;

use dm_backend_api::{
    BackendError, BackendHandle, BackendProgress, DownloadBackend, TransferRequest, TransferState,
};
use dm_domain::{ByteCount, Capability, CapabilityReport, CapabilitySupport, ErrorKind};

#[derive(Debug, Default)]
pub struct FakeBackend {
    pub caps: Vec<CapabilityReport>,
    pub calls: Mutex<Vec<String>>,
    pub started: Mutex<Vec<TransferRequest>>,
    pub progress: Mutex<HashMap<String, BackendProgress>>,
}

impl FakeBackend {
    /// Every capability set to `support`.
    pub fn with_all(support: CapabilitySupport) -> Self {
        Self {
            caps: Capability::ALL
                .into_iter()
                .map(|capability| CapabilityReport {
                    capability,
                    support,
                    note: None,
                })
                .collect(),
            ..Self::default()
        }
    }

    /// Like `with_all(Supported)` but with one capability overridden.
    pub fn supported_except(capability: Capability, support: CapabilitySupport) -> Self {
        let mut fake = Self::with_all(CapabilitySupport::Supported);
        for entry in &mut fake.caps {
            if entry.capability == capability {
                entry.support = support;
            }
        }
        fake
    }

    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    pub fn set_progress(&self, handle: &BackendHandle, progress: BackendProgress) {
        self.progress
            .lock()
            .unwrap()
            .insert(handle.0.clone(), progress);
    }

    fn record(&self, call: String) {
        self.calls.lock().unwrap().push(call);
    }
}

pub fn progress(state: TransferState, completed: u64, total: Option<u64>) -> BackendProgress {
    BackendProgress {
        state,
        completed: ByteCount(completed),
        total: total.map(ByteCount),
        connections: 1,
    }
}

impl DownloadBackend for FakeBackend {
    fn id(&self) -> &'static str {
        "fake"
    }

    fn capabilities(&self) -> Vec<CapabilityReport> {
        self.caps.clone()
    }

    async fn start(&self, request: TransferRequest) -> Result<BackendHandle, BackendError> {
        let handle = BackendHandle(request.attempt_id.as_str().to_owned());
        self.record(format!(
            "start {} conns={}",
            handle.0, request.max_connections
        ));
        self.set_progress(&handle, progress(TransferState::Active, 0, None));
        self.started.lock().unwrap().push(request);
        Ok(handle)
    }

    async fn pause(&self, handle: &BackendHandle) -> Result<(), BackendError> {
        self.record(format!("pause {}", handle.0));
        Ok(())
    }

    async fn resume(&self, handle: &BackendHandle) -> Result<(), BackendError> {
        self.record(format!("resume {}", handle.0));
        Ok(())
    }

    async fn remove(&self, handle: &BackendHandle) -> Result<(), BackendError> {
        self.record(format!("remove {}", handle.0));
        self.progress.lock().unwrap().remove(&handle.0);
        Ok(())
    }

    async fn discard(&self, handle: &BackendHandle) -> Result<(), BackendError> {
        self.record(format!("discard {}", handle.0));
        self.progress.lock().unwrap().remove(&handle.0);
        Ok(())
    }

    async fn progress(&self, handle: &BackendHandle) -> Result<BackendProgress, BackendError> {
        self.progress
            .lock()
            .unwrap()
            .get(&handle.0)
            .cloned()
            .ok_or_else(|| BackendError::new(ErrorKind::InvalidRequest, "unknown attempt"))
    }
}
