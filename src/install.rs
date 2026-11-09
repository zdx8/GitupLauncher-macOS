//! The local side: what is installed, opening it, and putting a fresh
//! download into /Applications.
//!
//! The Mac's own tools do the heavy lifting — PlistBuddy to read a
//! bundle's version, ditto to unpack a zip and to copy a bundle without
//! losing anything, hdiutil to mount a dmg — so nothing here depends on
//! an unsigned helper or a third-party installer.

use crate::github::AssetKind;
use std::path::{Path, PathBuf};
use std::process::Command;

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

/// Where app bundles are looked for.
fn app_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![PathBuf::from("/Applications")];
    if let Ok(home) = std::env::var("HOME") {
        let dir = PathBuf::from(home).join("Applications");
        if dir.is_dir() {
            dirs.push(dir);
        }
    }
    dirs
}

/// The version of the installed `<bundle>.app`, when it is on this Mac.
pub fn installed_version(bundle: &str) -> Option<String> {
    for dir in app_dirs() {
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
pub fn open_app(bundle: &str) -> Result<(), String> {
    for dir in app_dirs() {
        let app = dir.join(format!("{bundle}.app"));
        if app.exists() {
            run(Command::new("open").arg(&app)).map(|_| ())?;
            return Ok(());
        }
    }
    run(Command::new("open").args(["-a", bundle])).map(|_| ())
}

/// Move the installed `<bundle>.app` to the Bin. The subscription is a
/// setting inside the launcher, so it is untouched: the app goes, the entry
/// stays, and it can be installed again from the same card.
pub fn uninstall_app(bundle: &str) -> Result<(), String> {
    for dir in app_dirs() {
        let app = dir.join(format!("{bundle}.app"));
        if !app.exists() {
            continue;
        }
        // Through the Finder, so the bundle lands in the Bin the way a
        // person expects, and can be put back if they change their mind.
        let script = format!(
            "tell application \"Finder\" to delete POSIX file \"{}\"",
            app.display()
        );
        run(Command::new("osascript").arg("-e").arg(&script))?;
        return Ok(());
    }
    Err("没有在本机找到该应用".to_string())
}

/// The installed app's own icon, turned into PNG bytes by sips. The name
/// in the bundle's Info.plist may or may not carry the `.icns` suffix, so
/// both spellings are tried; a bundle without an icon answers `None`.
pub fn bundle_icon_png(bundle: &str) -> Option<Vec<u8>> {
    for dir in app_dirs() {
        let contents = dir.join(format!("{bundle}.app/Contents"));
        let plist = contents.join("Info.plist");
        if !plist.exists() {
            continue;
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
        // sips reads every icon format on the Mac and writes a 256-pixel
        // PNG, which is plenty for the tiles. Its folder must exist
        // first: without it, sips quietly writes somewhere else.
        let dir = std::env::temp_dir().join("gitup-launcher/icons");
        if std::fs::create_dir_all(&dir).is_err() {
            return None;
        }
        let out = dir.join(format!("{bundle}.png"));
        let converted = run(
            Command::new("sips")
                .arg("-s")
                .arg("format")
                .arg("png")
                .arg("--resampleWidth")
                .arg("256")
                .arg(&icon)
                .arg("--out")
                .arg(&out),
        );
        if converted.is_err() {
            continue;
        }
        return std::fs::read(&out).ok();
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
                if path.file_stem().and_then(|s| s.to_str()) == Some(bundle) {
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

/// Put the app inside a downloaded zip or dmg into /Applications, replacing
/// the copy that is already there. The old copy is moved aside first and
/// put back if anything goes wrong, so a half-done update never leaves the
/// Mac without the app.
pub fn install_asset(file: &Path, kind: AssetKind, bundle: &str) -> Result<(), String> {
    let tmp = std::env::temp_dir().join("gitup-launcher/install");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| format!("无法创建临时目录: {e}"))?;

    let mut mount: Option<PathBuf> = None;
    let src_app = match kind {
        AssetKind::Zip => {
            let out = tmp.join("unzipped");
            std::fs::create_dir_all(&out).map_err(|e| format!("{e}"))?;
            let unzip = run(Command::new("ditto").arg("-x").arg("-k").arg(file).arg(&out));
            if let Err(e) = unzip {
                return Err(format!("解压失败: {e}"));
            }
            match find_app(&out, bundle) {
                Some(app) => app,
                None => return Err("安装包里没有找到 .app".to_string()),
            }
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
            match find_app(&mnt, bundle) {
                Some(app) => app,
                None => {
                    let _ = run(Command::new("hdiutil").arg("detach").arg(&mnt));
                    return Err("安装包里没有找到 .app".to_string());
                }
            }
        }
    };

    let dst = PathBuf::from("/Applications").join(format!("{bundle}.app"));
    let backup = tmp.join("old.app");
    let had_old = dst.exists();
    if had_old {
        std::fs::rename(&dst, &backup).map_err(|e| format!("无法移开旧版本: {e}"))?;
    }
    let copied = run(Command::new("ditto").arg(&src_app).arg(&dst));
    if let Some(m) = &mount {
        let _ = run(Command::new("hdiutil").arg("detach").arg(m));
    }
    match copied {
        Ok(_) => {
            let _ = std::fs::remove_dir_all(&backup);
            Ok(())
        }
        Err(e) => {
            if had_old {
                let _ = std::fs::rename(&backup, &dst);
            }
            Err(format!("拷贝到 /Applications 失败: {e}"))
        }
    }
}
