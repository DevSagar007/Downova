//! File naming (B1.6, E04, ADR 0011).
//!
//! The service owns every name. A server's `Content-Disposition` is advisory input: it is parsed
//! here (RFC 6266, with RFC 8187 `filename*`), reduced to one path component, made safe for the
//! platform's file systems and fitted to their length limits, and only then used as the *final*
//! name. The staging (partial-file) name is chosen once, at acceptance, and never changes.
//!
//! Precedence of the final name: a valid `filename*` → `filename` → the final response URL's last
//! path segment → the requested URL's last path segment → `download`. A candidate that is empty
//! after sanitizing falls through to the next one.

use std::borrow::Cow;
use std::collections::HashSet;
use std::path::Path;

use dm_backend_api::ResponseNaming;
use unicode_segmentation::UnicodeSegmentation;

/// Name used when no source yields one.
pub(crate) const DEFAULT_NAME: &str = "download";

/// Suffixes Downova or its engines append to a chosen name: the partial file, aria2's control
/// file. A final name never ends with one.
const CONTROL_SUFFIXES: [&str; 2] = [".downova-part", ".aria2"];

/// Room kept free in every name for what may be appended to it: a collision suffix
/// (` (` + up to 8 hex digits + `)` = 11) plus the longest control suffix (`.downova-part` = 13).
/// All of them are ASCII, so the reserve means the same in bytes and in UTF-16 units.
pub(crate) const SUFFIX_RESERVE: usize = 24;

/// A name is never shortened below this to fit a long directory path (see [`Limits::path`]).
const MIN_NAME: usize = 32;

/// Longest extension kept apart when a name is truncated (measured like the name).
const MAX_EXTENSION: usize = 16;

/// How a file system counts name length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Measure {
    /// UTF-8 bytes: ext4, Btrfs, XFS, APFS (Linux and macOS default volumes).
    Utf8Bytes,
    /// UTF-16 code units: NTFS, ReFS, exFAT, FAT long names (Windows volumes).
    Utf16Units,
}

impl Measure {
    pub(crate) fn of(self, s: &str) -> usize {
        match self {
            Measure::Utf8Bytes => s.len(),
            Measure::Utf16Units => s.encode_utf16().count(),
        }
    }
}

/// Documented, conservative name and path limits (not universal constants).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Limits {
    /// Longest file name component.
    pub component: usize,
    pub measure: Measure,
    /// Longest whole path kept when the directory allows it. On Windows this is the classic
    /// `MAX_PATH` (259 characters + terminator), so that other programs without long-path support
    /// can open the file; Downova itself uses long paths through the standard library.
    pub path: usize,
}

impl Limits {
    /// The limits used on this platform: 255 UTF-16 units / 259-unit paths on Windows, 255 UTF-8
    /// bytes / 4095-byte paths elsewhere. Volumes with smaller limits (e.g. eCryptfs, some network
    /// shares) refuse the file at creation, which is reported as `DESTINATION_UNWRITABLE`.
    pub(crate) const fn native() -> Self {
        if cfg!(windows) {
            Self {
                component: 255,
                measure: Measure::Utf16Units,
                path: 259,
            }
        } else {
            Self {
                component: 255,
                measure: Measure::Utf8Bytes,
                path: 4095,
            }
        }
    }
}

/// Where a job's final name came from. Stored with the job; `None` in the job means the name is
/// still provisional.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Source {
    /// RFC 8187 `filename*` of the response.
    FilenameExt,
    /// `filename` of the response.
    Filename,
    /// Last path segment of the final response URL (after redirects).
    ResponseUrl,
    /// Last path segment of the requested URL.
    RequestUrl,
    /// No usable source: [`DEFAULT_NAME`].
    Default,
    /// A job created before B1.6: its name was already in use as the final name.
    Legacy,
}

impl Source {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Source::FilenameExt => "filename_ext",
            Source::Filename => "filename",
            Source::ResponseUrl => "response_url",
            Source::RequestUrl => "request_url",
            Source::Default => "default",
            Source::Legacy => "legacy",
        }
    }

    pub(crate) fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "filename_ext" => Source::FilenameExt,
            "filename" => Source::Filename,
            "response_url" => Source::ResponseUrl,
            "request_url" => Source::RequestUrl,
            "default" => Source::Default,
            "legacy" => Source::Legacy,
            _ => return None,
        })
    }
}

