//! Test-only I/O fault injection (B1.4). Compiled into debug builds only; even then it acts only
//! when `DOWNOVA_FAULT` (a comma-separated list) names the point. Each point fires once per
//! process, so the same process can then recover.

/// Returns an injected "storage full" error the first time `point` is reached while `armed`.
pub(crate) fn storage_full(point: &str, armed: bool) -> std::io::Result<()> {
    #[cfg(debug_assertions)]
    if armed && enabled(point) && first_time(point) {
        return Err(std::io::Error::from(std::io::ErrorKind::StorageFull));
    }
    #[cfg(not(debug_assertions))]
    let _ = (point, armed);
    Ok(())
}

#[cfg(debug_assertions)]
fn enabled(point: &str) -> bool {
    std::env::var("DOWNOVA_FAULT").is_ok_and(|v| v.split(',').any(|p| p.trim() == point))
}

#[cfg(debug_assertions)]
fn first_time(point: &str) -> bool {
    use std::collections::HashSet;
    use std::sync::Mutex;
    static FIRED: Mutex<Option<HashSet<String>>> = Mutex::new(None);
    let mut fired = FIRED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    fired
        .get_or_insert_with(HashSet::new)
        .insert(point.to_owned())
}
