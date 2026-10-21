//! Service-owned identifiers.
//!
//! A [`JobId`] is the logical identity of a download for its whole life. It is never an aria2
//! GID: backend handles live only inside the backend adapter and are mapped per attempt.
//! An [`AttemptId`] identifies one transfer attempt; events from a stale attempt are rejected.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};

/// Validation error for identifiers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdError {
    /// Expected prefix, e.g. `job_`.
    pub expected_prefix: &'static str,
}

impl fmt::Display for IdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "identifier must be `{}` followed by 1-64 characters of [0-9A-Za-z_-]",
            self.expected_prefix
        )
    }
}

impl std::error::Error for IdError {}

fn validate(value: &str, prefix: &'static str) -> Result<(), IdError> {
    let err = IdError {
        expected_prefix: prefix,
    };
    let rest = value.strip_prefix(prefix).ok_or_else(|| err.clone())?;
    let valid_chars = rest
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    if rest.is_empty() || rest.len() > 64 || !valid_chars {
        return Err(err);
    }
    Ok(())
}

macro_rules! prefixed_id {
    ($(#[$doc:meta])* $name:ident, $prefix:literal) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Required prefix of this identifier on the wire.
            pub const PREFIX: &'static str = $prefix;

            /// Parses and validates an identifier.
            pub fn parse(value: &str) -> Result<Self, IdError> {
                validate(value, Self::PREFIX)?;
                Ok(Self(value.to_owned()))
            }

            /// Returns the identifier as a string slice.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let raw = String::deserialize(deserializer)?;
                Self::parse(&raw).map_err(serde::de::Error::custom)
            }
        }
    };
}

prefixed_id!(
    /// Logical download job identifier (`job_…`).
    JobId,
    "job_"
);
prefixed_id!(
    /// Transfer attempt identifier (`att_…`).
    AttemptId,
    "att_"
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_prefixed_ids() {
        assert_eq!(JobId::parse("job_01H-x_y").unwrap().as_str(), "job_01H-x_y");
        assert!(AttemptId::parse("att_1").is_ok());
    }

    #[test]
    fn rejects_backend_gids_and_malformed_ids() {
        // A raw aria2 GID must never be accepted as a job ID.
        assert!(JobId::parse("2089b05ecca3d829").is_err());
        assert!(JobId::parse("job_").is_err());
        assert!(JobId::parse("job_has space").is_err());
        assert!(JobId::parse("att_123").is_err());
        assert!(JobId::parse(&format!("job_{}", "a".repeat(65))).is_err());
    }
}
