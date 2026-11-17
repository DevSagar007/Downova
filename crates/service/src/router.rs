//! ADR 0043: one service, two engines. [`EngineRouter`] implements [`DownloadBackend`] over the
//! native engine (always present) and the optional media engine, and sends every call to the
//! engine of its job: a new attempt by `TransferRequest::engine`, everything after that by the
//! handle's private prefix. Engine handles never leave the router; nothing falls back from one
//! engine to the other.

use std::sync::atomic::{AtomicBool, Ordering};

use dm_backend_api::{
    BackendError, BackendHandle, BackendProgress, DownloadBackend, DurableCheckpoint, ProxyConfig,
    ReplacementCheck, StagedOutput, TransferRequest,
};
use dm_domain::policy::Engine;
use dm_domain::{CapabilityReport, ErrorKind};

const FILE: &str = "f:";
const MEDIA: &str = "m:";

/// The native engine and, when installed, the media engine (ADR 0043).
pub struct EngineRouter<F, M> {
    file: F,
    media: Option<M>,
    media_version: Option<String>,
    /// A proxy is in effect. The media engine connects directly, so it refuses new attempts
    /// meanwhile instead of bypassing the proxy (F06, ADR 0034).
    proxy_set: AtomicBool,
}

impl<F, M> std::fmt::Debug for EngineRouter<F, M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EngineRouter")
            .field("media", &self.media.is_some())
            .field("media_version", &self.media_version)
            .field("proxy_set", &self.proxy_set.load(Ordering::SeqCst))
            .finish_non_exhaustive()
    }
}

impl<F: DownloadBackend, M: DownloadBackend> EngineRouter<F, M> {
    pub fn new(file: F, media: Option<(M, Option<String>)>) -> Self {
        let (media, media_version) = match media {
            Some((m, v)) => (Some(m), v),
            None => (None, None),
        };
        Self {
            file,
            media,
            media_version,
            proxy_set: AtomicBool::new(false),
        }
    }

    pub fn file(&self) -> &F {
        &self.file
    }

    pub fn media(&self) -> Option<&M> {
        self.media.as_ref()
    }

    fn media_engine(&self) -> Result<&M, BackendError> {
        self.media.as_ref().ok_or_else(|| {
            BackendError::new(
                ErrorKind::BackendCapabilityUnavailable,
                "video and audio downloads need the media engine (yt-dlp), which is not installed",
            )
        })
    }
}

/// Which engine a router handle belongs to, and the engine's own handle.
fn split(handle: &BackendHandle) -> Result<(Engine, BackendHandle), BackendError> {
    let (engine, inner) = if let Some(inner) = handle.0.strip_prefix(FILE) {
        (Engine::File, inner)
    } else if let Some(inner) = handle.0.strip_prefix(MEDIA) {
        (Engine::Media, inner)
    } else {
        return Err(BackendError::new(
            ErrorKind::InvalidRequest,
            "unknown transfer handle",
        ));
    };
    Ok((engine, BackendHandle(inner.to_owned())))
}

fn wrap(engine: Engine, inner: BackendHandle) -> BackendHandle {
    let prefix = match engine {
        Engine::File => FILE,
        Engine::Media => MEDIA,
    };
    BackendHandle(format!("{prefix}{}", inner.0))
}

/// Runs `$call` on the engine `$handle` belongs to, with the engine's own handle as `$h`.
macro_rules! route {
    ($self:ident, $handle:expr, |$b:ident, $h:ident| $call:expr) => {{
        let (engine, $h) = split($handle)?;
        match engine {
            Engine::File => {
                let $b = &$self.file;
                $call.await
            }
            Engine::Media => {
                let $b = $self.media_engine()?;
                $call.await
            }
        }
    }};
}

