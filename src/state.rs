//! What is installed, what a repository's newest release looks like, and
//! what the person has set. Everything live arrives through `Msg` from
//! worker threads, and the window redraws as it lands.

use crate::catalog;
use crate::github::{ReleaseFetch, ReleaseInfo};
use std::collections::BTreeMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// How the window is laid out.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    Grid,
    List,
}

/// How the list is sorted.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SortBy {
    Name,
    Updated,
}

/// Whether a download is still moving or already being put in place.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum JobPhase {
    Downloading,
    Installing,
}

/// One running install, as the window sees it.
pub struct Job {
    /// 0.0 .. 1.0, from the real byte counts.
    pub progress: f32,
    /// Bytes a second, smoothed, so the footer can say how fast the
    /// download is really going.
    pub speed: f32,
    /// The byte count and the moment the last measurement was taken.
    pub last: Option<(u64, std::time::Instant)>,
    /// Bytes moved so far, and the size of the file.
    pub done: u64,
    pub total: u64,
    pub phase: JobPhase,
    /// Raised to make the worker thread stop and clean up after itself.
    pub cancel: Arc<AtomicBool>,
}

/// Where a repository's release information stands.
pub enum FetchState {
    /// The worker thread has not answered yet.
    Pending,
    /// Its answer, good or bad.
    Done(Result<ReleaseInfo, String>),
}

/// Everything the window knows about one repository.
pub struct RepoState {
    pub fetch: FetchState,
    /// The version of the copy on this Mac, when there is one.
    pub installed: Option<String>,
    pub job: Option<Job>,
}

/// What the worker threads tell the window.
pub enum Msg {
    Meta {
        idx: usize,
        result: Result<ReleaseFetch, String>,
    },
    Progress {
        idx: usize,
        done: u64,
        total: u64,
    },
    Installing {
        idx: usize,
    },
    Done {
        idx: usize,
        /// The actual `.app` bundle stem installed (recorded for custom repos).
        bundle: Option<String>,
        /// Ok with the version the install ended up at, or Err with why
        /// it stopped ("cancelled" when the person clicked it away).
        result: Result<String, String>,
    },
    /// The whole release list for the version chooser.
    Versions {
        idx: usize,
        result: Result<Vec<crate::github::ReleaseInfo>, String>,
    },
    /// The repository's own description, fetched once and kept on disk.
    Description { slug: String, text: String },
    /// An app's own icon, decoded off the window's thread.
    Icon {
        idx: usize,
        width: u32,
        height: u32,
        uv: (f32, f32, f32, f32),
        rgba: Vec<u8>,
    },
    /// Self-update check result for Gitup Launcher itself.
    SelfUpdateChecked(Result<ReleaseFetch, String>),
    /// Self-update download progress.
    SelfUpdateProgress { done: u64, total: u64 },
    /// Self-update package downloaded; now extracting & staging.
    SelfUpdateStaging,
    /// Self-update package staged and ready to swap & relaunch.
    SelfUpdateReady {
        version: String,
        staged_app: std::path::PathBuf,
        staging_dir: std::path::PathBuf,
    },
    /// Self-update failed or cancelled.
    SelfUpdateError(String),
}

/// Effective `.app` bundle name for a repository, preferring a recorded override.
pub fn effective_bundle<'a>(
    slug: &str,
    default_bundle: &'a str,
    overrides: &'a BTreeMap<String, String>,
) -> &'a str {
    overrides
        .get(slug)
        .map(String::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or(default_bundle)
}

pub struct State {
    /// One entry per repository, in `REPOS` order.
    pub repos: Vec<RepoState>,
    /// Which repositories the person subscribes to, in the same order;
    /// an unsubscribed one is neither fetched nor shown.
    pub subscribed: Vec<bool>,
    pub layout: Layout,
    pub sort: SortBy,
    /// The last thing the footer said.
    pub status: String,
    /// How far the card grid is scrolled down.
    pub scroll: f32,
}

impl State {
    pub fn new() -> Self {
        let list = catalog::repos();
        Self {
            repos: list
                .iter()
                .map(|_| RepoState {
                    fetch: FetchState::Pending,
                    installed: None,
                    job: None,
                })
                .collect(),
            subscribed: list.iter().map(|_| true).collect(),
            layout: Layout::Grid,
            sort: SortBy::Name,
            status: "正在检查更新…".to_string(),
            scroll: 0.0,
        }
    }

