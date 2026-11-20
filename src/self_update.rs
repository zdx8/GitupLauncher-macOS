//! Self-update engine for Gitup Launcher itself.
//!
//! Checks `zdx8/GitupLauncher-macOS` on GitHub Releases, downloads and stages the
//! macOS `.app` bundle into a temporary staging directory, and atomically
//! replaces the running `.app` bundle via a detached shell helper before
//! relaunching the new version.

use crate::github::{AssetKind, ReleaseInfo};
use crate::install::expand_install_dir;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// Default GitHub repository slug for Gitup Launcher itself. This is the
/// public repository the releases are actually published to; the shorter
/// `zdx8/GitupLauncher` name does not exist, and pointing the updater at it
/// made every check answer 404.
const DEFAULT_SELF_REPO: &str = "zdx8/GitupLauncher-macOS";

/// The bundle name produced by `bundle.sh`.
const SELF_BUNDLE_NAME: &str = "Gitup Launcher";

/// Repository slug used for self-update checks (can be overridden for testing).
pub fn self_repo_slug() -> String {
    std::env::var("GITUP_SELF_REPO")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| s.contains('/'))
        .unwrap_or_else(|| DEFAULT_SELF_REPO.to_string())
}

/// The currently running app's compiled version from Cargo.toml.
pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Returns the `.app` bundle path if the current executable is running from
/// inside a macOS application bundle (`.../Something.app/Contents/MacOS/bin`).
pub fn current_app_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let macos = exe.parent()?;
    if macos.file_name().and_then(|s| s.to_str()) != Some("MacOS") {
        return None;
    }
    let contents = macos.parent()?;
    if contents.file_name().and_then(|s| s.to_str()) != Some("Contents") {
        return None;
    }
    let bundle = contents.parent()?;
    if bundle.extension().and_then(|s| s.to_str()) == Some("app") {
        Some(bundle.to_path_buf())
    } else {
        None
    }
}

/// Lifecycle state of Gitup Launcher's own auto-updater.
pub enum SelfUpdateState {
    Idle,
    Checking,
    UpToDate,
    Available(ReleaseInfo),
    Downloading {
        done: u64,
        total: u64,
        cancel: Arc<AtomicBool>,
    },
    Staging,
    Ready {
        version: String,
        staged_app: PathBuf,
        staging_dir: PathBuf,
    },
    Error(String),
}

impl SelfUpdateState {
    pub fn is_busy(&self) -> bool {
        matches!(self, Self::Checking | Self::Downloading { .. } | Self::Staging)
    }

    pub fn progress_pct(&self) -> Option<u8> {
        if let Self::Downloading { done, total, .. } = self {
            if *total > 0 {
                let pct = ((*done as f32 / *total as f32) * 100.0)
                    .round()
                    .clamp(0.0, 100.0) as u8;
                return Some(pct);
            }
        }
        None
    }
}

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

