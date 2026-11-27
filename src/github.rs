//! The GitHub side: reading a repository's releases, and downloading one
//! of its assets with progress.
//!
//! Everything here runs on worker threads and reports back through a
//! channel, so the window never waits on the network. A shared `ureq::Agent`
//! reuses TLS/HTTP keep-alive connections, and `ETag` / `If-None-Match`
//! avoids burning the 60 req/hr anonymous quota when nothing changed.

use serde_json::Value;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

pub const UA: &str = concat!("GitupLauncher/", env!("CARGO_PKG_VERSION"));

static AGENT: OnceLock<ureq::Agent> = OnceLock::new();

/// One shared agent per process to reuse keep-alive connections.
fn agent() -> &'static ureq::Agent {
    AGENT.get_or_init(|| {
        ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(15))
            .timeout_read(Duration::from_secs(30))
            .try_proxy_from_env(true)
            .build()
    })
}

/// Prefer a user-configured token from Settings, falling back to `GITHUB_TOKEN`.
fn effective_token(token: Option<&str>) -> Option<String> {
    if let Some(t) = token.map(str::trim).filter(|s| !s.is_empty()) {
        return Some(t.to_string());
    }
    std::env::var("GITHUB_TOKEN")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AssetKind {
    Zip,
    Dmg,
}

/// The one file of a release that can be installed on this Mac.
#[derive(Clone)]
pub struct MacAsset {
    pub name: String,
    /// Where the bytes are fetched from. This is the API's own asset
    /// endpoint when the release JSON offers one: it answers with a
    /// redirect straight to GitHub's file store, so the download never
    /// touches github.com — the host that is the first to go missing on
    /// a network where the API still answers.
    pub url: String,
    /// The link a browser would use, kept as a second chance.
    pub browser_url: String,
    #[allow(dead_code)]
    pub size: u64,
    pub kind: AssetKind,
}

/// Result of a conditional `fetch_latest_release` call.
pub enum ReleaseFetch {
    /// New release payload arrived from GitHub (with optional ETag for future 304s).
    Fresh {
        info: ReleaseInfo,
        etag: Option<String>,
    },
    /// GitHub returned `304 Not Modified` — cached release is still current
    /// and this request did not count against the rate limit.
    NotModified,
}

/// The release as plain JSON, so it can be kept on disk and shown again
/// when GitHub cannot be asked.
pub fn to_json(rel: &ReleaseInfo) -> Value {
    let asset = rel.asset.as_ref().map(|a| {
        serde_json::json!({
            "name": a.name,
            "url": a.url,
            "browser": a.browser_url,
            "size": a.size,
            "kind": match a.kind {
                AssetKind::Zip => "zip",
                AssetKind::Dmg => "dmg",
            },
        })
    });
    serde_json::json!({
        "tag": rel.tag,
        "published": rel.published,
        "html_url": rel.html_url,
        "body": rel.body,
        "asset": asset,
    })
}

/// Read one back off the cache.
pub fn from_json(v: &Value) -> Option<ReleaseInfo> {
    let tag = v["tag"].as_str()?.to_string();
    let asset = v.get("asset").and_then(|a| {
        let url = a["url"].as_str()?.to_string();
        let browser_url = a["browser"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| url.clone());
        Some(MacAsset {
            name: a["name"].as_str()?.to_string(),
            url,
            browser_url,
            size: a["size"].as_u64().unwrap_or(0),
            kind: match a["kind"].as_str() {
                Some("dmg") => AssetKind::Dmg,
                _ => AssetKind::Zip,
            },
        })
    });
    Some(ReleaseInfo {
        tag,
        published: v["published"].as_str().unwrap_or("").to_string(),
        html_url: v["html_url"].as_str().unwrap_or("").to_string(),
        body: v["body"].as_str().unwrap_or("").to_string(),
        asset,
    })
}

/// What a repository's newest usable release looks like.
#[derive(Clone)]
pub struct ReleaseInfo {
    /// The tag, e.g. `v2.3.0-macos`.
    pub tag: String,
    /// When it was published, e.g. `2026-11-05T10:19:11Z`.
    pub published: String,
    /// The release's own page on GitHub.
    pub html_url: String,
    /// The notes the release was published with, shown on the detail page.
    pub body: String,
    /// The asset to install, when the release has one for the Mac.
    pub asset: Option<MacAsset>,
}

/// True when the file name looks like an Intel-only Mac build (not universal).
fn is_intel_only(lower: &str) -> bool {
    let has_intel = lower.contains("x86_64")
        || lower.contains("amd64")
        || lower.contains("_x64")
        || lower.contains("-x64")
        || lower.contains(".x64")
        || lower.contains("x64.");
    let has_arm = lower.contains("arm64") || lower.contains("aarch64");
    let universal = lower.contains("universal");
    has_intel && !has_arm && !universal
}

/// True when the file name looks like an Apple Silicon build.
fn is_apple_silicon(lower: &str) -> bool {
    (lower.contains("arm64") || lower.contains("aarch64"))
        && !lower.contains("x86_64")
        && !is_intel_only(lower)
}

/// Score one release asset for installing on Apple Silicon. Higher is better;
/// negative means "skip".
fn score_mac_asset(lower: &str, kind: AssetKind) -> i32 {
    if lower.contains("windows")
        || lower.contains("linux")
        || lower.contains("android")
        || lower.contains("sha256")
        || lower.contains("checksum")
        || lower.ends_with(".txt")
        || lower.ends_with(".json")
        || lower.ends_with(".sig")
        || lower.ends_with(".exe")
        || lower.ends_with(".msi")
        || lower.ends_with(".apk")
        || lower.ends_with(".deb")
        || lower.ends_with(".rpm")
        || lower.ends_with(".appimage")
        || lower.ends_with(".part")
    {
        return -1;
    }

    let mut score = if is_apple_silicon(lower) {
        40
    } else if lower.contains("universal") {
        30
    } else if lower.contains("macos")
        || lower.contains("darwin")
        || lower.contains("-mac")
        || lower.contains("_mac")
        || lower.contains(".mac")
        || lower.starts_with("mac")
    {
        20
    } else if kind == AssetKind::Dmg {
        // Bare `App.dmg` (e.g. UTM.dmg) is usually a Mac-only installer.
        15
    } else {
        0
    };

    // Intel-only builds are a last resort on Apple Silicon.
    if is_intel_only(lower) {
        score = 1;
    }

    // Prefer the full app over brew tarballs / "mini" side builds when tied.
    if lower.contains("homebrew") || lower.contains("-mini-") || lower.contains("_mini_") {
        score -= 10;
    }
    if kind == AssetKind::Zip {
        score += 5;
    }
    score
}

/// The best macOS asset of a release: `None` when there is none.
fn pick_mac_asset(assets: &[Value]) -> Option<MacAsset> {
    let mut best: Option<(i32, MacAsset)> = None;
    for a in assets {
        let Some(name) = a["name"].as_str() else { continue };
        let lower = name.to_lowercase();
        let kind = if lower.ends_with(".zip") {
            AssetKind::Zip
        } else if lower.ends_with(".dmg") {
            AssetKind::Dmg
        } else {
            continue;
        };
        let score = score_mac_asset(&lower, kind);
        if score < 0 {
            continue;
        }
        let browser_url = a["browser_download_url"].as_str().unwrap_or_default();
        if browser_url.is_empty() {
            continue;
        }
        let api_url = a["url"].as_str().unwrap_or_default();
        let url = if api_url.is_empty() {
            browser_url.to_string()
        } else {
            api_url.to_string()
        };
        if best.as_ref().is_none_or(|(s, _)| score > *s) {
            best = Some((
                score,
                MacAsset {
                    name: name.to_string(),
                    url,
                    browser_url: browser_url.to_string(),
                    size: a["size"].as_u64().unwrap_or(0),
                    kind,
                },
            ));
        }
    }
    best.map(|(_, a)| a)
}

/// Among releases that have a Mac asset, pick the newest *versioned* one.
/// Unversioned side tags (e.g. `antigravity-manager`) lose to `v5.0.4`.
fn pick_latest_release(candidates: Vec<ReleaseInfo>) -> Option<ReleaseInfo> {
    if candidates.is_empty() {
        return None;
    }
    let versioned = candidates
        .iter()
        .filter_map(|info| {
            crate::version::parse_version(&info.tag).map(|ver| (ver, info))
        })
        .max_by(|(a, _), (b, _)| a.cmp(b));
    if let Some((_, info)) = versioned {
        return Some(info.clone());
    }
    // Nothing parseable: keep GitHub's newest-first order.
    candidates.into_iter().next()
}

enum ReleasesRaw {
    NotModified,
    Data(Vec<Value>, Option<String>),
}

/// The raw JSON of a repository's releases, newest first, with optional ETag support.
fn releases_json(
    slug: &str,
    per_page: usize,
    etag: Option<&str>,
    token: Option<&str>,
) -> Result<ReleasesRaw, String> {
    let url = format!("https://api.github.com/repos/{slug}/releases?per_page={per_page}");
    let mut req = agent()
        .get(&url)
        .set("User-Agent", UA)
        .set("Accept", "application/vnd.github+json");
    if let Some(t) = effective_token(token) {
        req = req.set("Authorization", &format!("Bearer {t}"));
    }
    if let Some(tag) = etag.map(str::trim).filter(|s| !s.is_empty()) {
        req = req.set("If-None-Match", tag);
    }
    let resp = req.timeout(Duration::from_secs(30)).call().map_err(|e| match e {
        ureq::Error::Status(404, _) => "仓库不存在".to_string(),
        ureq::Error::Status(401, _) => "GitHub Token 无效，请在设置中检查".to_string(),
        ureq::Error::Status(403, _) => "GitHub API 限流，请在设置中配置 Token 或稍后再试".to_string(),
        other => format!("{other}"),
    })?;
    if resp.status() == 304 {
        return Ok(ReleasesRaw::NotModified);
    }
    let new_etag = resp.header("ETag").map(str::to_string);
    let v: Value = resp
        .into_json()
        .map_err(|e| format!("GitHub 返回了无法解析的内容: {e}"))?;
    let arr = v
        .as_array()
        .ok_or_else(|| "GitHub 返回了意外的内容".to_string())?;
    if arr.is_empty() {
        return Err("该仓库还没有发布任何版本".to_string());
    }
    Ok(ReleasesRaw::Data(arr.clone(), new_etag))
}

/// One release, read out of the API's JSON.
fn release_from_json(rel: &Value, include_pre: bool) -> Option<ReleaseInfo> {
    if rel["draft"].as_bool().unwrap_or(true) {
        return None;
    }
    if rel["prerelease"].as_bool().unwrap_or(false) && !include_pre {
        return None;
    }
    let tag = rel["tag_name"].as_str()?;
    if tag.is_empty() {
        return None;
    }
    let asset = rel["assets"]
        .as_array()
        .and_then(|assets| pick_mac_asset(assets));
    Some(ReleaseInfo {
        tag: tag.to_string(),
        published: rel["published_at"].as_str().unwrap_or("").to_string(),
        html_url: rel["html_url"].as_str().unwrap_or_default().to_string(),
        body: rel["body"].as_str().unwrap_or("").to_string(),
        asset,
    })
}

pub fn fetch_latest_release(
    slug: &str,
    include_pre: bool,
    etag: Option<&str>,
    token: Option<&str>,
) -> Result<ReleaseFetch, String> {
    // 30: busy repos interleave many prereleases / side tags before the
    // real versioned macOS build (see Antigravity-Manager).
    match releases_json(slug, 30, etag, token)? {
        ReleasesRaw::NotModified => Ok(ReleaseFetch::NotModified),
        ReleasesRaw::Data(arr, new_etag) => {
            let candidates: Vec<ReleaseInfo> = arr
                .iter()
                .filter_map(|rel| release_from_json(rel, include_pre))
                .filter(|info| info.asset.is_some())
                .collect();
            let Some(info) = pick_latest_release(candidates) else {
                return Err("最新的发布里没有 macOS 安装包".to_string());
            };
            Ok(ReleaseFetch::Fresh {
                info,
                etag: new_etag,
            })
        }
    }
}

/// Every published release, newest first, for the version picker.
pub fn fetch_release_list(
    slug: &str,
    include_pre: bool,
    token: Option<&str>,
) -> Result<Vec<ReleaseInfo>, String> {
    let ReleasesRaw::Data(arr, _) = releases_json(slug, 30, None, token)? else {
        return Err("没有获取到版本列表".to_string());
    };
    let list: Vec<ReleaseInfo> = arr
        .iter()
        .filter_map(|rel| release_from_json(rel, include_pre))
        .collect();
    if list.is_empty() {
        return Err("该仓库还没有发布任何版本".to_string());
    }
    Ok(list)
}

/// The release notes as a list of changes for the detail page.
pub fn changelog_lines(body: &str) -> Vec<String> {
    let clean = |line: &str| -> Option<String> {
        let t = line.trim();
        let text = t
            .strip_prefix("- ")
            .or_else(|| t.strip_prefix("* "))
            .or_else(|| t.strip_prefix("· "))?;
        let text = text.replace("**", "");
        let text = text.trim();
        if text.is_empty() || text.starts_with('[') {
            None
        } else {
            Some(text.to_string())
        }
    };
    let bullets: Vec<String> = body.lines().filter_map(clean).collect();
    if !bullets.is_empty() {
        return bullets;
    }
    let mut summary: Vec<String> = Vec::new();
    for line in body.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') || t.starts_with('|') || t.starts_with('-') {
            continue;
        }
        let t = t.replace("**", "");
        if t.len() < 4 {
            continue;
        }
        summary.push(t);
        if summary.len() >= 3 {
            break;
        }
    }
    if summary.len() == 1 {
        let long = summary.remove(0);
        summary.extend(
            long.split('。')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string() + "。"),
        );
        summary.truncate(4);
    }
    summary
}

