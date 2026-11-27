//! The local side: what is installed, opening it, and putting a fresh
//! download into the configured install directory.
//!
//! Uses macOS built-in tools (`PlistBuddy`, `ditto`, `hdiutil`, `sips`,
//! `osascript`). Each installation runs in its own isolated temp directory
//! so concurrent downloads/installs never collide.

use crate::github::AssetKind;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static INSTALL_SEQ: AtomicU64 = AtomicU64::new(1);

/// Run a command and hand back its stdout, or its stderr as the error.
fn run(cmd: &mut Command) -> Result<String, String> {
    let out = cmd
        .output()
        .map_err(|e| format!("无法运行 {}: {e}", cmd.get_program().to_string_lossy()))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Expand a leading `~` or `~/` in a user-configured path.
pub fn expand_install_dir(install_dir: &str) -> PathBuf {
    let trimmed = install_dir.trim();
    if trimmed.is_empty() {
        return PathBuf::from("/Applications");
    }
    if let Ok(home) = std::env::var("HOME") {
        if trimmed == "~" {
            return PathBuf::from(home);
        }
        if let Some(rest) = trimmed.strip_prefix("~/") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(trimmed)
}

/// Where app bundles are looked for: configured install dir first, then
/// `/Applications` and `~/Applications` (deduplicated).
fn app_dirs(install_dir: &str) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut push_unique = |p: PathBuf| {
        if !dirs.iter().any(|d| d == &p) {
            dirs.push(p);
        }
    };
    push_unique(expand_install_dir(install_dir));
    push_unique(PathBuf::from("/Applications"));
    if let Ok(home) = std::env::var("HOME") {
        let dir = PathBuf::from(home).join("Applications");
        if dir.is_dir() {
            push_unique(dir);
        }
    }
    dirs
}

/// Escape a string for safe embedding inside AppleScript double quotes.
fn escape_applescript(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Gently ask a running app to quit before replacing or moving it to the Bin.
fn quit_running_app(bundle: &str) {
    let escaped = escape_applescript(bundle);
    let script = format!(
        "if application \"{escaped}\" is running then tell application \"{escaped}\" to quit"
    );
    if run(Command::new("osascript").arg("-e").arg(&script)).is_ok() {
        std::thread::sleep(std::time::Duration::from_millis(350));
    }
}

/// Letters and digits only, lowercased — used to compare bundle names that
/// differ only by spaces, hyphens or case (`Snow Shot` ↔ `snow-shot`).
fn normalize_bundle_key(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

fn plist_string(plist: &Path, key: &str) -> Option<String> {
    let v = run(Command::new("/usr/libexec/PlistBuddy")
        .arg("-c")
        .arg(format!("Print :{key}"))
        .arg(plist))
    .ok()?;
    if v.is_empty() {
        None
    } else {
        Some(v)
    }
}

fn version_from_plist(plist: &Path) -> Option<String> {
    for key in ["CFBundleShortVersionString", "CFBundleVersion"] {
        if let Some(v) = plist_string(plist, key) {
            return Some(v);
        }
    }
    None
}

/// An installed `.app` resolved from an expected bundle name (and optional
/// `owner/repo` slug, which helps pick the right app when names diverged —
/// e.g. subscription `Antigravity-Manager` → `Antigravity Tools.app`).
#[derive(Clone, Debug)]
pub struct ResolvedApp {
    pub path: PathBuf,
    /// Folder stem without `.app`.
    pub stem: String,
}

/// Locate `<bundle>.app` under the install directories. Exact match first, then
/// case / punctuation-insensitive name match, then a slug-aware scan of
/// Info.plist identity fields.
pub fn resolve_app(bundle: &str, install_dir: &str, slug: Option<&str>) -> Option<ResolvedApp> {
    if bundle.is_empty() {
        return None;
    }
    for dir in app_dirs(install_dir) {
        let exact = dir.join(format!("{bundle}.app"));
        if exact.join("Contents/Info.plist").exists() {
            return Some(ResolvedApp {
                path: exact,
                stem: bundle.to_string(),
            });
        }
    }

    let want = normalize_bundle_key(bundle);
    if want.is_empty() {
        return None;
    }
    let owner_key = slug
        .and_then(|s| s.split_once('/'))
        .map(|(o, _)| normalize_bundle_key(o))
        .filter(|s| !s.is_empty());
    let repo_key = slug
        .and_then(|s| s.split_once('/'))
        .map(|(_, r)| {
            let r = r
                .trim_end_matches("-macOS")
                .trim_end_matches("-macos")
                .trim_end_matches("-MacOS");
            normalize_bundle_key(r)
        })
        .filter(|s| !s.is_empty());

    let mut best: Option<(i32, ResolvedApp)> = None;
    for dir in app_dirs(install_dir) {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("app") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let plist = path.join("Contents/Info.plist");
            if !plist.exists() {
                continue;
            }
            let stem_key = normalize_bundle_key(stem);
            let name = plist_string(&plist, "CFBundleName").unwrap_or_default();
            let display = plist_string(&plist, "CFBundleDisplayName").unwrap_or_default();
            let ident = plist_string(&plist, "CFBundleIdentifier")
                .unwrap_or_default()
                .to_ascii_lowercase();
            let name_key = normalize_bundle_key(&name);
            let display_key = normalize_bundle_key(&display);

            let common_prefix = |a: &str, b: &str| -> usize {
                a.chars()
                    .zip(b.chars())
                    .take_while(|(x, y)| x == y)
                    .count()
            };

            let mut score = 0i32;
            if stem_key == want || name_key == want || display_key == want {
                score = 100;
            } else {
                let overlap = common_prefix(&stem_key, &want)
                    .max(common_prefix(&name_key, &want))
                    .max(common_prefix(&display_key, &want));
                // Shared product prefix (`Antigravity…`), but not a full match.
                if overlap >= 8 {
                    score = 40 + overlap as i32;
                }
            }
            if let Some(repo) = repo_key.as_ref() {
                if stem_key.contains(repo.as_str())
                    || name_key.contains(repo.as_str())
                    || display_key.contains(repo.as_str())
                {
                    score = score.max(80);
                }
            }
            // Owner in the bundle id is the strongest signal for custom
            // subscriptions whose folder was renamed (Manager → Tools).
            if let Some(owner) = owner_key.as_ref() {
                if ident.contains(owner) {
                    score += 50;
                } else if score < 100 {
                    // A same-named unrelated app (e.g. Google Antigravity)
                    // must not beat the publisher's own build.
                    score -= 60;
                }
            }

            if score < 55 {
                continue;
            }
            let candidate = ResolvedApp {
                path: path.clone(),
                stem: stem.to_string(),
            };
            if best.as_ref().is_none_or(|(s, _)| score > *s) {
                best = Some((score, candidate));
            }
        }
    }
    best.map(|(_, app)| app)
}

/// The version of the installed app matching `bundle` (with optional slug hint).
pub fn installed_version(bundle: &str, install_dir: &str) -> Option<String> {
    installed_version_for(bundle, install_dir, None)
}

/// Like [`installed_version`], but uses `owner/repo` to disambiguate renames.
pub fn installed_version_for(
    bundle: &str,
    install_dir: &str,
    slug: Option<&str>,
) -> Option<String> {
    let app = resolve_app(bundle, install_dir, slug)?;
    version_from_plist(&app.path.join("Contents/Info.plist"))
}

/// Resolve and, when the on-disk stem differs from `bundle`, return that stem
/// so the caller can record a `bundle_overrides` entry.
pub fn discover_bundle_stem(bundle: &str, install_dir: &str, slug: &str) -> Option<String> {
    let app = resolve_app(bundle, install_dir, Some(slug))?;
    if app.stem != bundle {
        Some(app.stem)
    } else {
        None
    }
}

/// Open the installed app, by path first and by name as a fallback.
pub fn open_app(bundle: &str, install_dir: &str) -> Result<(), String> {
    if let Some(app) = resolve_app(bundle, install_dir, None) {
        return run(Command::new("open").arg(&app.path)).map(|_| ());
    }
    run(Command::new("open").args(["-a", bundle])).map(|_| ())
}

/// Move the installed `<bundle>.app` to the Bin.
pub fn uninstall_app(bundle: &str, install_dir: &str) -> Result<(), String> {
    let Some(app) = resolve_app(bundle, install_dir, None) else {
        return Err("没有在本机找到该应用".to_string());
    };
    trash_app_at(&app.path)
}

/// Move one `.app` bundle to the Bin (Finder trash).
fn trash_app_at(app: &Path) -> Result<(), String> {
    let stem = app
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("app");
    quit_running_app(stem);
    let escaped = escape_applescript(&app.display().to_string());
    let script = format!("tell application \"Finder\" to delete POSIX file \"{escaped}\"");
    run(Command::new("osascript").arg("-e").arg(&script))?;
    Ok(())
}

/// Every `.app` under the install dirs whose `CFBundleIdentifier` matches.
fn apps_with_identifier(identifier: &str, install_dir: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    if identifier.is_empty() {
        return found;
    }
    for dir in app_dirs(install_dir) {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("app") {
                continue;
            }
            let plist = path.join("Contents/Info.plist");
            if plist_string(&plist, "CFBundleIdentifier").as_deref() == Some(identifier) {
                found.push(path);
            }
        }
    }
    found
}

fn path_version_key(app: &Path) -> Option<crate::version::Version> {
    let plist = app.join("Contents/Info.plist");
    let ver = version_from_plist(&plist)?;
    crate::version::parse_version(&ver)
}

/// After a rename (`snow-shot.app` → `Snow Shot.app`), leftover copies that
/// share the same `CFBundleIdentifier` are moved to the Bin.
///
/// Keeps `prefer` when it is one of the copies; otherwise keeps the highest
/// version. Returns the paths that were trashed.
pub fn dedupe_same_identifier(install_dir: &str, prefer: Option<&Path>) -> Vec<PathBuf> {
    let mut by_id: std::collections::BTreeMap<String, Vec<PathBuf>> =
        std::collections::BTreeMap::new();
    for dir in app_dirs(install_dir) {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("app") {
                continue;
            }
            let plist = path.join("Contents/Info.plist");
            let Some(id) = plist_string(&plist, "CFBundleIdentifier") else {
                continue;
            };
            by_id.entry(id).or_default().push(path);
        }
    }

    let mut removed = Vec::new();
    for (_, paths) in by_id {
        if paths.len() < 2 {
            continue;
        }
        let keep = prefer
            .filter(|p| paths.iter().any(|c| c == *p))
            .map(|p| p.to_path_buf())
            .or_else(|| {
                paths
                    .iter()
                    .max_by(|a, b| match (path_version_key(a), path_version_key(b)) {
                        (Some(x), Some(y)) => x.cmp(&y),
                        (Some(_), None) => std::cmp::Ordering::Greater,
                        (None, Some(_)) => std::cmp::Ordering::Less,
                        (None, None) => a.cmp(b),
                    })
                    .cloned()
            });
        let Some(keep_path) = keep else {
            continue;
        };
        for old in &paths {
            if old == &keep_path {
                continue;
            }
            if trash_app_at(old).is_ok() {
                removed.push(old.clone());
            }
        }
    }
    removed
}

