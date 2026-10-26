//! Scriptable in-memory backend for service tests. Never compiled into the service binary.

use std::collections::HashMap;
use std::sync::Mutex;

use dm_backend_api::{
    BackendError, BackendHandle, BackendProgress, DownloadBackend, DurableCheckpoint, StagedOutput,
    TransferRequest, TransferState,
};
use dm_domain::{ByteCount, Capability, CapabilityReport, CapabilitySupport, ErrorKind};

#[derive(Debug, Default)]
pub struct FakeBackend {
    pub caps: Vec<CapabilityReport>,
    pub calls: Mutex<Vec<String>>,
    pub started: Mutex<Vec<TransferRequest>>,
    pub progress: Mutex<HashMap<String, BackendProgress>>,
    /// Scripted `checkpoint` results per handle (default: `Ok(None)`).
    pub checkpoints: Mutex<HashMap<String, Result<Option<DurableCheckpoint>, BackendError>>>,
    /// Scripted `restore` failure (default: success, paused at the checkpoint's bytes).
    pub restore_error: Mutex<Option<BackendError>>,
    /// Checkpoints handed to `restore`, with the new attempt's request.
    pub restored: Mutex<Vec<(TransferRequest, DurableCheckpoint)>>,
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

    pub fn set_checkpoint(
        &self,
        handle: &BackendHandle,
        checkpoint: Result<Option<DurableCheckpoint>, BackendError>,
    ) {
        self.checkpoints
            .lock()
            .unwrap()
            .insert(handle.0.clone(), checkpoint);
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
        naming: None,
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

    /// Like the HTTP backend: a complete attempt leaves `<name>.downova-part` next to the
    /// destination (tests write that file before reporting `Complete`).
    async fn staged_output(&self, handle: &BackendHandle) -> Result<StagedOutput, BackendError> {
        let complete = self
            .progress
            .lock()
            .unwrap()
            .get(&handle.0)
            .is_some_and(|p| p.state == TransferState::Complete);
        if !complete {
            return Err(BackendError::new(
                ErrorKind::InvalidRequest,
                "the transfer is not complete",
            ));
        }
        let started = self.started.lock().unwrap();
        let restored = self.restored.lock().unwrap();
        let request = started
            .iter()
            .chain(restored.iter().map(|(r, _)| r))
            .find(|r| r.attempt_id.as_str() == handle.0)
            .ok_or_else(|| BackendError::new(ErrorKind::InvalidRequest, "unknown attempt"))?;
        Ok(StagedOutput::Staged(
            request
                .destination_dir
                .join(format!("{}.downova-part", request.file_name)),
        ))
    }

    async fn checkpoint(
        &self,
        handle: &BackendHandle,
    ) -> Result<Option<DurableCheckpoint>, BackendError> {
        self.record(format!("checkpoint {}", handle.0));
        self.checkpoints
            .lock()
            .unwrap()
            .get(&handle.0)
            .cloned()
            .unwrap_or(Ok(None))
    }

    async fn restore(
        &self,
        request: TransferRequest,
        checkpoint: &DurableCheckpoint,
    ) -> Result<BackendHandle, BackendError> {
        let handle = BackendHandle(request.attempt_id.as_str().to_owned());
        self.record(format!("restore {}", handle.0));
        if let Some(e) = self.restore_error.lock().unwrap().clone() {
            return Err(e);
        }
        self.set_progress(
            &handle,
            progress(TransferState::Paused, checkpoint.confirmed.0, None),
        );
        self.restored
            .lock()
            .unwrap()
            .push((request, checkpoint.clone()));
        Ok(handle)
    }
}
