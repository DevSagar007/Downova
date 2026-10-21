//! Classification of aria2 error codes into stable [`ErrorKind`]s.
//!
//! The detail never includes aria2's `errorMessage`, which can contain the request URL (and so
//! signed-query tokens). Only the numeric code is kept (SEC-01).

use dm_backend_api::BackendError;
use dm_domain::ErrorKind;

/// Maps an aria2 exit/error code (see the aria2c manual, "EXIT STATUS").
pub fn classify(code: u32) -> BackendError {
    let kind = match code {
        2 | 6 | 19 => ErrorKind::NetworkOffline, // timeout, network problem, name resolution
        3 | 4 => ErrorKind::LinkRefreshRequired, // resource not found
        8 => ErrorKind::RangeUnsupported,        // resume required but not supported
        9 => ErrorKind::DiskFull,
        13 => ErrorKind::InvalidRequest, // file already exists; we never overwrite
        22 => ErrorKind::InvalidRange,   // bad or unexpected HTTP response header
        24 => ErrorKind::AuthRequired,
        32 => ErrorKind::ChecksumMismatch,
        _ => ErrorKind::Internal,
    };
    BackendError::new(kind, format!("aria2 error code {code}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_known_codes_and_keeps_detail_free_of_urls() {
        assert_eq!(classify(9).kind, ErrorKind::DiskFull);
        assert_eq!(classify(24).kind, ErrorKind::AuthRequired);
        assert_eq!(classify(22).kind, ErrorKind::InvalidRange);
        assert_eq!(classify(999).kind, ErrorKind::Internal);
        assert_eq!(classify(3).detail, "aria2 error code 3");
    }
}
