//! `service.v1.hello` result construction.

use dm_domain::api::{API_VERSION, BackendInfo, HelloResult};

use crate::DownloadBackend;

/// Builds the `service.v1.hello` result for a running service instance.
pub fn hello_result<B: DownloadBackend>(
    backend: &B,
    backend_version: Option<String>,
    service_instance_id: &str,
) -> HelloResult {
    HelloResult {
        api_version: API_VERSION,
        service_version: env!("CARGO_PKG_VERSION").to_owned(),
        service_instance_id: service_instance_id.to_owned(),
        backend: BackendInfo {
            id: backend.id().to_owned(),
            version: backend_version,
            capabilities: backend.capabilities(),
        },
    }
}