fn find_app_bundle(root: &Path) -> Option<PathBuf> {
    let mut fallback: Option<PathBuf> = None;
    let mut queue: Vec<(PathBuf, u32)> = vec![(root.to_path_buf(), 0)];
    while let Some((dir, depth)) = queue.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if !kind.is_dir() {
                continue;
            }
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("app") {
                if path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| s.to_lowercase().contains("gitup"))
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

/// Extracts a downloaded `.zip` or `.dmg` into a clean staging directory and
/// returns `(staged_app_path, staging_dir)`. Any mounted DMG is detached
/// immediately after copying the `.app` out.
pub fn stage_update(file: &Path, kind: AssetKind) -> Result<(PathBuf, PathBuf), String> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let staging_dir = std::env::temp_dir()
        .join("gitup-launcher/self-update")
        .join(format!("stage-{nanos}"));
    std::fs::create_dir_all(&staging_dir).map_err(|e| format!("无法创建更新暂存目录: {e}"))?;

    let mut mount: Option<PathBuf> = None;
    let staged_app = staging_dir.join(format!("{SELF_BUNDLE_NAME}.app"));

    let outcome = (|| -> Result<PathBuf, String> {
        let src_app = match kind {
            AssetKind::Zip => {
                let out = staging_dir.join("unzipped");
                std::fs::create_dir_all(&out).map_err(|e| format!("{e}"))?;
                run(Command::new("ditto").arg("-x").arg("-k").arg(file).arg(&out))
                    .map_err(|e| format!("解压更新包失败: {e}"))?;
                find_app_bundle(&out).ok_or_else(|| "更新包内未找到 .app 文件".to_string())?
            }
            AssetKind::Dmg => {
                let mnt = staging_dir.join("mnt");
                std::fs::create_dir_all(&mnt).map_err(|e| format!("{e}"))?;
                run(Command::new("hdiutil")
                    .arg("attach")
                    .arg("-nobrowse")
                    .arg("-readonly")
                    .arg("-mountpoint")
                    .arg(&mnt)
                    .arg(file))
                .map_err(|e| format!("挂载更新镜像失败: {e}"))?;
                mount = Some(mnt.clone());
                find_app_bundle(&mnt).ok_or_else(|| "更新镜像内未找到 .app 文件".to_string())?
            }
        };

        run(Command::new("ditto").arg(&src_app).arg(&staged_app))
            .map_err(|e| format!("暂存新版本失败: {e}"))?;
        if !staged_app.join("Contents/Info.plist").exists() {
            return Err("暂存的应用包不完整（缺少 Info.plist）".to_string());
        }
        let _ = run(Command::new("xattr").args(["-cr"]).arg(&staged_app));
        Ok(staged_app.clone())
    })();

    if let Some(m) = &mount {
        let _ = run(Command::new("hdiutil").arg("detach").arg(m));
    }
    // Remove intermediate unzipped folder to keep staging small.
    let _ = std::fs::remove_dir_all(staging_dir.join("unzipped"));
    let _ = std::fs::remove_dir_all(staging_dir.join("mnt"));

    match outcome {
        Ok(app_path) => Ok((app_path, staging_dir)),
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging_dir);
            Err(e)
        }
    }
}

/// Spawns a detached shell helper that waits for this process to exit,
/// atomically swaps the staged `.app` into place (with rollback on error),
/// cleans up temporary files, and relaunches the updated app.
pub fn apply_and_relaunch(
    staged_app: &Path,
    staging_dir: &Path,
    install_dir: &str,
) -> Result<(), String> {
    if !staged_app.exists() {
        return Err("暂存的新版本已失效，请重新检查更新".to_string());
    }
    let target_app = current_app_bundle()
        .unwrap_or_else(|| expand_install_dir(install_dir).join(format!("{SELF_BUNDLE_NAME}.app")));
    if let Some(parent) = target_app.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("无法创建目标目录 {}: {e}", parent.display()))?;
        }
    }

    let pid = std::process::id().to_string();
    let script = r#"
pid="$1"
src="$2"
dst="$3"
tmp="$4"
i=0
while kill -0 "$pid" 2>/dev/null && [ "$i" -lt 100 ]; do
  sleep 0.1
  i=$((i + 1))
done
# Still alive after the wait: leave the bundle untouched rather than swap a
# copy the running app is using and start a second one next to it.
if kill -0 "$pid" 2>/dev/null; then
  exit 1
fi
backup="${dst}.old.$$"
rm -rf "$backup"
if [ -e "$dst" ]; then
  mv "$dst" "$backup" || exit 1
fi
if ditto "$src" "$dst"; then
  rm -rf "$backup" "$tmp"
  xattr -cr "$dst" 2>/dev/null || true
  open "$dst"
else
  if [ -e "$backup" ]; then
    # ditto may have left a partial bundle behind; moving the backup over an
    # existing directory would nest it inside instead of restoring it.
    rm -rf "$dst"
    mv "$backup" "$dst"
  fi
  rm -rf "$tmp"
fi
"#;

    Command::new("/bin/sh")
        .arg("-c")
        .arg(script)
        .arg("gitup-self-updater")
        .arg(&pid)
        .arg(staged_app)
        .arg(&target_app)
        .arg(staging_dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("无法启动更新替换脚本: {e}"))?;

    Ok(())
}
