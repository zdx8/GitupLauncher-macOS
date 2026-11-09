//! Small helpers for comparing and displaying version numbers and dates.
//!
//! Tags come in several shapes across the managed repos — `v1.1.2`,
//! `spektralab-v2.2.1`, `v2.3.0-macos` — so the parser takes the first
//! run of digits-and-dots it can find and reads up to three numbers out
//! of it.

/// The first `x.y.z` in the string, as three numbers. Missing parts read
/// as zero; anything without a digit at all is `None`.
pub fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() && !bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i >= bytes.len() {
        return None;
    }
    let mut parts = [0u64; 3];
    let mut used = 0usize;
    while i < bytes.len() && used < 3 {
        if bytes[i].is_ascii_digit() {
            let mut n: u64 = 0;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                n = n.saturating_mul(10).saturating_add((bytes[i] - b'0') as u64);
                i += 1;
            }
            parts[used] = n;
            used += 1;
        } else if bytes[i] == b'.' && used > 0 {
            i += 1;
            if i >= bytes.len() || !bytes[i].is_ascii_digit() {
                break;
            }
        } else {
            break;
        }
    }
    if used == 0 {
        None
    } else {
        Some((parts[0], parts[1], parts[2]))
    }
}

/// A tag turned into a clean `x.y.z` for display, without the leading `v`
/// or trailing `-macos` and friends.
pub fn clean_tag(tag: &str) -> String {
    match parse_version(tag) {
        Some((a, b, c)) => format!("{a}.{b}.{c}"),
        None => tag.trim_start_matches('v').to_string(),
    }
}

/// The release date as the cards show it: `2026年11月5日`.
pub fn fmt_date(iso: &str) -> String {
    // `2026-11-05T10:19:11Z`
    let mut parts = iso.splitn(3, '-');
    let (year, month, rest) = match (parts.next(), parts.next(), parts.next()) {
        (Some(y), Some(m), Some(r)) => (y, m, r),
        _ => return "未知".to_string(),
    };
    let day = rest
        .split(|c: char| !c.is_ascii_digit())
        .next()
        .unwrap_or("");
    if year.is_empty() || month.is_empty() || day.is_empty() {
        return "未知".to_string();
    }
    format!(
        "{}年{}月{}日",
        year,
        month.trim_start_matches('0'),
        day.trim_start_matches('0')
    )
}