/// Remove other installs that share `keep`'s bundle identifier.
fn trash_other_copies(keep: &Path, install_dir: &str) {
    let plist = keep.join("Contents/Info.plist");
    let Some(ident) = plist_string(&plist, "CFBundleIdentifier") else {
        return;
    };
    for other in apps_with_identifier(&ident, install_dir) {
        if other != keep {
            let _ = trash_app_at(&other);
        }
    }
}

/// Directory for cached 256px PNG icons so startup doesn't re-run `sips`.
fn icon_cache_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join("Library/Caches/GitupLauncher/icons")
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Candidate `.icns` paths inside an app bundle, in preference order.
fn icon_candidates(app_dir: &Path) -> Vec<PathBuf> {
    let contents = app_dir.join("Contents");
    let plist = contents.join("Info.plist");
    let resources = contents.join("Resources");
    let mut out = Vec::new();
    let mut push = |p: PathBuf| {
        if p.exists() && !out.contains(&p) {
            out.push(p);
        }
    };
    for key in ["CFBundleIconFile", "CFBundleIconName"] {
        if let Some(name) = plist_string(&plist, key) {
            push(resources.join(&name));
            if !name.ends_with(".icns") {
                push(resources.join(format!("{name}.icns")));
            }
        }
    }
    for preferred in ["AppIcon.icns", "app.icns", "icon.icns", "Icon.icns"] {
        push(resources.join(preferred));
    }
    if let Ok(entries) = std::fs::read_dir(&resources) {
        let mut rest: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("icns"))
            .collect();
        rest.sort();
        for p in rest {
            push(p);
        }
    }
    out
}