impl<F: DownloadBackend, M: DownloadBackend> DownloadBackend for EngineRouter<F, M> {
    /// The native engine's id: checkpoints are native-only and keep their identity.
    fn id(&self) -> &'static str {
        self.file.id()
    }

    fn capabilities(&self) -> Vec<CapabilityReport> {
        self.file.capabilities()
    }

    fn engine_capabilities(&self, engine: Engine) -> Option<Vec<CapabilityReport>> {
        match engine {
            Engine::File => Some(self.file.capabilities()),
            Engine::Media => self.media.as_ref().map(DownloadBackend::capabilities),
        }
    }

    fn engine_info(&self, engine: Engine) -> Option<(&'static str, Option<String>)> {
        match engine {
            Engine::File => Some((self.file.id(), None)),
            Engine::Media => self
                .media
                .as_ref()
                .map(|m| (m.id(), self.media_version.clone())),
        }
    }

    async fn start(&self, request: TransferRequest) -> Result<BackendHandle, BackendError> {
        match request.engine {
            Engine::File => Ok(wrap(Engine::File, self.file.start(request).await?)),
            Engine::Media => {
                let media = self.media_engine()?;
                if self.proxy_set.load(Ordering::SeqCst) {
                    return Err(BackendError::new(
                        ErrorKind::BackendCapabilityUnavailable,
                        "video and audio downloads cannot use the proxy; turn the proxy off to download this",
                    ));
                }
                Ok(wrap(Engine::Media, media.start(request).await?))
            }
        }
    }

    async fn pause(&self, handle: &BackendHandle) -> Result<(), BackendError> {
        route!(self, handle, |b, h| b.pause(&h))
    }

    async fn resume(&self, handle: &BackendHandle) -> Result<(), BackendError> {
        route!(self, handle, |b, h| b.resume(&h))
    }

    async fn remove(&self, handle: &BackendHandle) -> Result<(), BackendError> {
        route!(self, handle, |b, h| b.remove(&h))
    }

    async fn discard(&self, handle: &BackendHandle) -> Result<(), BackendError> {
        route!(self, handle, |b, h| b.discard(&h))
    }

    async fn progress(&self, handle: &BackendHandle) -> Result<BackendProgress, BackendError> {
        route!(self, handle, |b, h| b.progress(&h))
    }

    async fn staged_output(&self, handle: &BackendHandle) -> Result<StagedOutput, BackendError> {
        route!(self, handle, |b, h| b.staged_output(&h))
    }

    async fn checkpoint(
        &self,
        handle: &BackendHandle,
    ) -> Result<Option<DurableCheckpoint>, BackendError> {
        route!(self, handle, |b, h| b.checkpoint(&h))
    }

    /// Checkpoints are native-only (the media engine has no durable resume).
    async fn restore(
        &self,
        request: TransferRequest,
        checkpoint: &DurableCheckpoint,
    ) -> Result<BackendHandle, BackendError> {
        match request.engine {
            Engine::File => Ok(wrap(
                Engine::File,
                self.file.restore(request, checkpoint).await?,
            )),
            Engine::Media => Err(BackendError::new(
                ErrorKind::BackendCapabilityUnavailable,
                "a video or audio download cannot continue from a checkpoint; restart it",
            )),
        }
    }

    async fn discard_output(&self, request: &TransferRequest) -> Result<(), BackendError> {
        match request.engine {
            Engine::File => self.file.discard_output(request).await,
            Engine::Media => self.media_engine()?.discard_output(request).await,
        }
    }

    /// The overall limit applies to the native engine; the media engine reports the capability
    /// as unsupported, and the UI says so.
    fn set_global_rate(&self, bytes_per_second: Option<u64>) -> Result<(), BackendError> {
        self.file.set_global_rate(bytes_per_second)
    }

    fn set_proxy(&self, proxy: Option<ProxyConfig>) -> Result<(), BackendError> {
        let set = proxy.is_some();
        self.file.set_proxy(proxy)?;
        self.proxy_set.store(set, Ordering::SeqCst);
        Ok(())
    }

    fn set_rate(
        &self,
        handle: &BackendHandle,
        bytes_per_second: Option<u64>,
    ) -> Result<(), BackendError> {
        let (engine, h) = split(handle)?;
        match engine {
            Engine::File => self.file.set_rate(&h, bytes_per_second),
            Engine::Media => self.media_engine()?.set_rate(&h, bytes_per_second),
        }
    }

    async fn verify_replacement(
        &self,
        request: &TransferRequest,
        checkpoint: &DurableCheckpoint,
    ) -> Result<ReplacementCheck, BackendError> {
        match request.engine {
            Engine::File => self.file.verify_replacement(request, checkpoint).await,
            Engine::Media => Ok(ReplacementCheck::Incompatible(
                dm_domain::api::ReplaceReason::Unverifiable,
            )),
        }
    }
}

#[cfg(test)]
#[path = "router/tests.rs"]
mod tests;