/// The repository's own one-line description, when it has one.
pub fn fetch_repo_description(slug: &str, token: Option<&str>) -> Option<String> {
    let url = format!("https://api.github.com/repos/{slug}");
    let mut req = agent()
        .get(&url)
        .set("User-Agent", UA)
        .set("Accept", "application/vnd.github+json");
    if let Some(t) = effective_token(token) {
        req = req.set("Authorization", &format!("Bearer {t}"));
    }
    let resp = req.timeout(Duration::from_secs(20)).call().ok()?;
    let v: Value = resp.into_json().ok()?;
    let text = v["description"].as_str()?.trim().to_string();
    Some(text)
}

/// Stream a file to `dest`, calling `progress` with bytes moved so far and total.
pub fn download_file(
    urls: &[String],
    dest: &Path,
    token: Option<&str>,
    cancel: &AtomicBool,
    progress: impl Fn(u64, u64),
) -> Result<(), String> {
    let mut last = "没有可用的下载地址".to_string();
    for url in urls {
        match download_one(url, dest, token, cancel, &progress) {
            Ok(()) => return Ok(()),
            Err(e) if e == "cancelled" => return Err(e),
            Err(e) => last = e,
        }
    }
    Err(last)
}

fn download_one(
    url: &str,
    dest: &Path,
    token: Option<&str>,
    cancel: &AtomicBool,
    progress: &impl Fn(u64, u64),
) -> Result<(), String> {
    let mut req = agent()
        .get(url)
        .set("User-Agent", UA)
        .set("Accept", "application/octet-stream");
    if let Some(t) = effective_token(token) {
        req = req.set("Authorization", &format!("Bearer {t}"));
    }
    let resp = req.call().map_err(|e| format!("{e}"))?;
    let total: u64 = resp
        .header("Content-Length")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let mut reader = resp.into_reader();
    let mut buf = vec![0u8; 256 * 1024];
    let mut file = std::fs::File::create(dest).map_err(|e| format!("无法写入临时文件: {e}"))?;
    let mut done: u64 = 0;
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = std::fs::remove_file(dest);
            return Err("cancelled".to_string());
        }
        let n = reader.read(&mut buf).map_err(|e| format!("下载中断: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| format!("无法写入临时文件: {e}"))?;
        done += n as u64;
        progress(done, total);
    }
    file.flush().map_err(|e| format!("{e}"))?;
    if total > 0 && done != total {
        let _ = std::fs::remove_file(dest);
        return Err("下载不完整".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn asset(name: &str) -> Value {
        json!({
            "name": name,
            "url": format!("https://api.example/{name}"),
            "browser_download_url": format!("https://example/{name}"),
            "size": 1u64,
        })
    }

    #[test]
    fn picks_aarch64_over_x64() {
        let assets = vec![
            asset("HotDownloader_2.2.0_x64.dmg"),
            asset("HotDownloader_2.2.0_aarch64.dmg"),
        ];
        let picked = pick_mac_asset(&assets).unwrap();
        assert_eq!(picked.name, "HotDownloader_2.2.0_aarch64.dmg");
    }

    #[test]
    fn picks_arm64_macos_dmg() {
        let assets = vec![
            asset("App-windows-x64.exe"),
            asset("App-macos-x86_64.dmg"),
            asset("App-macos-arm64.dmg"),
        ];
        let picked = pick_mac_asset(&assets).unwrap();
        assert_eq!(picked.name, "App-macos-arm64.dmg");
    }

    #[test]
    fn bare_dmg_still_counts() {
        let assets = vec![asset("UTM.dmg"), asset("UTM.ipa")];
        let picked = pick_mac_asset(&assets).unwrap();
        assert_eq!(picked.name, "UTM.dmg");
    }

    #[test]
    fn latest_prefers_versioned_tag() {
        let candidates = vec![
            ReleaseInfo {
                tag: "antigravity-manager".into(),
                published: "2026-11-26T07:54:38Z".into(),
                html_url: String::new(),
                body: String::new(),
                asset: Some(MacAsset {
                    name: "Antigravity.Manager_0.0.3_universal.dmg".into(),
                    url: String::new(),
                    browser_url: String::new(),
                    size: 1,
                    kind: AssetKind::Dmg,
                }),
            },
            ReleaseInfo {
                tag: "v5.0.4".into(),
                published: "2026-11-20T16:25:01Z".into(),
                html_url: String::new(),
                body: String::new(),
                asset: Some(MacAsset {
                    name: "Antigravity.Tools_5.0.4_aarch64.dmg".into(),
                    url: String::new(),
                    browser_url: String::new(),
                    size: 1,
                    kind: AssetKind::Dmg,
                }),
            },
            ReleaseInfo {
                tag: "am-v0.0.3".into(),
                published: "2026-11-26T10:27:28Z".into(),
                html_url: String::new(),
                body: String::new(),
                asset: Some(MacAsset {
                    name: "Antigravity.Manager_0.0.3_aarch64.dmg".into(),
                    url: String::new(),
                    browser_url: String::new(),
                    size: 1,
                    kind: AssetKind::Dmg,
                }),
            },
        ];
        let best = pick_latest_release(candidates).unwrap();
        assert_eq!(best.tag, "v5.0.4");
        assert!(best.asset.unwrap().name.contains("Tools"));
    }

    #[test]
    fn latest_prefers_newer_prerelease_core() {
        let candidates = vec![
            ReleaseInfo {
                tag: "v5.0.5-beta.5".into(),
                published: String::new(),
                html_url: String::new(),
                body: String::new(),
                asset: Some(MacAsset {
                    name: "Tools_aarch64.dmg".into(),
                    url: String::new(),
                    browser_url: String::new(),
                    size: 1,
                    kind: AssetKind::Dmg,
                }),
            },
            ReleaseInfo {
                tag: "v5.0.4".into(),
                published: String::new(),
                html_url: String::new(),
                body: String::new(),
                asset: Some(MacAsset {
                    name: "Tools_5.0.4_aarch64.dmg".into(),
                    url: String::new(),
                    browser_url: String::new(),
                    size: 1,
                    kind: AssetKind::Dmg,
                }),
            },
        ];
        let best = pick_latest_release(candidates).unwrap();
        assert_eq!(best.tag, "v5.0.5-beta.5");
    }
}