/// The file-name parameters of a `Content-Disposition` value. Each is `None` when absent,
/// malformed, duplicated or (for `filename*`) in an unsupported charset.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Disposition {
    pub filename_ext: Option<String>,
    pub filename: Option<String>,
}

/// Parses a `Content-Disposition` field value (RFC 6266 §4).
///
/// - The disposition type (`attachment`, `inline`, anything else) is ignored: the user already
///   chose to download.
/// - Parameter names are case-insensitive. Values are tokens or quoted strings; `\` escapes the
///   next character inside quotes, and `;` inside quotes is data.
/// - A parameter that occurs more than once is ignored entirely (RFC 6266 §4.1 calls such values
///   invalid); the other parameters still count.
/// - An unterminated quoted string, or text after a closing quote, makes that parameter and
///   everything after it unusable.
/// - `filename*` follows RFC 8187: `charset'language'pct-encoded`, charset `UTF-8` or `ISO-8859-1`
///   (case-insensitive). Malformed percent-encoding, invalid UTF-8 or another charset discard it.
/// - `filename` is taken literally: no percent- or RFC 2047 decoding.
pub(crate) fn parse_content_disposition(value: &str) -> Disposition {
    let params = parameters(value);
    let single = |name: &str| {
        let mut found = params.iter().filter(|(n, _)| n == name);
        match (found.next(), found.next()) {
            (Some((_, v)), None) => Some(v.clone()),
            _ => None,
        }
    };
    Disposition {
        filename_ext: single("filename*").and_then(|v| decode_ext_value(&v)),
        filename: single("filename"),
    }
}

/// `(lower-case name, unquoted value)` pairs after the disposition type.
fn parameters(value: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Some((_, mut rest)) = value.split_once(';') else {
        return out;
    };
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            return out;
        }
        let Some(eq) = rest.find(['=', ';']) else {
            return out;
        };
        let (name, after) = rest.split_at(eq);
        let name = name.trim().to_ascii_lowercase();
        let Some(after) = after.strip_prefix('=') else {
            // `name;` without a value.
            rest = after.strip_prefix(';').unwrap_or_default();
            continue;
        };
        let after = after.trim_start();
        let (val, next) = if let Some(quoted) = after.strip_prefix('"') {
            let Some((val, end)) = unquote(quoted) else {
                return out;
            };
            let tail = end.trim_start();
            match tail.strip_prefix(';') {
                Some(next) => (val, next),
                None if tail.is_empty() => (val, ""),
                None => return out,
            }
        } else {
            match after.split_once(';') {
                Some((v, next)) => (v.trim().to_owned(), next),
                None => (after.trim().to_owned(), ""),
            }
        };
        if !name.is_empty() {
            out.push((name, val));
        }
        rest = next;
    }
}

/// Reads a quoted string body (after the opening quote): the value and what follows the closing
/// quote, or `None` when it never closes.
fn unquote(s: &str) -> Option<(String, &str)> {
    let mut value = String::new();
    let mut chars = s.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => value.push(chars.next()?.1),
            '"' => return Some((value, s.get(i.saturating_add(1)..)?)),
            other => value.push(other),
        }
    }
    None
}

/// RFC 8187 `ext-value` → text, or `None`.
fn decode_ext_value(v: &str) -> Option<String> {
    let (charset, rest) = v.split_once('\'')?;
    let (_language, encoded) = rest.split_once('\'')?;
    let bytes = percent_decode_strict(encoded)?;
    if charset.eq_ignore_ascii_case("utf-8") {
        String::from_utf8(bytes).ok()
    } else if charset.eq_ignore_ascii_case("iso-8859-1") {
        Some(bytes.iter().map(|&b| char::from(b)).collect())
    } else {
        None
    }
}

/// Percent-decoding that refuses a `%` not followed by two hex digits.
fn percent_decode_strict(s: &str) -> Option<Vec<u8>> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while let Some(&b) = bytes.get(i) {
        if b == b'%' {
            let hex = bytes.get(i.saturating_add(1)..i.saturating_add(3))?;
            let hex = std::str::from_utf8(hex).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i = i.saturating_add(3);
        } else {
            out.push(b);
            i = i.saturating_add(1);
        }
    }
    Some(out)
}

