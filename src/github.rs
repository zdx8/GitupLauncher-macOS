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
    pub url: String,
    /// Kept for the day the button wants to say how big the download is.
    #[allow(dead_code)]
    pub size: u64,
    pub kind: AssetKind,
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
        let url = a["browser_download_url"].as_str().unwrap_or_default();
        if url.is_empty() {
            continue;
        }
        if best.as_ref().is_none_or(|(s, _)| score > *s) {
            best = Some((
                score,
                MacAsset {
                    name: name.to_string(),
                    url: url.to_string(),
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
pub fn fetch_latest_release(slug: &str, include_pre: bool) -> Result<ReleaseInfo, String> {
    let url = format!("https://api.github.com/repos/{slug}/releases?per_page=15");
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
    for rel in arr {
        if rel["draft"].as_bool().unwrap_or(true) {
            continue;
        }
        if rel["prerelease"].as_bool().unwrap_or(false) && !include_pre {
            continue;
        }
        let Some(tag) = rel["tag_name"].as_str() else { continue };
        if tag.is_empty() {
            continue;
        }
        let Some(assets) = rel["assets"].as_array() else { continue };
        let Some(asset) = pick_mac_asset(assets) else { continue };
        return Ok(ReleaseInfo {
            tag: tag.to_string(),
            published: rel["published_at"].as_str().unwrap_or("").to_string(),
            html_url: rel["html_url"].as_str().unwrap_or_default().to_string(),
            body: rel["body"].as_str().unwrap_or("").to_string(),
            asset: Some(asset),
        });
    }
    Err("最新的发布里没有 macOS 安装包".to_string())
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
pub fn download_file(
    url: &str,
    dest: &Path,
    cancel: &AtomicBool,
    progress: impl Fn(u64, u64),
) -> Result<(), String> {
    let resp = agent()
        .get(url)
        .set("User-Agent", UA)
        .call()
        .map_err(|e| format!("{e}"))?;
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
