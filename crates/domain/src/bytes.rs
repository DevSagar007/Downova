//! Byte counts.
//!
//! On the wire a byte count is a decimal string, because JSON numbers lose precision above 2^53
//! in JavaScript consumers. Internally it is a `u64`; arithmetic must be checked.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// An unsigned byte count serialized as a canonical decimal string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct ByteCount(pub u64);

/// Error returned when a decimal byte-count string is not canonical.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ByteCountParseError;

impl fmt::Display for ByteCountParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("byte count must be a canonical unsigned decimal string that fits in u64")
    }
}

impl std::error::Error for ByteCountParseError {}

impl ByteCount {
    /// Parses a canonical decimal string: digits only, no sign, no leading zeros (except "0").
    pub fn parse_decimal(value: &str) -> Result<Self, ByteCountParseError> {
        let canonical = !value.is_empty()
            && value.bytes().all(|b| b.is_ascii_digit())
            && (value == "0" || !value.starts_with('0'));
        if !canonical {
            return Err(ByteCountParseError);
        }
        value.parse::<u64>().map(ByteCount).map_err(|_| ByteCountParseError)
    }

    /// Checked addition; `None` on overflow.
    pub fn checked_add(self, other: ByteCount) -> Option<ByteCount> {
        self.0.checked_add(other.0).map(ByteCount)
    }
}

impl fmt::Display for ByteCount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Serialize for ByteCount {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ByteCount {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        ByteCount::parse_decimal(&raw).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_u64_max_as_string() {
        let json = serde_json::to_string(&ByteCount(u64::MAX)).unwrap();
        assert_eq!(json, "\"18446744073709551615\"");
        let back: ByteCount = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ByteCount(u64::MAX));
    }

    #[test]
    fn rejects_non_canonical_values() {
        for bad in ["", "01", "+1", "-1", "1.0", " 1", "18446744073709551616"] {
            assert!(ByteCount::parse_decimal(bad).is_err(), "{bad:?} must be rejected");
        }
        assert!(serde_json::from_str::<ByteCount>("1024").is_err(), "JSON numbers are rejected");
    }

    #[test]
    fn addition_is_checked() {
        assert_eq!(ByteCount(u64::MAX).checked_add(ByteCount(1)), None);
        assert_eq!(ByteCount(2).checked_add(ByteCount(3)), Some(ByteCount(5)));
    }
}