/// Lenient percent-decoding for URL path segments: a stray `%` stays literal, and bytes that are
/// not UTF-8 become U+FFFD.
pub(crate) fn percent_decode_lossy(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while let Some(&b) = bytes.get(i) {
        let decoded = (b == b'%')
            .then(|| bytes.get(i.saturating_add(1)..i.saturating_add(3)))
            .flatten()
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match decoded {
            Some(value) => {
                out.push(value);
                i = i.saturating_add(3);
            }
            None => {
                out.push(b);
                i = i.saturating_add(1);
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The last path segment of a URL, still encoded; the query and fragment are never looked at.
pub(crate) fn url_last_segment(url: &str) -> &str {
    let without_query = url.split(['?', '#']).next().unwrap_or_default();
    let after_scheme = without_query
        .split_once("://")
        .map_or(without_query, |(_, r)| r);
    after_scheme
        .split_once('/')
        .map_or("", |(_, p)| p)
        .rsplit('/')
        .next()
        .unwrap_or("")
}

/// Windows device names, reserved with any extension and in any case.
const RESERVED: [&str; 32] = [
    "CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$", "COM0", "COM1", "COM2", "COM3", "COM4",
    "COM5", "COM6", "COM7", "COM8", "COM9", "COM¹", "COM²", "COM³", "LPT0", "LPT1", "LPT2", "LPT3",
    "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9", "LPT¹", "LPT²", "LPT³",
];

/// Invisible formatting characters that can disguise a name (bidirectional overrides and
/// isolates, zero-width space, word joiner, BOM). ZWJ and ZWNJ stay: Bangla and emoji need them.
fn is_disguising(c: char) -> bool {
    matches!(
        c,
        '\u{200B}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'
            | '\u{2066}'..='\u{2069}' | '\u{FEFF}'
    )
}

/// NFC form; names are stored and compared composed.
pub(crate) fn nfc(s: &str) -> Cow<'_, str> {
    icu_normalizer::ComposingNormalizerBorrowed::new_nfc().normalize(s)
}

/// Reduces untrusted text to one safe file-name component, or `None` when nothing is left.
///
/// - NFC-normalized; Bangla, other scripts and emoji are kept.
/// - Path separators (`/`, `\`), the other characters Windows forbids (`<>:"|?*`) and control
///   characters become `_`; disguising format characters are removed.
/// - Leading dots and spaces are removed (no hidden files, no `..`); trailing dots and spaces are
///   removed (Windows drops them silently).
/// - Windows device names (`CON`, `NUL`, `COM1`, `LPT¹`, …, with any extension) get a `_` prefix.
/// - A name ending in a control suffix (`.downova-part`, `.aria2`) gets a `_` appended.
///
/// Length is handled separately by [`fit`].
pub(crate) fn sanitize(raw: &str) -> Option<String> {
    let cleaned: String = nfc(raw)
        .chars()
        .filter(|&c| !is_disguising(c))
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned
        .trim()
        .trim_start_matches(|c: char| c == '.' || c.is_whitespace())
        .trim_end_matches(|c: char| c == '.' || c.is_whitespace());
    if trimmed.is_empty() {
        return None;
    }
    let stem = trimmed
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end()
        .to_uppercase();
    let mut name = if RESERVED.contains(&stem.as_str()) {
        format!("_{trimmed}")
    } else {
        trimmed.to_owned()
    };
    let lower = name.to_lowercase();
    if CONTROL_SUFFIXES.iter().any(|s| lower.ends_with(s)) {
        name.push('_');
    }
    Some(name)
}

/// The provisional name from the requested URL (used as the staging name too).
pub(crate) fn provisional(url: &str) -> (String, Source) {
    match sanitize(&percent_decode_lossy(url_last_segment(url))) {
        Some(name) => (name, Source::RequestUrl),
        None => (DEFAULT_NAME.to_owned(), Source::Default),
    }
}

/// The final-name candidate by precedence (see the module notes), sanitized but not yet fitted.
pub(crate) fn choose(naming: Option<&ResponseNaming>, request_url: &str) -> (String, Source) {
    if let Some(naming) = naming {
        let disposition = naming
            .content_disposition
            .as_deref()
            .map(parse_content_disposition)
            .unwrap_or_default();
        let candidates = [
            (disposition.filename_ext, Source::FilenameExt),
            (disposition.filename, Source::Filename),
            (
                naming.url_file_name.as_deref().map(percent_decode_lossy),
                Source::ResponseUrl,
            ),
        ];
        for (candidate, source) in candidates {
            if let Some(name) = candidate.as_deref().and_then(sanitize) {
                return (name, source);
            }
        }
    }
    provisional(request_url)
}

/// `stem` and `.ext` (empty when there is no usable extension).
fn split_extension(name: &str, measure: Measure) -> (&str, &str) {
    match name.rfind('.') {
        Some(dot) if dot > 0 => {
            let (stem, ext) = name.split_at(dot);
            let ok = measure.of(ext) <= MAX_EXTENSION
                && ext.len() > 1
                && !ext.chars().any(char::is_whitespace);
            if ok { (stem, ext) } else { (name, "") }
        }
        _ => (name, ""),
    }
}

/// Fits a sanitized name into the limits for `dir`, keeping room for [`SUFFIX_RESERVE`].
///
/// The budget is the component limit minus the reserve, reduced further so the whole path stays
/// within [`Limits::path`] when the directory leaves room for at least 32 units. Truncation cuts
/// the stem at grapheme-cluster boundaries (a Bangla conjunct or an emoji sequence is never
/// split), keeps the extension and trims trailing dots and spaces again.
pub(crate) fn fit(name: &str, dir: &Path, limits: Limits) -> String {
    let m = limits.measure;
    let component = limits.component.saturating_sub(SUFFIX_RESERVE);
    let dir_len = m.of(&dir.to_string_lossy());
    let path_room = limits
        .path
        .saturating_sub(dir_len)
        .saturating_sub(1)
        .saturating_sub(SUFFIX_RESERVE);
    let budget = component.min(path_room.max(MIN_NAME));
    if m.of(name) <= budget {
        return name.to_owned();
    }
    let (stem, ext) = split_extension(name, m);
    let room = budget.saturating_sub(m.of(ext));
    let mut kept = String::new();
    for g in stem.graphemes(true) {
        if m.of(&kept).saturating_add(m.of(g)) > room {
            break;
        }
        kept.push_str(g);
    }
    let kept = kept.trim_end_matches(|c: char| c == '.' || c.is_whitespace());
    if kept.is_empty() {
        return format!("{DEFAULT_NAME}{ext}");
    }
    format!("{kept}{ext}")
}

/// Comparison key for collisions: NFC and lower case, because Windows and macOS volumes are
/// case-insensitive by default and APFS ignores normalization differences.
pub(crate) fn key(name: &str) -> String {
    nfc(name).to_lowercase()
}

fn present(path: &Path) -> bool {
    // `symlink_metadata`: a dangling link still occupies the name.
    std::fs::symlink_metadata(path).is_ok()
}

/// A name in `dir` that collides with nothing: no existing entry (the file system decides case
/// and normalization), no other job's name (`taken`, built with [`key`]) and no partial or
/// control file of that name. `own_staging` is the job's own staging name, whose partial file may
/// of course exist. Collisions get ` (1)`, ` (2)`, … before the extension, within the reserve.
pub(crate) fn unique(
    dir: &Path,
    name: &str,
    taken: &HashSet<String>,
    own_staging: Option<&str>,
    limits: Limits,
) -> String {
    let own = own_staging.map(key);
    let free = |candidate: &str| {
        let k = key(candidate);
        !taken.contains(&k)
            && !present(&dir.join(candidate))
            && (own.as_deref() == Some(k.as_str())
                || CONTROL_SUFFIXES
                    .iter()
                    .all(|s| !present(&dir.join(format!("{candidate}{s}")))))
    };
    if free(name) {
        return name.to_owned();
    }
    let (stem, ext) = split_extension(name, limits.measure);
    (1u32..10_000)
        .map(|n| format!("{stem} ({n}){ext}"))
        .find(|c| free(c))
        .unwrap_or_else(|| format!("{stem} ({}){ext}", crate::core::random_hex(4)))
}

#[cfg(test)]
mod tests;
