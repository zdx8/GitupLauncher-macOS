//! The GitHub side: reading a repository's releases, and downloading one
//! of its assets with progress.
//!
//! Everything here runs on worker threads and reports back through a
//! channel, so the window never waits on the network. A `GITHUB_TOKEN`
//! environment variable, when set, is used for the API calls; without it
//! GitHub allows 60 an hour from one address, which is plenty for eight
//! repositories.

use serde_json::Value;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub const UA: &str = concat!("GitupLauncher/", env!("CARGO_PKG_VERSION"));

/// One agent per process is fine; it keeps the connections warm.
fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .try_proxy_from_env(true)
        .build()
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
    /// The link a browser would use, kept as a second chance and for the
    /// day a page wants to offer it.
    pub browser_url: String,
    /// Kept for the day the button wants to say how big the download is.
    #[allow(dead_code)]
    pub size: u64,
    pub kind: AssetKind,
}

/// The release as plain JSON, so it can be kept on disk and shown again
/// when GitHub cannot be asked (the anonymous API allows sixty calls an
/// hour, and a launcher that loses its version numbers when that runs out
/// is no use).
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
        // Caches written before the two links were kept apart have only
        // the browser one, which still works on most networks.
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

/// The best macOS asset of a release: `None` when there is none.
///
/// Windows and Linux builds, checksums and signature files are skipped;
/// arm64 beats universal beats a plain build, and a zip is preferred over
/// a dmg at the same tier because it installs without a mount.
fn pick_mac_asset(assets: &[Value]) -> Option<MacAsset> {
    let mut best: Option<(i32, MacAsset)> = None;
    for a in assets {
        let Some(name) = a["name"].as_str() else { continue };
        let lower = name.to_lowercase();
        if lower.contains("windows") || lower.contains("linux") || lower.contains("sha256")
            || lower.contains("checksum") || lower.ends_with(".txt") || lower.ends_with(".json")
            || lower.ends_with(".sig") || lower.ends_with(".exe") || lower.ends_with(".part")
        {
            continue;
        }
        let kind = if lower.ends_with(".zip") {
            AssetKind::Zip
        } else if lower.ends_with(".dmg") {
            AssetKind::Dmg
        } else {
            continue;
        };
        let mut score = if lower.contains("arm64") && !lower.contains("x86_64") {
            40
        } else if lower.contains("universal") {
            30
        } else if lower.contains("macos") || lower.contains("mac") {
            20
        } else {
            0
        };
        if kind == AssetKind::Zip {
            score += 5;
        }
        let browser_url = a["browser_download_url"].as_str().unwrap_or_default();
        if browser_url.is_empty() {
            continue;
        }
        // The API's own link first: it redirects to the file store without
        // asking github.com, which is the host that fails first.
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

/// The newest release of `owner/repo` that carries something the Mac can
/// install. Repositories that ship their Mac build under its own tag —
/// `v2.3.0-macos` beside a Windows `v2.3.0`, say — are handled because
/// the releases with nothing to install are simply skipped.
/// The raw JSON of a repository's releases, newest first.
fn releases_json(slug: &str, per_page: usize) -> Result<Vec<Value>, String> {
    let url = format!("https://api.github.com/repos/{slug}/releases?per_page={per_page}");
    let mut req = agent()
        .get(&url)
        .set("User-Agent", UA)
        .set("Accept", "application/vnd.github+json");
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        if !token.trim().is_empty() {
            req = req.set("Authorization", &format!("Bearer {}", token.trim()));
        }
    }
    let resp = req.timeout(Duration::from_secs(30)).call().map_err(|e| match e {
        ureq::Error::Status(404, _) => "仓库不存在".to_string(),
        ureq::Error::Status(403, _) => "GitHub API 限流，请稍后再试".to_string(),
        other => format!("{other}"),
    })?;
    let v: Value = resp
        .into_json()
        .map_err(|e| format!("GitHub 返回了无法解析的内容: {e}"))?;
    let arr = v
        .as_array()
        .ok_or_else(|| "GitHub 返回了意外的内容".to_string())?;
    if arr.is_empty() {
        return Err("该仓库还没有发布任何版本".to_string());
    }
    Ok(arr.clone())
}

/// One release, read out of the API's JSON. `None` for drafts, and for
/// prereleases when they are not wanted.
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

pub fn fetch_latest_release(slug: &str, include_pre: bool) -> Result<ReleaseInfo, String> {
    let arr = releases_json(slug, 15)?;
    for rel in &arr {
        if let Some(info) = release_from_json(rel, include_pre) {
            if info.asset.is_some() {
                return Ok(info);
            }
        }
    }
    Err("最新的发布里没有 macOS 安装包".to_string())
}

/// Every published release, newest first, for the version picker. Releases
/// without a Mac asset are kept, with `asset` empty, so the list still
/// reads as a history.
pub fn fetch_release_list(slug: &str, include_pre: bool) -> Result<Vec<ReleaseInfo>, String> {
    let arr = releases_json(slug, 30)?;
    let list: Vec<ReleaseInfo> = arr
        .iter()
        .filter_map(|rel| release_from_json(rel, include_pre))
        .collect();
    if list.is_empty() {
        return Err("该仓库还没有发布任何版本".to_string());
    }
    Ok(list)
}

/// The release notes as a list of changes for the detail page. Bullet
/// lines are kept one per entry; a release without any gets its opening
/// paragraph instead, so the page never reads empty.
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
    // No bullets: the first few plain sentences stand in for them.
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
        summary.extend(long.split("。").map(|s| s.trim()).filter(|s| !s.is_empty()).map(|s| s.to_string() + "。"));
        summary.truncate(4);
    }
    summary
}