/// The installed app's own icon, turned into PNG bytes by sips and cached on
/// disk keyed by bundle mtime so subsequent launches load instantly.
/// `slug` (`owner/repo`) helps find apps whose folder name no longer matches
/// the subscription (e.g. `Antigravity-Manager` → `Antigravity Tools.app`).
pub fn bundle_icon_png_for(
    bundle: &str,
    install_dir: &str,
    slug: Option<&str>,
) -> Option<Vec<u8>> {
    let app = resolve_app(bundle, install_dir, slug)?;
    let plist = app.path.join("Contents/Info.plist");
    let mtime = std::fs::metadata(&plist)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let cache_dir = icon_cache_dir();
    // Cache under the *resolved* stem so a rename still hits after discovery.
    let cached_png = cache_dir.join(format!("{}-{mtime}.png", sanitize_filename(&app.stem)));
    if let Ok(bytes) = std::fs::read(&cached_png) {
        if !bytes.is_empty() {
            return Some(bytes);
        }
    }
    if std::fs::create_dir_all(&cache_dir).is_err() {
        return None;
    }
    for icon in icon_candidates(&app.path) {
        let converted = run(
            Command::new("sips")
                .arg("-s")
                .arg("format")
                .arg("png")
                .arg("--resampleWidth")
                .arg("256")
                .arg(&icon)
                .arg("--out")
                .arg(&cached_png),
        );
        if converted.is_ok() {
            if let Ok(bytes) = std::fs::read(&cached_png) {
                if !bytes.is_empty() {
                    return Some(bytes);
                }
            }
        }
    }
    None
}

