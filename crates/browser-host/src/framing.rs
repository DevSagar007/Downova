//! Chrome native-messaging framing: a 32-bit length in native byte order, then that many bytes
//! of UTF-8 JSON (https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging).
//!
//! - The length is checked against [`MAX_REQUEST`] **before** any buffer is allocated.
//! - Reads are exact: a stream that ends inside the length or the body is `Truncated`, never a
//!   short message.
//! - Responses are limited to Chrome's 1 MB host-to-browser maximum.

use std::io::{self, Read, Write};

/// Largest request this host accepts. A hand-off carries one URL (≤ 8 KiB) and a few short
/// fields; anything near this size is not a legitimate request.
pub const MAX_REQUEST: u32 = 64 * 1024;
/// Chrome's limit for a single message from the host to the browser.
pub const MAX_RESPONSE: usize = 1024 * 1024;

/// Why no request could be read.
#[derive(Debug, PartialEq, Eq)]
pub enum ReadError {
    /// The stream ended cleanly before a new message (normal end of a session).
    Eof,
    /// The stream ended inside a length prefix or a body.
    Truncated,
    /// The announced length exceeds [`MAX_REQUEST`]; nothing was allocated or read.
    TooLarge(u32),
    /// Any other I/O failure.
    Io(io::ErrorKind),
}

/// Fills `buf` completely, or reports how many bytes arrived before end of stream.
fn read_full(r: &mut impl Read, buf: &mut [u8]) -> Result<usize, io::Error> {
    let mut filled = 0;
    while filled < buf.len() {
        let rest = buf.get_mut(filled..).unwrap_or_default();
        match r.read(rest) {
            Ok(0) => break,
            Ok(n) => filled = filled.saturating_add(n),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(filled)
}

/// Reads one framed message.
pub fn read_frame(r: &mut impl Read) -> Result<Vec<u8>, ReadError> {
    let mut len = [0u8; 4];
    match read_full(r, &mut len).map_err(|e| ReadError::Io(e.kind()))? {
        0 => return Err(ReadError::Eof),
        4 => {}
        _ => return Err(ReadError::Truncated),
    }
    let n = u32::from_ne_bytes(len);
    if n > MAX_REQUEST {
        return Err(ReadError::TooLarge(n));
    }
    let size = usize::try_from(n).map_err(|_| ReadError::TooLarge(n))?;
    let mut body = vec![0u8; size];
    if read_full(r, &mut body).map_err(|e| ReadError::Io(e.kind()))? != size {
        return Err(ReadError::Truncated);
    }
    Ok(body)
}

/// Writes one framed message and flushes it. `stdout` must carry nothing else.
pub fn write_frame(w: &mut impl Write, body: &[u8]) -> io::Result<()> {
    if body.len() > MAX_RESPONSE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "response exceeds the 1 MB native-messaging limit",
        ));
    }
    let n = u32::try_from(body.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "response too large"))?;
    w.write_all(&n.to_ne_bytes())?;
    w.write_all(body)?;
    w.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(body: &[u8]) -> Vec<u8> {
        let mut v = (body.len() as u32).to_ne_bytes().to_vec();
        v.extend_from_slice(body);
        v
    }

    #[test]
    fn round_trip_and_eof() {
        let mut out = Vec::new();
        write_frame(&mut out, br#"{"a":1}"#).unwrap();
        write_frame(&mut out, "{\"t\":\"বাংলা\"}".as_bytes()).unwrap();
        let mut r = out.as_slice();
        assert_eq!(read_frame(&mut r).unwrap(), br#"{"a":1}"#);
        assert_eq!(read_frame(&mut r).unwrap(), "{\"t\":\"বাংলা\"}".as_bytes());
        assert_eq!(read_frame(&mut r), Err(ReadError::Eof));
    }

    #[test]
    fn truncated_length_or_body_is_never_a_short_message() {
        assert_eq!(read_frame(&mut &[1u8, 0][..]), Err(ReadError::Truncated));
        let mut short = frame(b"0123456789");
        short.truncate(8);
        assert_eq!(read_frame(&mut short.as_slice()), Err(ReadError::Truncated));
    }

    #[test]
    fn oversized_length_is_refused_before_allocation() {
        // A length of 4 GiB - 1 with no body: refused from the prefix alone.
        let huge = u32::MAX.to_ne_bytes();
        assert_eq!(
            read_frame(&mut &huge[..]),
            Err(ReadError::TooLarge(u32::MAX))
        );
        let just_over = (MAX_REQUEST + 1).to_ne_bytes();
        assert_eq!(
            read_frame(&mut &just_over[..]),
            Err(ReadError::TooLarge(MAX_REQUEST + 1))
        );
    }

    #[test]
    fn reads_are_exact_across_partial_reads() {
        // A reader that returns one byte at a time.
        struct Trickle<'a>(&'a [u8]);
        impl Read for Trickle<'_> {
            fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
                match (self.0.split_first(), buf.first_mut()) {
                    (Some((b, rest)), Some(slot)) => {
                        *slot = *b;
                        self.0 = rest;
                        Ok(1)
                    }
                    _ => Ok(0),
                }
            }
        }
        let data = frame(b"{\"op\":\"hello\"}");
        assert_eq!(
            read_frame(&mut Trickle(&data)).unwrap(),
            b"{\"op\":\"hello\"}"
        );
    }

    #[test]
    fn oversized_responses_are_not_written() {
        let mut out = Vec::new();
        assert!(write_frame(&mut out, &vec![b' '; MAX_RESPONSE + 1]).is_err());
        assert!(out.is_empty());
    }
}