    /// Read the installed versions off this Mac. Local and quick, so it
    /// runs on the window's own thread. Uses each repo's slug so a renamed
    /// `.app` (subscription name ≠ folder name) still counts as installed.
    pub fn rescan_installed(&mut self, install_dir: &str, overrides: &BTreeMap<String, String>) {
        for (i, repo) in catalog::repos().iter().enumerate() {
            if i < self.repos.len() {
                let bundle = effective_bundle(repo.slug, repo.bundle, overrides);
                self.repos[i].installed = crate::install::installed_version_for(
                    bundle,
                    install_dir,
                    Some(repo.slug),
                );
            }
        }
    }

    /// Mark every repository as waiting for its answer again.
    pub fn set_all_fetching(&mut self) {
        for repo in &mut self.repos {
            repo.fetch = FetchState::Pending;
        }
    }

    pub fn release(&self, idx: usize) -> Option<&ReleaseInfo> {
        match &self.repos[idx].fetch {
            FetchState::Done(Ok(rel)) => Some(rel),
            _ => None,
        }
    }

    pub fn fetch_error(&self, idx: usize) -> Option<&str> {
        match &self.repos[idx].fetch {
            FetchState::Done(Err(e)) => Some(e.as_str()),
            _ => None,
        }
    }

    pub fn is_fetching(&self, idx: usize) -> bool {
        matches!(self.repos[idx].fetch, FetchState::Pending)
    }

    pub fn is_installed(&self, idx: usize) -> bool {
        self.repos[idx].installed.is_some()
    }

    pub fn busy(&self, idx: usize) -> bool {
        self.repos[idx].job.is_some()
    }

    /// True when the newest release is newer than the installed copy
    /// (including prerelease -> stable upgrades).
    pub fn has_update(&self, idx: usize) -> bool {
        let (Some(installed), Some(rel)) =
            (self.repos[idx].installed.as_deref(), self.release(idx))
        else {
            return false;
        };
        let (Some(here), Some(there)) = (
            crate::version::parse_version(installed),
            crate::version::parse_version(&rel.tag),
        ) else {
            return false;
        };
        here < there
    }

    pub fn all_fetched(&self) -> bool {
        self.repos
            .iter()
            .enumerate()
            .all(|(i, r)| !self.subscribed[i] || !matches!(r.fetch, FetchState::Pending))
    }

    /// Take a worker's answer about one repository, and update the footer
    /// summary once every subscribed repository has answered.
    pub fn apply_meta(&mut self, idx: usize, result: Result<ReleaseInfo, String>) {
        self.repos[idx].fetch = FetchState::Done(result);
        if self.all_fetched() {
            let n = self.repos.len();
            let on = |i: usize| self.subscribed[i];
            let total_on = (0..n).filter(|&i| on(i)).count();
            let updates = (0..n).filter(|&i| on(i) && self.has_update(i)).count();
            let missing = (0..n).filter(|&i| on(i) && !self.is_installed(i)).count();
            let failed = (0..n).filter(|&i| on(i) && self.fetch_error(i).is_some()).count();
            self.status = if total_on > 0 && failed == total_on {
                "GitHub 连接失败（可能已到每小时请求上限），可在设置中配置 Token 或稍后重试"
                    .to_string()
            } else if failed > 0 {
                format!("检查完成：{updates} 个可更新，{missing} 个未安装，{failed} 个仓库获取失败")
            } else if updates + missing == 0 {
                "全部应用均为最新版本".to_string()
            } else {
                format!("检查完成：{updates} 个可更新，{missing} 个未安装")
            };
        }
    }

    pub fn apply_progress(&mut self, idx: usize, done: u64, total: u64) {
        if let Some(job) = self.repos[idx].job.as_mut() {
            job.progress = if total > 0 {
                done as f32 / total as f32
            } else {
                0.5
            };
            job.done = done;
            job.total = total;
            if let Some((last_done, last_at)) = job.last {
                let secs = last_at.elapsed().as_secs_f32();
                if secs > 0.05 && done >= last_done {
                    let instant = (done - last_done) as f32 / secs;
                    job.speed = if job.speed > 0.0 {
                        job.speed * 0.6 + instant * 0.4
                    } else {
                        instant
                    };
                }
            }
            job.last = Some((done, std::time::Instant::now()));
        }
    }

