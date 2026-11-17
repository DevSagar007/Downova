//! `service.v1.hello` result construction.

use dm_domain::api::{API_VERSION, BackendInfo, HelloResult};
use dm_domain::policy::Engine;

use crate::DownloadBackend;

/// Builds the `service.v1.hello` result for a running service instance. `backend` describes the
/// native (`file`) engine; `media` is present only when a media engine is installed (ADR 0043).
pub fn hello_result<B: DownloadBackend>(
    backend: &B,
    backend_version: Option<String>,
    service_instance_id: &str,
) -> HelloResult {
    let media = backend
        .engine_capabilities(Engine::Media)
        .map(|capabilities| {
            let (id, version) = backend
                .engine_info(Engine::Media)
                .unwrap_or(("media", None));
            BackendInfo {
                id: id.to_owned(),
                version,
                capabilities,
            }
        });
    let id = backend
        .engine_info(Engine::File)
        .map_or_else(|| backend.id(), |(id, _)| id);
    HelloResult {
        api_version: API_VERSION,
        service_version: dm_domain::BUILD_VERSION.to_owned(),
        service_instance_id: service_instance_id.to_owned(),
        backend: BackendInfo {
            id: id.to_owned(),
            version: backend_version,
            capabilities: backend
                .engine_capabilities(Engine::File)
                .unwrap_or_else(|| backend.capabilities()),
        },
        media,
    }
}