/// The repository's own one-line description, when it has one. This is
/// what the cards show under the name: Chinese when the repository
/// writes one, whatever language it uses otherwise.
pub fn fetch_repo_description(slug: &str) -> Option<String> {
    let url = format!("https://api.github.com/repos/{slug}");
    let mut req = agent()
        .get(&url)
        .set("User-Agent", UA)
        .set("Accept", "application/vnd.github+json");
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        if !token.trim().is_empty() {
            req = req.set("Authorization", &format!("Bearer {}", token.trim()));
        }
    }
    let resp = req.timeout(Duration::from_secs(20)).call().ok()?;
    let v: Value = resp.into_json().ok()?;
    let text = v["description"].as_str()?.trim().to_string();
    Some(text)
}

/// Stream a file to `dest`, calling `progress` with the bytes moved so far
/// and the total when it is known. Sets the error string to "cancelled"
/// when `cancel` was raised; the part-file is removed in that case.
///
/// The addresses are tried in turn, so a file can be fetched from the API
/// endpoint when the browser link is unreachable, or the other way round.
pub fn download_file(
    urls: &[String],
    dest: &Path,
    cancel: &AtomicBool,
    progress: impl Fn(u64, u64),
) -> Result<(), String> {
    let mut last = "没有可用的下载地址".to_string();
    for url in urls {
        match download_one(url, dest, cancel, &progress) {
            Ok(()) => return Ok(()),
            // A cancelled download is the person's own doing; trying the
            // next address would ignore that.
            Err(e) if e == "cancelled" => return Err(e),
            Err(e) => last = e,
        }
    }
    Err(last)
}

fn download_one(
    url: &str,
    dest: &Path,
    cancel: &AtomicBool,
    progress: &impl Fn(u64, u64),
) -> Result<(), String> {
    let mut req = agent()
        .get(url)
        .set("User-Agent", UA)
        // Asking for the bytes themselves makes the API answer with a
        // redirect to the file rather than JSON about it.
        .set("Accept", "application/octet-stream");
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        if !token.trim().is_empty() {
            req = req.set("Authorization", &format!("Bearer {}", token.trim()));
        }
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