    pub fn apply_installing(&mut self, idx: usize) {
        if let Some(job) = self.repos[idx].job.as_mut() {
            job.phase = JobPhase::Installing;
            job.progress = 1.0;
        }
    }
}

// ---------------------------------------------------------------- settings

/// Which appearance the window follows.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    System,
    Dark,
    Light,
}

impl ThemeMode {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "system" => Some(ThemeMode::System),
            "dark" => Some(ThemeMode::Dark),
            "light" => Some(ThemeMode::Light),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ThemeMode::System => "system",
            ThemeMode::Dark => "dark",
            ThemeMode::Light => "light",
        }
    }
}

/// What the person has set, kept between runs in a small JSON file.
pub struct Settings {
    pub theme: ThemeMode,
    /// Whether the launcher looks for updates on its own when it opens.
    pub auto_check: bool,
    /// Whether Gitup Launcher automatically downloads its own updates in the background.
    pub self_auto_update: bool,
    /// Whether prereleases count as updates.
    pub include_pre: bool,
    /// Where new installs are placed (`/Applications` or `~/Applications` or custom).
    pub install_dir: String,
    /// Optional GitHub Personal Access Token to raise API rate limit from 60 to 5000/hr.
    pub github_token: String,
    /// Recorded actual `.app` bundle stem per custom repo slug.
    pub bundle_overrides: BTreeMap<String, String>,
    /// The ids of the built-in repositories the person has turned off.
    pub disabled: Vec<String>,
    /// The slugs of repositories the person added themselves.
    pub custom: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemeMode::System,
            auto_check: true,
            self_auto_update: true,
            include_pre: false,
            install_dir: "/Applications".to_string(),
            github_token: String::new(),
            bundle_overrides: BTreeMap::new(),
            disabled: Vec::new(),
            custom: Vec::new(),
        }
    }
}

fn settings_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    std::path::PathBuf::from(home)
        .join("Library/Application Support/GitupLauncher/settings.json")
}

pub fn load_settings() -> Settings {
    let Ok(text) = std::fs::read_to_string(settings_path()) else {
        return Settings::default();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Settings::default();
    };
    let mut s = Settings::default();
    if let Some(name) = v["theme"].as_str() {
        if let Some(mode) = ThemeMode::from_name(name) {
            s.theme = mode;
        }
    }
    if let Some(b) = v["auto_check"].as_bool() {
        s.auto_check = b;
    }
    if let Some(b) = v["self_auto_update"].as_bool() {
        s.self_auto_update = b;
    }
    if let Some(b) = v["include_pre"].as_bool() {
        s.include_pre = b;
    }
    if let Some(d) = v["install_dir"].as_str() {
        if !d.is_empty() {
            s.install_dir = d.to_string();
        }
    }
    if let Some(t) = v["github_token"].as_str() {
        s.github_token = t.to_string();
    }
    if let Some(map) = v["bundle_overrides"].as_object() {
        for (k, val) in map {
            if let Some(b) = val.as_str() {
                if !b.is_empty() {
                    s.bundle_overrides.insert(k.clone(), b.to_string());
                }
            }
        }
    }
    if let Some(list) = v["disabled"].as_array() {
        s.disabled = list
            .iter()
            .filter_map(|x| x.as_str().map(|t| t.to_string()))
            .collect();
    }
    if let Some(list) = v["custom"].as_array() {
        s.custom = list
            .iter()
            .filter_map(|x| x.as_str().map(|t| t.to_string()))
            .collect();
    }
    s
}

pub fn save_settings(settings: &Settings) {
    let path = settings_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let v = serde_json::json!({
        "theme": settings.theme.name(),
        "auto_check": settings.auto_check,
        "self_auto_update": settings.self_auto_update,
        "include_pre": settings.include_pre,
        "install_dir": settings.install_dir,
        "github_token": settings.github_token,
        "bundle_overrides": settings.bundle_overrides,
        "disabled": settings.disabled,
        "custom": settings.custom,
    });
    let _ = std::fs::write(&path, serde_json::to_string_pretty(&v).unwrap_or_default());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
}
