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

/// The version of the installed `<bundle>.app`, when it is on this Mac.
pub fn installed_version(bundle: &str, install_dir: &str) -> Option<String> {
    for dir in app_dirs(install_dir) {
        let plist = dir.join(format!("{bundle}.app/Contents/Info.plist"));
        if !plist.exists() {
            continue;
        }
        for key in ["CFBundleShortVersionString", "CFBundleVersion"] {
            let ok = run(Command::new("/usr/libexec/PlistBuddy")
                .arg("-c")
                .arg(format!("Print :{key}"))
                .arg(&plist));
            if let Ok(v) = ok {
                if !v.is_empty() {
                    return Some(v);
                }
            }
        }
    }
    None
}

/// Open the installed app, by path first and by name as a fallback.
pub fn open_app(bundle: &str, install_dir: &str) -> Result<(), String> {
    for dir in app_dirs(install_dir) {
        let app = dir.join(format!("{bundle}.app"));
        if app.exists() {
            run(Command::new("open").arg(&app)).map(|_| ())?;
            return Ok(());
        }
    }
    run(Command::new("open").args(["-a", bundle])).map(|_| ())
}

/// Move the installed `<bundle>.app` to the Bin.
pub fn uninstall_app(bundle: &str, install_dir: &str) -> Result<(), String> {
    for dir in app_dirs(install_dir) {
        let app = dir.join(format!("{bundle}.app"));
        if !app.exists() {
            continue;
        }
        quit_running_app(bundle);
        let escaped = escape_applescript(&app.display().to_string());
        let script = format!("tell application \"Finder\" to delete POSIX file \"{escaped}\"");
        run(Command::new("osascript").arg("-e").arg(&script))?;
        return Ok(());
    }
    Err("没有在本机找到该应用".to_string())
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

/// The installed app's own icon, turned into PNG bytes by sips and cached on
/// disk keyed by bundle mtime so subsequent launches load instantly.
pub fn bundle_icon_png(bundle: &str, install_dir: &str) -> Option<Vec<u8>> {
    for dir in app_dirs(install_dir) {
        let app_dir = dir.join(format!("{bundle}.app"));
        let contents = app_dir.join("Contents");
        let plist = contents.join("Info.plist");
        if !plist.exists() {
            continue;
        }
        let mtime = std::fs::metadata(&plist)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let cache_dir = icon_cache_dir();
        let cached_png = cache_dir.join(format!("{}-{mtime}.png", sanitize_filename(bundle)));
        if let Ok(bytes) = std::fs::read(&cached_png) {
            if !bytes.is_empty() {
                return Some(bytes);
            }
        }

        let icon_name = run(Command::new("/usr/libexec/PlistBuddy")
            .arg("-c")
            .arg("Print :CFBundleIconFile")
            .arg(&plist))
        .ok()?;
        if icon_name.is_empty() {
            continue;
        }
        let resources = contents.join("Resources");
        let mut icon = resources.join(&icon_name);
        if !icon.exists() && !icon_name.ends_with(".icns") {
            icon = resources.join(format!("{icon_name}.icns"));
        }
        if !icon.exists() {
            continue;
        }
        if std::fs::create_dir_all(&cache_dir).is_err() {
            return None;
        }
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
        if converted.is_err() {
            continue;
        }
        return std::fs::read(&cached_png).ok();
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

        // Keep existing location when updating; otherwise install to `install_dir`.
        let target_dir = app_dirs(install_dir)
            .into_iter()
            .find(|d| d.join(format!("{actual_bundle}.app")).exists())
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
        let copied = run(Command::new("ditto").arg(&src_app).arg(&dst));
        match copied {
            Ok(_) => {
                let _ = run(Command::new("xattr").args(["-cr"]).arg(&dst));
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