/// The first `.app` bundle under `root`, preferring one whose name matches
/// the expected bundle. It never walks into a bundle it has already found.
fn find_app(root: &Path, bundle: &str) -> Option<PathBuf> {
    let mut fallback: Option<PathBuf> = None;
    let mut queue: Vec<(PathBuf, u32)> = vec![(root.to_path_buf(), 0)];
    while let Some((dir, depth)) = queue.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else { continue };
            if !kind.is_dir() {
                continue;
            }
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("app") {
                if path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| s.eq_ignore_ascii_case(bundle))
                {
                    return Some(path);
                }
                fallback.get_or_insert(path);
            } else if depth < 4 {
                queue.push((path, depth + 1));
            }
        }
    }
    fallback
}

/// Put the app inside a downloaded zip or dmg into the target folder,
/// replacing any existing copy safely with rollback on failure.
///
/// Each invocation uses its own isolated temp directory so concurrent
/// installations never interfere with one another. Returns the actual
/// installed bundle stem (without `.app`).
pub fn install_asset(
    file: &Path,
    kind: AssetKind,
    expected_bundle: &str,
    install_dir: &str,
    preserve_archive_name: bool,
) -> Result<String, String> {
    let seq = INSTALL_SEQ.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = std::env::temp_dir()
        .join("gitup-launcher/install")
        .join(format!("{}-{seq}-{nanos}", sanitize_filename(expected_bundle)));
    std::fs::create_dir_all(&tmp).map_err(|e| format!("无法创建临时目录: {e}"))?;

    let mut mount: Option<PathBuf> = None;
    let result = (|| -> Result<String, String> {
        let src_app = match kind {
            AssetKind::Zip => {
                let out = tmp.join("unzipped");
                std::fs::create_dir_all(&out).map_err(|e| format!("{e}"))?;
                let unzip = run(Command::new("ditto").arg("-x").arg("-k").arg(file).arg(&out));
                if let Err(e) = unzip {
                    return Err(format!("解压失败: {e}"));
                }
                find_app(&out, expected_bundle)
                    .ok_or_else(|| "安装包里没有找到 .app".to_string())?
            }
            AssetKind::Dmg => {
                let mnt = tmp.join("mnt");
                std::fs::create_dir_all(&mnt).map_err(|e| format!("{e}"))?;
                let attach = run(
                    Command::new("hdiutil")
                        .arg("attach")
                        .arg("-nobrowse")
                        .arg("-readonly")
                        .arg("-mountpoint")
                        .arg(&mnt)
                        .arg(file),
                );
                if let Err(e) = attach {
                    return Err(format!("挂载安装包失败: {e}"));
                }
                mount = Some(mnt.clone());
                find_app(&mnt, expected_bundle)
                    .ok_or_else(|| "安装包里没有找到 .app".to_string())?
            }
        };

        let actual_bundle = if preserve_archive_name || expected_bundle.is_empty() {
            src_app
                .file_stem()
                .and_then(|s| s.to_str())
                .filter(|s| !s.is_empty())
                .unwrap_or(expected_bundle)
                .to_string()
        } else {
            expected_bundle.to_string()
        };

        // Prefer the folder of an already-installed copy with the same
        // CFBundleIdentifier (even under a different `.app` name), so an
        // update does not land next to the old rename and leave two apps.
        let src_ident =
            plist_string(&src_app.join("Contents/Info.plist"), "CFBundleIdentifier");
        let existing = src_ident
            .as_deref()
            .map(|id| apps_with_identifier(id, install_dir))
            .unwrap_or_default();

        let target_dir = existing
            .iter()
            .find_map(|p| p.parent().map(|d| d.to_path_buf()))
            .or_else(|| {
                app_dirs(install_dir)
                    .into_iter()
                    .find(|d| d.join(format!("{actual_bundle}.app")).exists())
            })
            .unwrap_or_else(|| expand_install_dir(install_dir));
        if !target_dir.exists() {
            std::fs::create_dir_all(&target_dir)
                .map_err(|e| format!("无法创建安装目录 {}: {e}", target_dir.display()))?;
        }

        let dst = target_dir.join(format!("{actual_bundle}.app"));
        let backup = tmp.join("old.app");
        let had_old = dst.exists();
        if had_old {
            quit_running_app(&actual_bundle);
            std::fs::rename(&dst, &backup).map_err(|e| format!("无法移开旧版本: {e}"))?;
        }
        // Quit any differently named copy we are about to replace, so the
        // new bundle is not blocked by a still-running old process.
        for old in &existing {
            if old != &dst {
                if let Some(stem) = old.file_stem().and_then(|s| s.to_str()) {
                    quit_running_app(stem);
                }
            }
        }
        let copied = run(Command::new("ditto").arg(&src_app).arg(&dst));
        match copied {
            Ok(_) => {
                let _ = run(Command::new("xattr").args(["-cr"]).arg(&dst));
                // Drop leftover copies that share this identifier
                // (`snow-shot.app` after installing `Snow Shot.app`).
                trash_other_copies(&dst, install_dir);
                Ok(actual_bundle)
            }
            Err(e) => {
                if had_old {
                    let _ = std::fs::rename(&backup, &dst);
                }
                Err(format!("拷贝到 {} 失败: {e}", target_dir.display()))
            }
        }
    })();

    if let Some(m) = &mount {
        let _ = run(Command::new("hdiutil").arg("detach").arg(m));
    }
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_punctuation() {
        assert_eq!(normalize_bundle_key("Snow Shot"), "snowshot");
        assert_eq!(normalize_bundle_key("snow-shot"), "snowshot");
        assert_eq!(normalize_bundle_key("A7C2Tools"), normalize_bundle_key("a7c2-tools"));
    }

    #[test]
    fn resolves_renamed_antigravity_tools() {
        if !PathBuf::from("/Applications/Antigravity Tools.app").exists() {
            return;
        }
        let app = resolve_app(
            "Antigravity-Manager",
            "/Applications",
            Some("lbjlaq/Antigravity-Manager"),
        )
        .expect("should find Antigravity Tools via slug");
        assert_eq!(app.stem, "Antigravity Tools");
        assert!(bundle_icon_png_for(
            "Antigravity-Manager",
            "/Applications",
            Some("lbjlaq/Antigravity-Manager"),
        )
        .is_some());
    }

    #[test]
    fn does_not_pick_google_antigravity() {
        if !PathBuf::from("/Applications/Antigravity.app").exists() {
            return;
        }
        let app = resolve_app(
            "Antigravity-Manager",
            "/Applications",
            Some("lbjlaq/Antigravity-Manager"),
        );
        if let Some(app) = app {
            assert_ne!(app.stem, "Antigravity");
        }
    }
}
