//! Refresh-link proof (B2.17, ADR 0020): does a replacement link serve exactly the content a
//! checkpoint describes?
//!
//! Nothing here changes a file or an attempt. The replacement is resolved through the same gated,
//! manual redirect path as a transfer (every hop's scheme checked, credentials only to the
//! original origin), asking for the **last confirmed bytes** of one region (at most
//! [`SAMPLE_BYTES`]). It is compatible only when the answer is a `206` with the checkpoint's
//! strong ETag, its total length and exactly the requested `Content-Range`, and those bytes equal
//! the bytes on disk. The service additionally requires the same resource (origin and path) or a
//! trusted SHA-256 before it keeps progress, because an equal ETag on another resource proves
//! nothing by itself.

use dm_backend_api::{BackendError, DurableCheckpoint, ReplacementCheck, TransferRequest};
use dm_domain::ErrorKind;
use dm_domain::api::ReplaceReason;
use futures_util::StreamExt;
use reqwest::header::{ACCEPT_ENCODING, LOCATION, RANGE};

use crate::HttpConfig;
use crate::durable;
use crate::limits::HostLimiter;
use crate::transfer::{gate, gate_url, origin_scoped, strong_etag, validate_range_response};

/// The most confirmed bytes compared with the replacement.
pub(crate) const SAMPLE_BYTES: u64 = 64 * 1024;

pub(crate) async fn verify(
    client: &reqwest::Client,
    config: &HttpConfig,
    limiter: &HostLimiter,
    request: &TransferRequest,
    checkpoint: &DurableCheckpoint,
) -> Result<ReplacementCheck, BackendError> {
    use ReplaceReason as R;
    let no = |r: ReplaceReason| Ok(ReplacementCheck::Incompatible(r));
    let Ok(sample) = durable::sample(checkpoint, &request.destination_dir, &request.file_name)
    else {
        return no(R::NoCheckpoint);
    };
    let Ok(first) = gate_url(&request.url, &config.scheme_gate) else {
        return no(R::Unverifiable);
    };
    let origin = first.origin();
    let mut url = first;
    let mut hops: u8 = 0;
    let (response, _permit) = loop {
        let permit = limiter.acquire(&url).await?;
        let mut req = client
            .get(url.clone())
            .header(ACCEPT_ENCODING, "identity")
            .header(RANGE, format!("bytes={}-{}", sample.first, sample.last));
        let same_origin = url.origin() == origin;
        for (name, value) in &request.headers {
            if same_origin || !origin_scoped(name) {
                req = req.header(name.as_str(), value.expose());
            }
        }
        let Ok(response) = req.send().await else {
            return no(R::Unverifiable);
        };
        let status = response.status();
        if status.is_redirection() && status != reqwest::StatusCode::NOT_MODIFIED {
            let next = response
                .headers()
                .get(LOCATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|l| url.join(l).ok());
            hops = hops.saturating_add(1);
            let Some(next) = next else {
                return no(R::Unverifiable);
            };
            if hops > config.max_redirects || gate(&next, &config.scheme_gate).is_err() {
                return no(R::Unverifiable);
            }
            drop(response);
            drop(permit);
            url = next;
            continue;
        }
        break (response, permit);
    };
    match response.status() {
        reqwest::StatusCode::PARTIAL_CONTENT => {}
        reqwest::StatusCode::OK => return no(R::RangeIgnored),
        _ => return no(R::Unverifiable),
    }
    let headers = response.headers();
    match strong_etag(headers) {
        None => return no(R::ValidatorMissing),
        Some(etag) if etag != sample.etag => return no(R::ValidatorChanged),
        Some(_) => {}
    }
    if let Err(e) = validate_range_response(
        headers,
        Some(&sample.etag),
        sample.length,
        sample.first,
        sample.last,
    ) {
        return no(if e.kind == ErrorKind::RemoteChanged {
            R::LengthChanged
        } else {
            R::Unverifiable
        });
    }
    let span = sample.last.saturating_sub(sample.first).saturating_add(1);
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let Ok(chunk) = chunk else {
            return no(R::Unverifiable);
        };
        body.extend_from_slice(&chunk);
        if body.len() as u64 > span {
            return no(R::Unverifiable);
        }
    }
    if body.len() as u64 != span {
        return no(R::Unverifiable);
    }
    let part = request
        .destination_dir
        .join(format!("{}{}", request.file_name, crate::PART_SUFFIX));
    let (first_byte, len) = (sample.first, span);
    let on_disk = tokio::task::spawn_blocking(move || read_at(&part, first_byte, len))
        .await
        .map_err(|_| BackendError::new(ErrorKind::Internal, "partial file check failed"))?;
    let Some(on_disk) = on_disk else {
        return no(R::NoCheckpoint);
    };
    if on_disk != body {
        return no(R::ContentDiffers);
    }
    let same_resource = url.origin() == sample.url.origin() && url.path() == sample.url.path();
    let checkpoint = durable::rebind_url(
        checkpoint,
        &request.destination_dir,
        &request.file_name,
        &url,
    )?;
    Ok(ReplacementCheck::Compatible {
        checkpoint,
        same_resource,
    })
}

/// `len` bytes of `path` from `offset`, or `None` when the file is missing or shorter.
fn read_at(path: &std::path::Path, offset: u64, len: u64) -> Option<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).ok()?;
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut out = vec![0u8; usize::try_from(len).ok()?];
    file.read_exact(&mut out).ok()?;
    Some(out)
}
