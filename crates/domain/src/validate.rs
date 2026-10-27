//! Syntactic checks clients apply before sending a request (B0.6 browser host, B2.1 desktop).
//!
//! They are a first line only: the service still validates every request. Whether a link really
//! needs no sign-in cannot be known here; the service reports `AUTH_REQUIRED` /
//! `LINK_REFRESH_REQUIRED` for links that do.

/// Longest URL accepted (matches the contract).
pub const MAX_URL: usize = 8192;

/// Accepts only absolute http(s) URLs without embedded credentials, spaces or control characters.
pub fn check_url(url: &str) -> Result<(), &'static str> {
    if url.len() > MAX_URL {
        return Err("the link is too long");
    }
    if url.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err("the link contains spaces or control characters");
    }
    let lower = url.to_ascii_lowercase();
    let rest = if let Some(r) = lower.strip_prefix("https://") {
        r
    } else if let Some(r) = lower.strip_prefix("http://") {
        r
    } else {
        return Err("only http and https links can be handed to Downova");
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.contains('@') {
        return Err("links with an embedded user name or password are not handed off");
    }
    if authority.is_empty() || authority.starts_with(':') {
        return Err("the link has no host");
    }
    Ok(())
}

/// `[A-Za-z0-9_-]{min,max}`: request IDs and idempotency keys.
pub fn is_token(s: &str, min: usize, max: usize) -> bool {
    (min..=max).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// An idempotency key as clients send it: 8–128 token characters.
pub fn is_idempotency_key(s: &str) -> bool {
    is_token(s, 8, 128)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_are_checked_syntactically() {
        for ok in [
            "https://h/f.zip",
            "http://h:8080/a?b=c#d",
            "HTTPS://H/x",
            "https://h/a@b",
        ] {
            assert!(check_url(ok).is_ok(), "{ok}");
        }
        for bad in [
            "ftp://h/f",
            "https://u:p@h/f",
            "https:///f",
            "https://:80/f",
            "https://h/a b",
            "https://h/\n",
            "file:///etc/passwd",
            "",
        ] {
            assert!(check_url(bad).is_err(), "{bad}");
        }
        assert!(check_url(&format!("https://h/{}", "a".repeat(MAX_URL))).is_err());
    }

    #[test]
    fn keys_are_tokens() {
        assert!(is_idempotency_key("ui-01234567-89ab"));
        assert!(!is_idempotency_key("short"));
        assert!(!is_idempotency_key("has space!"));
        assert!(!is_idempotency_key(&"k".repeat(129)));
    }
}
