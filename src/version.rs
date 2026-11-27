//! Small helpers for comparing and displaying version numbers and dates.
//!
//! Tags come in several shapes across the managed repos — `v1.1.2`,
//! `spektralab-v2.2.1`, `v2.3.0-macos`, `v1.2.0-beta.1` — so the parser
//! extracts the numeric core (`major.minor.patch`) plus an optional
//! prerelease suffix so prereleases always sort before their stable release.

/// Parsed semantic version supporting prerelease ordering:
/// `1.2.0-alpha.1 < 1.2.0-beta.1 < 1.2.0-beta.2 < 1.2.0-rc.1 < 1.2.0`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    pub core: (u64, u64, u64),
    /// `None` means a stable release; `Some(...)` means a prerelease tag.
    pub pre: Option<Vec<PrePart>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PrePart {
    Num(u64),
    Text(String),
}

impl Ord for PrePart {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (self, other) {
            (PrePart::Num(a), PrePart::Num(b)) => a.cmp(b),
            (PrePart::Num(_), PrePart::Text(_)) => std::cmp::Ordering::Less,
            (PrePart::Text(_), PrePart::Num(_)) => std::cmp::Ordering::Greater,
            (PrePart::Text(a), PrePart::Text(b)) => a.cmp(b),
        }
    }
}

impl PartialOrd for PrePart {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match self.core.cmp(&other.core) {
            std::cmp::Ordering::Equal => match (&self.pre, &other.pre) {
                (None, None) => std::cmp::Ordering::Equal,
                // Prerelease is always older than the corresponding stable release.
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (Some(a), Some(b)) => a.cmp(b),
            },
            ord => ord,
        }
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Platform/build suffixes that are NOT prerelease qualifiers.
fn is_platform_suffix(s: &str) -> bool {
    matches!(
        s,
        "macos"
            | "mac"
            | "darwin"
            | "arm64"
            | "aarch64"
            | "universal"
            | "x86_64"
            | "amd64"
            | "x64"
            | "osx"
            | "apple"
    )
}

/// Parse the first `x.y.z` (and optional prerelease identifier) in `s`.
pub fn parse_version(s: &str) -> Option<Version> {
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
        return None;
    }

    // Check for a prerelease suffix such as `-beta.1`, `-rc2`, `-alpha`.
    let tail = s[i..].trim();
    let pre = tail.strip_prefix('-').and_then(|rest| {
        let rest = rest.split('+').next().unwrap_or(rest).trim();
        let lower = rest.to_ascii_lowercase();
        let tokens: Vec<PrePart> = lower
            .split(|c: char| c == '.' || c == '-' || c == '_')
            .filter(|t| !t.is_empty() && !is_platform_suffix(t))
            .map(|t| match t.parse::<u64>() {
                Ok(n) => PrePart::Num(n),
                Err(_) => PrePart::Text(t.to_string()),
            })
            .collect();
        if tokens.is_empty() {
            None
        } else {
            Some(tokens)
        }
    });

    Some(Version {
        core: (parts[0], parts[1], parts[2]),
        pre,
    })
}

/// A tag turned into a clean `x.y.z` (plus prerelease suffix when present) for display.
pub fn clean_tag(tag: &str) -> String {
    match parse_version(tag) {
        Some(v) => {
            let (a, b, c) = v.core;
            match v.pre {
                Some(parts) => {
                    let suffix: Vec<String> = parts
                        .iter()
                        .map(|p| match p {
                            PrePart::Num(n) => n.to_string(),
                            PrePart::Text(t) => t.clone(),
                        })
                        .collect();
                    format!("{a}.{b}.{c}-{}", suffix.join("."))
                }
                None => format!("{a}.{b}.{c}"),
            }
        }
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
