//! Disk caches for release metadata, ETags and repository descriptions.

/// Where the fetched descriptions are kept between runs.
pub(crate) fn descriptions_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    std::path::PathBuf::from(home)
        .join("Library/Application Support/GitupLauncher/descriptions.json")
}

pub(crate) fn load_descriptions() -> std::collections::BTreeMap<String, String> {
    let Ok(text) = std::fs::read_to_string(descriptions_path()) else {
        return std::collections::BTreeMap::new();
    };
    let Ok(map) = serde_json::from_str::<std::collections::BTreeMap<String, String>>(&text) else {
        return std::collections::BTreeMap::new();
    };
    map
}

pub(crate) fn save_descriptions(map: &std::collections::BTreeMap<String, String>) {
    let path = descriptions_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, serde_json::to_string_pretty(map).unwrap_or_default());
}

/// The last release read for each repository, kept between runs.
pub(crate) fn releases_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    std::path::PathBuf::from(home)
        .join("Library/Application Support/GitupLauncher/releases.json")
}

pub(crate) fn load_releases() -> std::collections::BTreeMap<String, serde_json::Value> {
    let Ok(text) = std::fs::read_to_string(releases_path()) else {
        return std::collections::BTreeMap::new();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

pub(crate) fn save_releases(map: &std::collections::BTreeMap<String, serde_json::Value>) {
    let path = releases_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, serde_json::to_string_pretty(map).unwrap_or_default());
}

/// Cached HTTP ETags per repository, kept between runs to save GitHub API quota.
pub(crate) fn etags_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    std::path::PathBuf::from(home)
        .join("Library/Application Support/GitupLauncher/etags.json")
}

pub(crate) fn load_etags() -> std::collections::BTreeMap<String, String> {
    let Ok(text) = std::fs::read_to_string(etags_path()) else {
        return std::collections::BTreeMap::new();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

pub(crate) fn save_etags(map: &std::collections::BTreeMap<String, String>) {
    let path = etags_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, serde_json::to_string_pretty(map).unwrap_or_default());
}

