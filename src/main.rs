//! Gitup Launcher — one small window that installs, updates and opens the
//! apps a handful of GitHub repositories publish.
//!
//! The window follows the picture of the original: a slim top bar with the
//! wordmark, a toolbar with sorting and search, a grid of app cards, and a
//! footer strip. Every position was measured off the reference picture,
//! which was 2240 × 1520 pixels for a window of 1120 × 760 points — but
//! everything is now anchored to the window's live edges, so the picture
//! holds at any size: the toolbar rides the right edge, the columns share
//! whatever width there is, and the grid scrolls for real when the cards
//! outgrow the window.
//!
//! The versions, dates and downloads all come from the GitHub Releases
//! API on worker threads, so the window never waits on the network. What
//! is installed is read from the app bundles' own Info.plist. Clicking
//! a card's tile opens that app's own page, with its features and its
//! newest release's changelog.

mod app_icon;
mod catalog;
mod fonts;
mod github;
mod icon;
mod install;
mod modals_ui;
mod self_update;
mod settings_ui;
mod state;
mod theme;
mod version;

use catalog::repos;
use eframe::egui::{
    self, pos2, vec2, Align2, Color32, Rect, Sense, Stroke, StrokeKind, TextEdit, Ui, Vec2,
};
use state::{FetchState, JobPhase, Layout as View, Msg, SortBy, State};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use theme::*;

/// The height of the top bar.
const TOPBAR_H: f32 = 44.0;
/// The height of the toolbar above the card grid.
const TOOLBAR_H: f32 = 50.0;
/// The height of the footer strip.
const FOOTER_H: f32 = 30.0;
/// Where the wordmark starts, clear of the traffic lights.
const WORDMARK_X: f32 = 92.0;
/// A few addresses, for the bulk importer's demo screen.
const SAMPLE_LIST: &str = "https://github.com/diamondfsd/luna-ai-cut
zdx8/NetScope-macOS
# 以 # 开头的行会被跳过
不是链接的一行";

/// The height of the 未安装 heading in the grid.
const SECTION_H: f32 = 32.0;
/// The width of one card's app tile.
const TILE: f32 = 44.0;
/// Balanced card height so 3 full rows fit comfortably in the default window.
const CARD_H: f32 = 192.0;
/// Gap between cards in the grid.
const CARD_GAP: f32 = 12.0;
/// Minimum width of a single card when computing responsive column count.
const CARD_MIN_W: f32 = 274.0;
/// The grid's margin at each side.
pub(crate) const MARGIN: f32 = 20.0;
/// The side margin of the detail page.
pub(crate) const DETAIL_MARGIN: f32 = 20.0;
/// How much room the detail page keeps clear on its right, so its own
/// scrollbar never sits on a button.
pub(crate) const RIGHT_GUTTER: f32 = 32.0;

/// Which of the other screens to show when the window opens.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Demo {
    None,
    List,
    Menu,
    Sort,
    Search,
    Detail,
    Settings,
    Hover,
    Picker,
    Batch,
    BatchRun,
}

/// The importer: one address, or a whole list of them, and what came of
/// the list when it was added.
pub(crate) struct Batch {
    pub(crate) single: String,
    pub(crate) text: String,
    pub(crate) report: Option<String>,
    pub(crate) expanded: bool,
    pub(crate) focus: bool,
}

/// The version chooser: one app's releases, with the one being looked at.
pub(crate) struct Picker {
    pub(crate) idx: usize,
    pub(crate) loading: bool,
    pub(crate) error: Option<String>,
    pub(crate) list: Vec<github::ReleaseInfo>,
    pub(crate) chosen: usize,
    pub(crate) open: bool,
}

/// What the window is showing: the grid, one app's page, or settings.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Page {
    Grid,
    Detail(usize),
    Settings,
}

/// Quick filter pills in the main toolbar.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Filter {
    All,
    Installed,
    Updatable,
}

pub(crate) struct Launcher {
    pub(crate) state: State,
    demo: Demo,
    pending_menu: Option<(Rect, usize)>,
    pending_sort: Option<Rect>,
    search: String,
    filter: Filter,
    pub(crate) menu_for: Option<usize>,
    pub(crate) sort_open: bool,
    tx: std::sync::mpsc::Sender<Msg>,
    rx: std::sync::mpsc::Receiver<Msg>,
    pub(crate) ctx: egui::Context,
    icons: Vec<Option<egui::TextureHandle>>,
    icon_uv: Vec<(f32, f32, f32, f32)>,
    pub(crate) page: Page,
    detail_scroll: f32,
    pub(crate) settings_scroll: f32,
    detail_expanded: bool,
    pub(crate) settings: state::Settings,
    pub(crate) self_update: self_update::SelfUpdateState,
    descriptions: std::collections::BTreeMap<String, String>,
    releases: std::collections::BTreeMap<String, serde_json::Value>,
    /// Cached HTTP ETags per repository slug for 304 Not Modified responses.
    pub(crate) etags: std::collections::BTreeMap<String, String>,
    /// Cached 2-line clamped descriptions keyed by (slug, width_px, summary_len).
    clamp_cache: std::collections::HashMap<(String, u32, usize), String>,
    hover_demo: Option<usize>,
    pending_scroll_end: bool,
    pub(crate) picker: Option<Picker>,
    pub(crate) batch: Option<Batch>,
    window_zoomed: bool,
    search_rect: Rect,
    /// Text input rects on the settings page (token, install_dir) to suppress window drag.
    pub(crate) settings_text_rects: [Rect; 2],
    drag_blocked: bool,
}

impl Launcher {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        fonts::install(&cc.egui_ctx);
        let mut settings = state::load_settings();
        // The override is only for looking at the other appearance here.
        match std::env::var("GITUP_THEME").as_deref() {
            Ok("light") => settings.theme = state::ThemeMode::Light,
            Ok("dark") => settings.theme = state::ThemeMode::Dark,
            _ => {}
        }
        cc.egui_ctx.set_theme(theme_preference(settings.theme));

        // The live list starts as the built-ins plus whatever was added
        // last time, so the window opens with exactly what it should.
        catalog::init_builtin();
        for slug in &settings.custom {
            let _ = catalog::add_custom(slug);
        }

        let (tx, rx) = std::sync::mpsc::channel();
        let mut state = State::new();
        // Local and quick: what is already on this Mac.
        state.rescan_installed(&settings.install_dir, &settings.bundle_overrides);
        let mut launcher = Self {
            state,
            demo: Demo::None,
            pending_menu: None,
            pending_sort: None,
            search: String::new(),
            filter: Filter::All,
            menu_for: None,
            sort_open: false,
            tx,
            rx,
            ctx: cc.egui_ctx.clone(),
            icons: repos().iter().map(|_| None).collect(),
            icon_uv: repos().iter().map(|_| (0.0, 0.0, 1.0, 1.0)).collect(),
            page: Page::Grid,
            detail_scroll: 0.0,
            settings_scroll: 0.0,
            detail_expanded: false,
            settings,
            self_update: self_update::SelfUpdateState::Idle,
            descriptions: load_descriptions(),
            releases: load_releases(),
            etags: load_etags(),
            clamp_cache: std::collections::HashMap::new(),
            hover_demo: None,
            pending_scroll_end: false,
            picker: None,
            batch: None,
            window_zoomed: false,
            search_rect: Rect::NOTHING,
            settings_text_rects: [Rect::NOTHING, Rect::NOTHING],
            drag_blocked: false,
        };
        // A repository the person added is either in the list or deleted,
        // never merely switched off; a stale entry there used to hide it
        // while still answering "already subscribed" when it was added back.
        let custom = launcher.settings.custom.clone();
        launcher
            .settings
            .disabled
            .retain(|d| !custom.iter().any(|c| c == d));
        for (i, repo) in repos().iter().enumerate() {
            launcher.state.subscribed[i] = !launcher.settings.disabled.iter().any(|d| d == repo.id);
            // What was read last time is shown at once, so the versions and
            // dates are never blank while — or if — GitHub stays silent.
            if let Some(rel) = launcher.releases.get(repo.slug).and_then(github::from_json) {
                launcher.state.repos[i].fetch = FetchState::Done(Ok(rel));
            }
        }
        if launcher.settings.auto_check {
            launcher.spawn_meta_fetch_all();
            launcher.check_self_update();
        } else {
            launcher.state.status = "未自动检查更新，点左上角按钮立即检查".to_string();
        }
        launcher.spawn_icon_load();
        launcher
    }

    /// Whether this repository is subscribed, and a toggle that keeps the
    /// setting file in step.
    pub(crate) fn toggle_subscription(&mut self, idx: usize) {
        let id = repos()[idx].id;
        let on = !self.settings.disabled.iter().any(|d| d == id);
        if on {
            self.settings.disabled.push(id.to_string());
            self.state.subscribed[idx] = false;
            self.state.status = format!("已取消订阅 {}", repos()[idx].name);
        } else {
            self.settings.disabled.retain(|d| d != id);
            self.state.subscribed[idx] = true;
            self.state.status = format!("已订阅 {}", repos()[idx].name);
        }
        state::save_settings(&self.settings);
    }

    /// Effective `.app` bundle name for repository `idx`.
    fn bundle_name(&self, idx: usize) -> String {
        let repo = repos()[idx];
        state::effective_bundle(repo.slug, repo.bundle, &self.settings.bundle_overrides).to_string()
    }

    /// Leave whatever pop-ups are open and show an app's page.
    fn open_page(&mut self, idx: usize) {
        self.page = Page::Detail(idx);
        self.detail_scroll = 0.0;
        self.detail_expanded = false;
        self.menu_for = None;
        self.sort_open = false;
    }

    /// One app's tile, wherever it is drawn.
    fn draw_tile(&self, painter: &egui::Painter, tile: Rect, idx: usize, corner: u8) {
        if let Some(texture) = self.icons.get(idx).and_then(|t| t.as_ref()) {
            let (u0, v0, u1, v1) = self.icon_uv[idx];
            painter.image(
                texture.id(),
                tile,
                egui::Rect::from_min_max(pos2(u0, v0), pos2(u1, v1)),
                Color32::WHITE,
            );
        } else {
            let (r, g, b) = catalog::tile_color(repos()[idx].id);
            painter.rect_filled(tile, radius(corner), Color32::from_rgb(r, g, b));
            let letter = repos()[idx].name.chars().next().unwrap_or('?');
            painter.text(
                pos2(tile.center().x, tile.center().y + tile.width() * 0.03),
                Align2::CENTER_CENTER,
                letter.to_string(),
                display(tile.width() * 0.46),
                Color32::WHITE,
            );
        }
    }

    /// Read every installed app's own icon off the UI thread (with disk PNG cache).
    pub(crate) fn spawn_icon_load(&mut self) {
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        let install_dir = self.settings.install_dir.clone();
        let bundles: Vec<(usize, String)> = repos()
            .iter()
            .enumerate()
            .filter(|(i, _)| self.state.is_installed(*i))
            .map(|(i, repo)| {
                let b = state::effective_bundle(
                    repo.slug,
                    repo.bundle,
                    &self.settings.bundle_overrides,
                )
                .to_string();
                (i, b)
            })
            .collect();
        std::thread::spawn(move || {
            for (idx, bundle) in bundles {
                let Some(bytes) = install::bundle_icon_png(&bundle, &install_dir) else {
                    continue;
                };
                if let Ok((width, height, mut rgba)) = icon::decode_png_rgba(&bytes) {
                    icon::clear_white_plate(width, height, &mut rgba);
                    let uv = icon::artwork_uv(width, height, &rgba);
                    let _ = tx.send(Msg::Icon {
                        idx,
                        width,
                        height,
                        uv,
                        rgba,
                    });
                    ctx.request_repaint();
                }
            }
        });
    }

    /// Ask GitHub about every subscribed repository, using ETag conditional requests.
    pub(crate) fn spawn_meta_fetch_all(&mut self) {
        self.state.set_all_fetching();
        self.state.status = "正在检查更新…".to_string();
        let include_pre = self.settings.include_pre;
        let token = if self.settings.github_token.trim().is_empty() {
            None
        } else {
            Some(self.settings.github_token.clone())
        };
        for idx in 0..repos().len() {
            if !self.state.subscribed[idx] {
                continue;
            }
            let tx = self.tx.clone();
            let ctx = self.ctx.clone();
            let slug = repos()[idx].slug.to_string();
            let cached_desc = self.descriptions.contains_key(&slug);
            let etag = if self.releases.contains_key(&slug) {
                self.etags.get(&slug).cloned()
            } else {
                None
            };
            let tok = token.clone();
            std::thread::spawn(move || {
                let result = github::fetch_latest_release(
                    &slug,
                    include_pre,
                    etag.as_deref(),
                    tok.as_deref(),
                );
                let _ = tx.send(Msg::Meta { idx, result });
                if !cached_desc {
                    if let Some(text) = github::fetch_repo_description(&slug, tok.as_deref()) {
                        let _ = tx.send(Msg::Description { slug, text });
                    }
                }
                ctx.request_repaint();
            });
        }
    }

    /// Rebuild every per-row table after the live list has changed shape.
    fn rebuild_repos(&mut self) {
        for repo in self.state.repos.iter() {
            if let Some(job) = &repo.job {
                job.cancel.store(true, Ordering::SeqCst);
            }
        }
        let page = match self.page {
            Page::Detail(_) => Page::Grid,
            other => other,
        };
        let mut fresh = State::new();
        fresh.layout = self.state.layout;
        fresh.sort = self.state.sort;
        for (i, repo) in repos().iter().enumerate() {
            fresh.subscribed[i] = !self.settings.disabled.iter().any(|d| d == repo.id);
        }
        fresh.rescan_installed(&self.settings.install_dir, &self.settings.bundle_overrides);
        self.state = fresh;
        self.icons = repos().iter().map(|_| None).collect();
        self.icon_uv = repos().iter().map(|_| (0.0, 0.0, 1.0, 1.0)).collect();
        self.clamp_cache.clear();
        self.page = page;
        self.spawn_meta_fetch_all();
        self.spawn_icon_load();
    }

    /// Move the installed app to the Bin. The subscription stays.
    pub(crate) fn uninstall(&mut self, idx: usize) {
        if let Some(job) = self.state.repos.get(idx).and_then(|r| r.job.as_ref()) {
            job.cancel.store(true, Ordering::SeqCst);
        }
        let name = repos()[idx].name;
        let bundle = self.bundle_name(idx);
        match install::uninstall_app(&bundle, &self.settings.install_dir) {
            Ok(()) => {
                self.state
                    .rescan_installed(&self.settings.install_dir, &self.settings.bundle_overrides);
                self.state.status = format!("已卸载 {name}（订阅保留）");
            }
            Err(e) => self.state.status = format!("卸载 {name} 失败: {e}"),
        }
    }

    /// Drop the subscription only: whatever is installed stays installed.
    pub(crate) fn delete_subscription(&mut self, idx: usize) {
        let name = repos()[idx].name;
        let builtin = catalog::is_builtin(repos()[idx].slug);
        if builtin {
            self.toggle_subscription(idx);
        } else {
            self.remove_repository(idx);
        }
        self.state.status = format!("已删除订阅 {name}（已安装的 {name}.app 保留）");
    }

    /// Remove a repository the person added. Built-ins are only ever switched off.
    pub(crate) fn remove_repository(&mut self, idx: usize) {
        let list = repos();
        if idx >= list.len() || catalog::is_builtin(list[idx].slug) {
            return;
        }
        if self.state.busy(idx) {
            self.state.status = "该仓库正在下载，请稍后再移除".to_string();
            return;
        }
        let slug = list[idx].slug.to_string();
        let id = list[idx].id.to_string();
        catalog::remove_custom(&slug);
        self.settings.custom.retain(|s| s != &slug);
        self.settings.disabled.retain(|d| d != &id);
        self.settings.bundle_overrides.remove(&slug);
        state::save_settings(&self.settings);
        self.rebuild_repos();
        self.state.status = format!("已移除订阅 {slug}");
    }

    /// The line under an app's name: the repository's own description
    /// (Chinese when it has one), and the built-in text when it has none.
    fn summary_text(&self, idx: usize) -> String {
        let repo = repos()[idx];
        match self.descriptions.get(repo.slug) {
            Some(text) if !text.is_empty() => first_language(text).to_string(),
            _ => repo.summary.to_string(),
        }
    }

    /// The repositories that pass the search box, in the chosen order.
    fn visible(&self) -> Vec<usize> {
        let q = self.search.trim().to_lowercase();
        let mut list: Vec<usize> = (0..repos().len())
            .filter(|&i| self.state.subscribed[i])
            .filter(|&i| match self.filter {
                Filter::All => true,
                Filter::Installed => self.state.is_installed(i),
                Filter::Updatable => self.state.has_update(i),
            })
            .filter(|&i| {
                let a = &repos()[i];
                q.is_empty()
                    || a.name.to_lowercase().contains(&q)
                    || a.tagline.to_lowercase().contains(&q)
                    || a.summary.to_lowercase().contains(&q)
                    || a.slug.to_lowercase().contains(&q)
            })
            .collect();
        match self.state.sort {
            SortBy::Name => list.sort_by(|&a, &b| repos()[a].name.cmp(&repos()[b].name)),
            // Newest first. The dates are ISO strings, so plain text order
            // is date order; a repository with no answer sorts last.
            SortBy::Updated => list.sort_by(|&a, &b| {
                let da = self
                    .state
                    .release(a)
                    .map(|r| r.published.clone())
                    .unwrap_or_default();
                let db = self
                    .state
                    .release(b)
                    .map(|r| r.published.clone())
                    .unwrap_or_default();
                db.cmp(&da)
            }),
        }
        list
    }

    /// What the big button on a card reads for this repository.
    fn button_label(&self, idx: usize) -> String {
        if let Some(job) = self.state.repos[idx].job.as_ref() {
            if job.phase == JobPhase::Installing {
                "正在安装…".to_string()
            } else if job.total > 0 {
                let pct = (job.progress * 100.0).round().clamp(0.0, 100.0) as u8;
                format!("下载中 {pct}%")
            } else {
                "正在下载…".to_string()
            }
        } else if self.state.has_update(idx) {
            "更新".to_string()
        } else if self.state.is_installed(idx) {
            "打开".to_string()
        } else {
            "安装".to_string()
        }
    }

    /// Start updating every subscribed app that currently has an update waiting.
    fn update_all(&mut self) {
        let targets: Vec<usize> = (0..repos().len())
            .filter(|&i| {
                self.state.subscribed[i] && self.state.has_update(i) && !self.state.busy(i)
            })
            .collect();
        if targets.is_empty() {
            self.state.status = "所有已安装应用均为最新版本".to_string();
            return;
        }
        let count = targets.len();
        for idx in targets {
            self.start_job(idx);
        }
        if count > 1 {
            self.state.status = format!("正在下载 {count} 个应用的更新…");
        }
    }

    /// Take whatever the worker threads have said since the last frame.
    fn handle_messages(&mut self) {
        let mut done_seen = false;
        let mut releases_dirty = false;
        let mut etags_dirty = false;
        let mut desc_dirty = false;
        let mut settings_dirty = false;
        while let Ok(msg) = self.rx.try_recv() {
            // A list resize may have left an old answer on the way; it
            // must not land on whatever row happens to sit at that index.
            let idx_ok = |idx: usize, repos: &Vec<state::RepoState>| idx < repos.len();
            match msg {
                Msg::Meta { idx, result } if idx_ok(idx, &self.state.repos) => {
                    let slug = repos()[idx].slug.to_string();
                    match result {
                        Ok(github::ReleaseFetch::Fresh { info, etag }) => {
                            self.releases.insert(slug.clone(), github::to_json(&info));
                            releases_dirty = true;
                            if let Some(tag) = etag {
                                self.etags.insert(slug, tag);
                                etags_dirty = true;
                            }
                            self.state.apply_meta(idx, Ok(info));
                        }
                        Ok(github::ReleaseFetch::NotModified) => {
                            if let Some(rel) = self.releases.get(&slug).and_then(github::from_json)
                            {
                                self.state.apply_meta(idx, Ok(rel));
                            } else {
                                self.etags.remove(&slug);
                                etags_dirty = true;
                            }
                        }
                        Err(e) => match self.releases.get(&slug).and_then(github::from_json) {
                            Some(rel) => {
                                self.state.apply_meta(idx, Ok(rel));
                                self.state.status =
                                    "GitHub 暂时无法访问，显示的是上次读取的版本信息".to_string();
                            }
                            None => self.state.apply_meta(idx, Err(e)),
                        },
                    }
                }
                Msg::Progress { idx, done, total } if idx_ok(idx, &self.state.repos) => {
                    self.state.apply_progress(idx, done, total)
                }
                Msg::Installing { idx } if idx_ok(idx, &self.state.repos) => {
                    self.state.apply_installing(idx)
                }
                Msg::Done { idx, bundle, result } if idx_ok(idx, &self.state.repos) => {
                    let name = repos()[idx].name;
                    match result {
                        Ok(ver) => {
                            if let Some(actual) = bundle {
                                if actual != repos()[idx].bundle {
                                    self.settings
                                        .bundle_overrides
                                        .insert(repos()[idx].slug.to_string(), actual);
                                    settings_dirty = true;
                                }
                            }
                            let was_installed = self.state.repos[idx].installed.is_some();
                            if !ver.is_empty() {
                                self.state.repos[idx].installed = Some(ver);
                            }
                            self.state.repos[idx].job = None;
                            let word = if was_installed { "已更新" } else { "已安装" };
                            let shown = self.state.repos[idx]
                                .installed
                                .clone()
                                .unwrap_or_default();
                            self.state.status = format!("{word} {name} {shown}");
                        }
                        Err(e) => {
                            self.state.repos[idx].job = None;
                            self.state.status = if e == "cancelled" {
                                format!("已取消下载 {name}")
                            } else {
                                format!("安装失败: {e}")
                            };
                        }
                    }
                    done_seen = true;
                }
                Msg::Icon {
                    idx,
                    width,
                    height,
                    uv,
                    rgba,
                } if idx < self.icons.len() => {
                    let image = egui::ColorImage::from_rgba_unmultiplied(
                        [width as usize, height as usize],
                        &rgba,
                    );
                    let texture = self.ctx.load_texture(
                        format!("app-icon-{}", repos()[idx].id),
                        image,
                        egui::TextureOptions::LINEAR,
                    );
                    self.icons[idx] = Some(texture);
                    self.icon_uv[idx] = uv;
                }
                Msg::Versions { idx, result } => {
                    if let Some(picker) = self.picker.as_mut() {
                        if picker.idx == idx {
                            picker.loading = false;
                            match result {
                                Ok(list) => {
                                    picker.list = list;
                                    picker.chosen = 0;
                                }
                                Err(e) => picker.error = Some(e),
                            }
                        }
                    }
                }
                Msg::Description { slug, text } => {
                    self.clamp_cache.retain(|(s, _, _), _| s != &slug);
                    self.descriptions.insert(slug, text);
                    desc_dirty = true;
                }
                Msg::SelfUpdateChecked(res) => match res {
                    Ok(github::ReleaseFetch::Fresh { info, .. }) => {
                        let cur = version::parse_version(self_update::current_version());
                        let latest = version::parse_version(&info.tag);
                        if matches!((cur, latest), (Some(a), Some(b)) if a < b)
                            && info.asset.is_some()
                        {
                            let tag = version::clean_tag(&info.tag).to_uppercase();
                            self.self_update = self_update::SelfUpdateState::Available(info);
                            if self.settings.self_auto_update
                                && self_update::current_app_bundle().is_some()
                            {
                                self.start_self_update_download();
                            } else {
                                self.state.status =
                                    format!("发现 Gitup Launcher 新版本 V{tag}，点击右上角即可更新");
                            }
                        } else {
                            self.self_update = self_update::SelfUpdateState::UpToDate;
                        }
                    }
                    Ok(github::ReleaseFetch::NotModified) => {
                        self.self_update = self_update::SelfUpdateState::UpToDate;
                    }
                    Err(e) => {
                        self.self_update = self_update::SelfUpdateState::Error(e);
                    }
                },
                Msg::SelfUpdateProgress { done, total } => {
                    if let self_update::SelfUpdateState::Downloading {
                        done: d,
                        total: t,
                        ..
                    } = &mut self.self_update
                    {
                        *d = done;
                        *t = total;
                    }
                }
                Msg::SelfUpdateStaging => {
                    if matches!(self.self_update, self_update::SelfUpdateState::Downloading { .. }) {
                        self.self_update = self_update::SelfUpdateState::Staging;
                    }
                }
                Msg::SelfUpdateReady {
                    version,
                    staged_app,
                    staging_dir,
                } => {
                    self.state.status = format!(
                        "Gitup Launcher V{} 已就绪，点击右上角即可重启更新",
                        version.to_uppercase()
                    );
                    self.self_update = self_update::SelfUpdateState::Ready {
                        version,
                        staged_app,
                        staging_dir,
                    };
                }
                Msg::SelfUpdateError(e) => {
                    if e == "cancelled" {
                        self.state.status = "已取消启动器更新".to_string();
                        self.self_update = self_update::SelfUpdateState::Idle;
                    } else {
                        self.state.status = format!("启动器更新失败: {e}");
                        self.self_update = self_update::SelfUpdateState::Error(e);
                    }
                }
                // Anything arriving for a row that no longer exists is
                // simply dropped.
                _ => {}
            }
        }
        if releases_dirty {
            save_releases(&self.releases);
        }
        if etags_dirty {
            save_etags(&self.etags);
        }
        if desc_dirty {
            save_descriptions(&self.descriptions);
        }
        if settings_dirty {
            state::save_settings(&self.settings);
        }
        if done_seen {
            self.state
                .rescan_installed(&self.settings.install_dir, &self.settings.bundle_overrides);
            // A freshly installed app can now show its own icon.
            self.spawn_icon_load();
        }
    }

    /// Check GitHub Releases for a newer version of Gitup Launcher itself.
    pub(crate) fn check_self_update(&mut self) {
        if self.self_update.is_busy() {
            return;
        }
        self.self_update = self_update::SelfUpdateState::Checking;
        let slug = self_update::self_repo_slug();
        let include_pre = self.settings.include_pre;
        let token = if self.settings.github_token.trim().is_empty() {
            None
        } else {
            Some(self.settings.github_token.clone())
        };
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let res = github::fetch_latest_release(&slug, include_pre, None, token.as_deref());
            let _ = tx.send(Msg::SelfUpdateChecked(res));
            ctx.request_repaint();
        });
    }

    /// Download and stage the available self-update release off the UI thread.
    pub(crate) fn start_self_update_download(&mut self) {
        let rel = match &self.self_update {
            self_update::SelfUpdateState::Available(r) => r.clone(),
            _ => return,
        };
        let Some(asset) = rel.asset.clone() else {
            self.self_update =
                self_update::SelfUpdateState::Error("该版本未提供 macOS 安装包".to_string());
            return;
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let version = version::clean_tag(&rel.tag);
        self.self_update = self_update::SelfUpdateState::Downloading {
            done: 0,
            total: asset.size,
            cancel: cancel.clone(),
        };
        let token = if self.settings.github_token.trim().is_empty() {
            None
        } else {
            Some(self.settings.github_token.clone())
        };
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let dir = std::env::temp_dir().join("gitup-launcher/self-update");
            let _ = std::fs::create_dir_all(&dir);
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let file = dir.join(format!("self-{stamp}-{}", asset.name));
            let mut urls = vec![asset.url.clone()];
            if asset.browser_url != asset.url {
                urls.push(asset.browser_url.clone());
            }
            let dl = github::download_file(
                &urls,
                &file,
                token.as_deref(),
                &cancel,
                |done, total| {
                    let _ = tx.send(Msg::SelfUpdateProgress { done, total });
                    ctx.request_repaint();
                },
            );
            let outcome = dl.and_then(|_| {
                let _ = tx.send(Msg::SelfUpdateStaging);
                ctx.request_repaint();
                self_update::stage_update(&file, asset.kind)
            });
            let _ = std::fs::remove_file(&file);
            match outcome {
                Ok((staged_app, staging_dir)) => {
                    let _ = tx.send(Msg::SelfUpdateReady {
                        version,
                        staged_app,
                        staging_dir,
                    });
                }
                Err(e) => {
                    let _ = tx.send(Msg::SelfUpdateError(e));
                }
            }
            ctx.request_repaint();
        });
    }

    /// Swap the staged `.app` into place via a detached helper and relaunch.
    pub(crate) fn apply_self_update_and_relaunch(&mut self) {
        let (staged_app, staging_dir) = match &self.self_update {
            self_update::SelfUpdateState::Ready {
                staged_app,
                staging_dir,
                ..
            } => (staged_app.clone(), staging_dir.clone()),
            _ => return,
        };
        match self_update::apply_and_relaunch(
            &staged_app,
            &staging_dir,
            &self.settings.install_dir,
        ) {
            Ok(()) => std::process::exit(0),
            Err(e) => {
                self.state.status = format!("重启更新失败: {e}");
                self.self_update = self_update::SelfUpdateState::Error(e);
            }
        }
    }

    /// Download the newest release and install it into the configured directory.
    fn start_job(&mut self, idx: usize) {
        if self.state.busy(idx) {
            return;
        }
        let Some(rel) = self.state.release(idx) else {
            if self.state.is_fetching(idx) {
                self.state.status = "正在获取版本信息，请稍候…".to_string();
            } else {
                self.state.status = format!(
                    "未能获取 {} 的版本信息：{}",
                    repos()[idx].name,
                    self.state.fetch_error(idx).unwrap_or("没有可用的安装包")
                );
            }
            return;
        };
        let Some(asset) = rel.asset.clone() else {
            self.state.status =
                format!("{} 的最新发布里没有 macOS 安装包", repos()[idx].name);
            return;
        };
        self.start_download(idx, asset);
    }

    /// Download and install one particular asset. The version chooser uses
    /// this to install a release that is not the newest.
    pub(crate) fn start_download(&mut self, idx: usize, asset: github::MacAsset) {
        if self.state.busy(idx) {
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.state.repos[idx].job = Some(state::Job {
            progress: 0.0,
            speed: 0.0,
            last: None,
            done: 0,
            total: 0,
            phase: JobPhase::Downloading,
            cancel: cancel.clone(),
        });
        self.state.status = format!("正在下载 {}…", repos()[idx].name);
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        let bundle = self.bundle_name(idx);
        let install_dir = self.settings.install_dir.clone();
        let preserve_name = !catalog::is_builtin(repos()[idx].slug);
        let token = if self.settings.github_token.trim().is_empty() {
            None
        } else {
            Some(self.settings.github_token.clone())
        };
        std::thread::spawn(move || {
            let dir = std::env::temp_dir().join("gitup-launcher/download");
            let _ = std::fs::create_dir_all(&dir);
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let file = dir.join(format!("{idx}-{stamp}-{}", asset.name));
            let _ = std::fs::remove_file(&file);
            // The API's link first, the browser link as a second chance.
            let mut urls = vec![asset.url.clone()];
            if asset.browser_url != asset.url {
                urls.push(asset.browser_url.clone());
            }
            let installed = github::download_file(
                &urls,
                &file,
                token.as_deref(),
                &cancel,
                |done, total| {
                    let _ = tx.send(Msg::Progress { idx, done, total });
                    ctx.request_repaint();
                },
            )
            .and_then(|_| {
                let _ = tx.send(Msg::Installing { idx });
                ctx.request_repaint();
                install::install_asset(&file, asset.kind, &bundle, &install_dir, preserve_name)
            });
            let _ = std::fs::remove_file(&file);
            let (actual_bundle, result) = match installed {
                Ok(actual) => {
                    let ver = install::installed_version(&actual, &install_dir).unwrap_or_default();
                    (Some(actual), Ok(ver))
                }
                Err(e) => (None, Err(e)),
            };
            let _ = tx.send(Msg::Done {
                idx,
                bundle: actual_bundle,
                result,
            });
            ctx.request_repaint();
        });
    }

    /// Open the installed app.
    fn open_app(&mut self, idx: usize) {
        let bundle = self.bundle_name(idx);
        match install::open_app(&bundle, &self.settings.install_dir) {
            Ok(()) => self.state.status = format!("已打开 {}", repos()[idx].name),
            Err(e) => self.state.status = format!("打开失败: {e}"),
        }
    }

    /// Show the repository's page in the browser.
    fn open_repo_page(&mut self, idx: usize) {
        let url = format!("https://github.com/{}", repos()[idx].slug);
        let _ = std::process::Command::new("open").arg(&url).spawn();
        self.state.status = format!("已在浏览器打开 {}", repos()[idx].slug);
    }

    /// Copy the repository's address.

    /// What the big button on a card does.
    fn act(&mut self, idx: usize) {
        if self.state.busy(idx) {
            // Clicking a running download cancels it; an install that is
            // already in place cannot be stopped halfway.
            let cancel = self.state.repos[idx]
                .job
                .as_ref()
                .filter(|j| j.phase == JobPhase::Downloading)
                .map(|j| j.cancel.clone());
            if let Some(cancel) = cancel {
                cancel.store(true, Ordering::SeqCst);
                self.state.status = format!("正在取消下载 {}…", repos()[idx].name);
            }
        } else if self.state.has_update(idx) {
            self.start_job(idx);
        } else if self.state.is_installed(idx) {
            self.open_app(idx);
        } else {
            self.start_job(idx);
        }
    }
}

/// The launcher's own state plus the current appearance.
struct App {
    launcher: Launcher,
    /// The demo screen is set up once, not on every frame.
    demo_applied: bool,
    /// Cached theme polarity so egui styles are only rebuilt when theme changes.
    last_dark: Option<bool>,
}

impl eframe::App for App {
    /// Command-V. The window's clipboard feature turns the key into a paste
    /// event of its own; when it does not — the system clipboard refused, or
    /// this build has no backend — the text is read here and put in its
    /// place, so a field pastes either way and never twice. A paste goes to
    /// whichever field has the keyboard; if none has it, the one field the
    /// open page shows is given the focus first.
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        // Esc closes any open modal or popover cleanly.
        let esc_pressed = raw_input.events.iter().any(|e| {
            matches!(
                e,
                egui::Event::Key {
                    key: egui::Key::Escape,
                    pressed: true,
                    ..
                }
            )
        });
        if esc_pressed {
            self.launcher.batch = None;
            self.launcher.picker = None;
            self.launcher.menu_for = None;
            self.launcher.sort_open = false;
        }

        let already_pasted = raw_input
            .events
            .iter()
            .any(|e| matches!(e, egui::Event::Paste(_)));
        if already_pasted {
            return;
        }
        let cmd_v = raw_input.events.iter().any(|e| {
            matches!(
                e,
                egui::Event::Key {
                    key: egui::Key::V,
                    pressed: true,
                    modifiers,
                    ..
                } if modifiers.command
            )
        });
        if !cmd_v {
            return;
        }
        let Some(text) = clipboard_text() else {
            return;
        };
        raw_input.events.retain(|event| {
            !matches!(
                event,
                egui::Event::Key {
                    key: egui::Key::V,
                    pressed: true,
                    modifiers,
                    ..
                } if modifiers.command
            )
        });
        let target = self.launcher.batch.as_ref().map(|batch| {
            if batch.expanded {
                egui::Id::unique("batch-field")
            } else {
                egui::Id::unique("batch-single")
            }
        });
        if let Some(id) = target {
            _ctx.memory_mut(|m| {
                if m.focused().is_none() {
                    m.request_focus(id);
                }
            });
        }
        raw_input.events.push(egui::Event::Paste(text));
    }


    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        // The window's own backdrop, matching the pure-black dark canvas.
        egui::Color32::BLACK.to_normalized_gamma_f32()
    }

    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // Whether the window is zoomed, straight from the window system, so
        // a double-click on the bar always goes the right way — even after
        // the green button was used instead.
        self.launcher.window_zoomed = frame
            .winit_window()
            .is_some_and(|window| window.is_maximized());

        // A drag inside a text box must select text, not move the window.
        // With the window movable by its background the window system takes
        // every such drag, so the switch is turned off while the pointer is
        // over a field — or while the importer, which is nothing but
        // fields, is open. The window system is only told when the answer
        // changes, not every frame.
        let pointer = ctx.input(|i| i.pointer.hover_pos()).unwrap_or_default();
        let over_text = self.launcher.batch.is_some()
            || (self.launcher.page == Page::Grid && self.launcher.search_rect.contains(pointer))
            || (self.launcher.page == Page::Settings
                && self
                    .launcher
                    .settings_text_rects
                    .iter()
                    .any(|r| r.contains(pointer)));
        if over_text != self.launcher.drag_blocked
            && set_movable_by_background(!over_text)
        {
            self.launcher.drag_blocked = over_text;
        }
        let dark = ctx.theme() == egui::Theme::Dark;
        let p = palette(dark);

        if self.last_dark != Some(dark) {
            self.last_dark = Some(dark);
            let theme = if dark {
                egui::Theme::Dark
            } else {
                egui::Theme::Light
            };
            let mut style = (*ctx.style_of(theme)).clone();
            let mut visuals = if dark {
                egui::Visuals::dark()
            } else {
                egui::Visuals::light()
            };
            visuals.panel_fill = p.bg;
            visuals.window_fill = p.bg;
            visuals.override_text_color = Some(p.text);
            visuals.widgets.noninteractive.bg_stroke = Stroke::NONE;
            visuals.widgets.inactive.bg_stroke = Stroke::NONE;
            visuals.widgets.hovered.bg_stroke = Stroke::NONE;
            visuals.widgets.active.bg_stroke = Stroke::NONE;
            visuals.widgets.open.bg_stroke = Stroke::NONE;
            visuals.widgets.noninteractive.fg_stroke = line(p.border);
            visuals.widgets.inactive.fg_stroke = line(p.border);
            visuals.widgets.hovered.fg_stroke = line(p.muted);
            visuals.widgets.active.fg_stroke = line(p.muted);
            visuals.selection.bg_fill = p.accent;
            style.visuals = visuals;
            style.text_styles.insert(egui::TextStyle::Body, text(13.0));
            style.text_styles.insert(egui::TextStyle::Button, text(13.0));
            style.text_styles.insert(egui::TextStyle::Small, text(11.5));
            style.text_styles.insert(egui::TextStyle::Heading, display(20.0));
            style.text_styles.insert(egui::TextStyle::Monospace, label(12.0));
            style.spacing.item_spacing = vec2(8.0, 8.0);
            style.spacing.button_padding = vec2(10.0, 5.0);
            ctx.set_style_of(theme, style);
        }

        // The demo screens, set up on the first frame only.
        if !self.demo_applied {
            self.demo_applied = true;
            match self.launcher.demo {
            Demo::List => self.launcher.state.layout = View::List,
            Demo::Menu => self.launcher.menu_for = Some(0),
            Demo::Sort => self.launcher.sort_open = true,
            // A search under way, to show what the box does.
            Demo::Search => self.launcher.search = "sync".to_string(),
            // One app's page open, to look at the detail view.
            Demo::Detail => self.launcher.open_page(0),
            // The settings page, to look at it too.
            Demo::Settings => self.launcher.page = Page::Settings,
            // A card held under the pointer, so the hover frame can be
            // looked at without a mouse.
            Demo::Hover => self.launcher.hover_demo = Some(0),
            // The version chooser for the first app.
            Demo::Picker => self.launcher.open_versions(0),
            // The bulk importer, with a sample list in it.
            Demo::Batch => {
                self.launcher.open_batch();
                if let Some(batch) = self.launcher.batch.as_mut() {
                    batch.single = SAMPLE_LIST.lines().next().unwrap_or("").to_string();
                    batch.text = SAMPLE_LIST.to_string();
                    // The default is the one-line box; the list is there for
                    // looking at when the demo asks for it.
                    batch.expanded = std::env::var("GITUP_BATCH_LIST").is_ok();
                }
            }
            // The bulk importer, run at once.
            Demo::BatchRun => {
                self.launcher.open_batch();
                if let Some(batch) = self.launcher.batch.as_mut() {
                    batch.single = SAMPLE_LIST.lines().next().unwrap_or("").to_string();
                    batch.text = SAMPLE_LIST.to_string();
                }
                self.launcher.import_repositories(SAMPLE_LIST);
            }
                Demo::None => {}
            }
        }

        // Whatever the worker threads have reported lands here, before
        // anything is drawn.
        self.launcher.handle_messages();

        // While downloads run, the footer keeps count.
        let running = self
            .launcher
            .state
            .repos
            .iter()
            .filter(|r| r.job.is_some())
            .count();
        if running > 0 && self.launcher.state.status.starts_with("正在下载") {
            let busy: Vec<(usize, f32, u64, u64)> = self
                .launcher
                .state
                .repos
                .iter()
                .enumerate()
                .filter_map(|(i, r)| r.job.as_ref().map(|j| (i, j.speed, j.done, j.total)))
                .collect();
            self.launcher.state.status = if busy.len() == 1 {
                let (i, speed, done, total) = busy[0];
                let name = repos().get(i).map(|r| r.name).unwrap_or("应用");
                let mut line = format!("正在下载 {name}… {}", human_speed(speed));
                if total > 0 {
                    line.push_str(&format!(
                        " · {} / {}",
                        human_size(done),
                        human_size(total)
                    ));
                }
                line
            } else {
                let speed: f32 = busy.iter().map(|b| b.1).sum();
                format!(
                    "正在下载 {} 个应用… {}",
                    busy.len(),
                    human_speed(speed)
                )
            };
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(p.bg).inner_margin(0.0))
            .show(ui, |ui| {
                let full = ui.max_rect();
                self.launcher.top_bar(ui, full, &p);
                let body = Rect::from_min_max(
                    pos2(full.min.x, TOPBAR_H),
                    pos2(full.max.x, full.max.y - FOOTER_H),
                );
                self.launcher.body(ui, body, &p);
                self.launcher.footer(ui, full, &p);
                // The pop-ups go on top of everything else.
                if let Some(rect) = self.launcher.pending_sort.take() {
                    self.launcher.draw_sort_dropdown(ui, rect, &p);
                }
                if let Some((rect, idx)) = self.launcher.pending_menu.take() {
                    self.launcher.draw_menu(ui, rect, idx, body, &p);
                }
                // The version chooser and the bulk importer sit above
                // everything else.
                self.launcher.draw_picker(ui, full, &p);
                self.launcher.draw_batch(ui, full, &p);
            });
    }
}

impl Launcher {
    // ------------------------------------------------------------- top bar

    fn top_bar(&mut self, ui: &mut Ui, rect: Rect, p: &Palette) {
        let bar = Rect::from_min_max(rect.min, pos2(rect.max.x, rect.min.y + TOPBAR_H));
        ui.painter().rect_filled(bar, 0, p.bg);
        ui.painter()
            .hline(bar.min.x..=bar.max.x - 0.5, TOPBAR_H, line(p.border));

        let mid = TOPBAR_H * 0.5;

        // The bar is the window's handle: a double-click zooms it, the way
        // a title bar does. The buttons are drawn after this, so they sit
        // on top and keep their own clicks.
        let zoom_area = Rect::from_min_max(
            pos2(bar.min.x + 78.0, bar.min.y),
            pos2(bar.max.x, bar.max.y),
        );
        let zoom_hit = ui.interact(zoom_area, egui::Id::unique("topbar-zoom"), Sense::click());
        if zoom_hit.double_clicked() {
            let zoomed = self.window_zoomed;
            self.window_zoomed = !zoomed;
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Maximized(!zoomed));
        }

        match self.page {
            Page::Grid => {
                draw_g_mark(
                    ui.painter(),
                    Rect::from_center_size(pos2(WORDMARK_X + 8.0, mid), vec2(16.0, 14.0)),
                    p.text,
                );
                ui.painter().text(
                    pos2(WORDMARK_X + 18.0, mid + 0.5),
                    Align2::LEFT_CENTER,
                    "ITUP LAUNCHER",
                    display(20.0),
                    p.text,
                );
            }
            Page::Settings => {
                ui.painter().text(
                    pos2(WORDMARK_X, mid),
                    Align2::LEFT_CENTER,
                    "设置",
                    text(14.0),
                    p.muted,
                );
                self.draw_close_button(ui, rect, mid, p);
            }
            Page::Detail(idx) => {
                ui.painter().text(
                    pos2(WORDMARK_X, mid),
                    Align2::LEFT_CENTER,
                    "应用详情",
                    text(14.0),
                    p.muted,
                );
                self.draw_detail_nav(ui, rect, mid, idx, p);
            }
        }

        // Self-update status pill on the Grid top bar when an update is available/ready.
        if self.page == Page::Grid {
            let pill_info: Option<(String, Color32, bool, u8)> = match &self.self_update {
                self_update::SelfUpdateState::Ready { version, .. } => Some((
                    format!("V{} 已就绪 · 重启更新", version.to_uppercase()),
                    Color32::from_rgb(0x3D, 0xC9, 0x7A),
                    true,
                    2,
                )),
                self_update::SelfUpdateState::Available(rel) => Some((
                    format!("发现新版 V{} · 更新", version::clean_tag(&rel.tag).to_uppercase()),
                    p.amber,
                    false,
                    1,
                )),
                self_update::SelfUpdateState::Downloading { .. } => {
                    let pct = self.self_update.progress_pct().unwrap_or(0);
                    Some((format!("启动器更新中 {pct}%"), p.accent, false, 0))
                }
                self_update::SelfUpdateState::Staging => {
                    Some(("正在准备更新…".to_string(), p.accent, false, 0))
                }
                _ => None,
            };
            if let Some((caption, accent_c, solid, action)) = pill_info {
                let pw = measure(ui, &caption, label(11.0)) + 20.0;
                let pill = Rect::from_min_max(
                    pos2(rect.max.x - 56.0 - pw, mid - 12.0),
                    pos2(rect.max.x - 56.0, mid + 12.0),
                );
                let hit = ui.interact(pill, ui.scope_id().with("topbar-self-upd"), Sense::click());
                if hit.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if solid {
                    ui.painter().rect_filled(pill, radius(12), accent_c);
                } else {
                    ui.painter().rect_filled(
                        pill,
                        radius(12),
                        Color32::from_rgba_unmultiplied(
                            accent_c.r(),
                            accent_c.g(),
                            accent_c.b(),
                            if hit.hovered() { 42 } else { 24 },
                        ),
                    );
                    ui.painter().rect_stroke(
                        pill,
                        radius(12),
                        Stroke::new(1.0, accent_c),
                        StrokeKind::Middle,
                    );
                }
                ui.painter().text(
                    pill.center(),
                    Align2::CENTER_CENTER,
                    &caption,
                    label(11.0),
                    if solid { p.bg } else { accent_c },
                );
                if hit.clicked() {
                    match action {
                        2 => self.apply_self_update_and_relaunch(),
                        1 => self.start_self_update_download(),
                        _ => self.page = Page::Settings,
                    }
                }
            }
        }

        let gear_x = rect.max.x - 28.0;
        ui.painter()
            .vline(rect.max.x - 46.0, 0.0..=TOPBAR_H, line(p.border));

        let gear_rect = Rect::from_center_size(pos2(gear_x, mid), vec2(22.0, 22.0));
        let resp = ui.interact(gear_rect, ui.scope_id().with("gear"), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(gear_rect, radius(4), p.surface);
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if resp.clicked() {
            self.page = Page::Settings;
            self.settings_scroll = 0.0;
            self.menu_for = None;
            self.sort_open = false;
        }
        gear_icon(ui.painter(), gear_rect, p.muted);
        if matches!(
            self.self_update,
            self_update::SelfUpdateState::Available(_) | self_update::SelfUpdateState::Ready { .. }
        ) {
            ui.painter().circle_filled(
                pos2(gear_rect.max.x - 2.0, gear_rect.min.y + 3.0),
                3.5,
                p.amber,
            );
        }
    }

    /// The detail page's navigation cluster: back, forward and close.
    fn draw_detail_nav(&mut self, ui: &mut Ui, rect: Rect, mid: f32, idx: usize, p: &Palette) {
        let _ = idx;
        let nav_end = rect.max.x - 56.0;
        let size = 28.0;
        // Cells sit side by side with an eight-point gap; the close cell's
        // right edge is the far end of the cluster.
        let step = size + 8.0;
        let close_x = nav_end - size;
        for (which, x) in [
            (2, close_x),
            (1, close_x - step),
            (0, close_x - 2.0 * step),
        ] {
            self.draw_nav_cell(ui, which, x, mid, p, size);
        }
    }

    /// One square cell of the navigation cluster: an outline, a hover
    /// fill, and a click. `which` says 0 back, 1 forward, 2 close.
    fn draw_nav_cell(&mut self, ui: &mut Ui, which: usize, x: f32, mid: f32, p: &Palette, size: f32) {
        let r = Rect::from_min_size(pos2(x, mid - size * 0.5), vec2(size, size));
        let resp = ui.interact(r, ui.scope_id().with(("nav", which)), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(r, radius(4), p.surface);
        }
        ui.painter()
            .rect_stroke(r, radius(4), line(p.border), StrokeKind::Middle);
        let c = r.center();
        let s = Stroke::new(1.5, p.muted);
        match which {
            0 => {
                let a = egui::pos2(c.x - 3.0, c.y - 5.0);
                let b = egui::pos2(c.x - 7.0, c.y);
                let d = egui::pos2(c.x - 3.0, c.y + 5.0);
                ui.painter().add(egui::Shape::line(vec![a, b, d], s));
            }
            1 => {
                let a = egui::pos2(c.x + 3.0, c.y - 5.0);
                let b = egui::pos2(c.x + 7.0, c.y);
                let d = egui::pos2(c.x + 3.0, c.y + 5.0);
                ui.painter().add(egui::Shape::line(vec![a, b, d], s));
            }
            _ => {
                ui.painter().line_segment(
                    [egui::pos2(c.x - 5.0, c.y - 5.0), egui::pos2(c.x + 5.0, c.y + 5.0)],
                    s,
                );
                ui.painter().line_segment(
                    [egui::pos2(c.x + 5.0, c.y - 5.0), egui::pos2(c.x - 5.0, c.y + 5.0)],
                    s,
                );
            }
        }
        if resp.clicked() {
            if let Page::Detail(idx) = self.page {
                match which {
                    0 => self.step_page(idx, false),
                    1 => self.step_page(idx, true),
                    _ => self.page = Page::Grid,
                }
            } else if which == 2 {
                self.page = Page::Grid;
            }
        }
    }

    /// The app before or after this one in the order the grid shows them.
    /// Only what is actually subscribed takes part, so the arrows never
    /// open a repository that has been removed from the subscriptions.
    fn step_page(&mut self, from: usize, forward: bool) {
        let list = self.visible();
        if list.is_empty() {
            self.page = Page::Grid;
            return;
        }
        let next = match list.iter().position(|&i| i == from) {
            Some(at) if forward => list[(at + 1) % list.len()],
            Some(at) => list[(at + list.len() - 1) % list.len()],
            None => list[0],
        };
        self.open_page(next);
    }

    /// The settings page's close cell, at the far right of its header.
    fn draw_close_button(&mut self, ui: &mut Ui, rect: Rect, mid: f32, p: &Palette) {
        let nav_end = rect.max.x - 56.0;
        self.draw_nav_cell(ui, 2, nav_end - 28.0, mid, p, 28.0);
    }

    // -------------------------------------------------------------- toolbar

    fn toolbar(&mut self, ui: &mut Ui, rect: Rect, p: &Palette) {
        let top = rect.min.y + 10.0;
        let h = 30.0;
        let bottom = top + h;

        // Left action buttons: 添加订阅 + 刷新.
        let mut left_x = MARGIN;
        for (id, caption, w, adds) in [
            ("toolbar-add", "添加订阅", 94.0f32, true),
            ("check-updates", "刷新", 68.0f32, false),
        ] {
            let btn = Rect::from_min_max(pos2(left_x, top), pos2(left_x + w, bottom));
            let resp = ui.interact(btn, ui.scope_id().with(id), Sense::click());
            let over = resp.hovered();
            if over {
                ui.painter().rect_filled(btn, radius(6), p.surface);
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            ui.painter().rect_stroke(
                btn,
                radius(6),
                if over {
                    Stroke::new(1.5, p.accent)
                } else {
                    line(p.border)
                },
                StrokeKind::Middle,
            );
            let ink = if over { p.accent } else { p.muted };
            let mark = Rect::from_center_size(
                pos2(btn.min.x + 17.0, btn.center().y),
                vec2(13.0, 13.0),
            );
            if adds {
                ui.painter().line_segment(
                    [
                        pos2(mark.center().x - 4.5, mark.center().y),
                        pos2(mark.center().x + 4.5, mark.center().y),
                    ],
                    Stroke::new(1.4, ink),
                );
                ui.painter().line_segment(
                    [
                        pos2(mark.center().x, mark.center().y - 4.5),
                        pos2(mark.center().x, mark.center().y + 4.5),
                    ],
                    Stroke::new(1.4, ink),
                );
            } else {
                refresh_icon(ui.painter(), mark, ink);
            }
            ui.painter().text(
                pos2(btn.min.x + 30.0, btn.center().y),
                Align2::LEFT_CENTER,
                caption,
                text(12.5),
                if over { p.accent } else { p.text },
            );
            if resp.clicked() {
                if adds {
                    self.open_batch();
                } else {
                    self.spawn_meta_fetch_all();
                }
            }
            left_x += w + 8.0;
        }

        // Right-anchored controls: view switch, search, sort.
        let right = rect.max.x - MARGIN;

        let toggle_w = 68.0;
        let toggle = Rect::from_min_max(pos2(right - toggle_w, top), pos2(right, bottom));
        let grid_rect = Rect::from_min_max(toggle.min, pos2(toggle.center().x, toggle.max.y));
        let list_rect = Rect::from_min_max(pos2(toggle.center().x, toggle.min.y), toggle.max);
        ui.painter().rect_filled(toggle, radius(6), p.bg);
        let hot = match self.state.layout {
            View::Grid => grid_rect,
            View::List => list_rect,
        };
        ui.painter().rect_filled(hot, radius(5), p.text);
        let resp = ui.interact(toggle, ui.scope_id().with("toggle"), Sense::click());
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        ui.painter().rect_stroke(
            toggle,
            radius(6),
            if resp.hovered() {
                Stroke::new(1.2, p.accent)
            } else {
                line(p.border)
            },
            StrokeKind::Middle,
        );
        grid_icon(
            ui.painter(),
            Rect::from_center_size(grid_rect.center(), vec2(16.0, 16.0)),
            if self.state.layout == View::Grid {
                p.bg
            } else {
                p.muted
            },
        );
        list_icon(
            ui.painter(),
            Rect::from_center_size(list_rect.center(), vec2(16.0, 16.0)),
            if self.state.layout == View::List {
                p.bg
            } else {
                p.muted
            },
        );
        if resp.clicked() {
            self.state.layout = match self.state.layout {
                View::Grid => View::List,
                View::List => View::Grid,
            };
        }

        // Search box with focus accent and clear button.
        let search_w = 196.0;
        let search_rect = Rect::from_min_max(
            pos2(toggle.min.x - 8.0 - search_w, top),
            pos2(toggle.min.x - 8.0, bottom),
        );
        self.search_rect = search_rect;
        let search_id = egui::Id::unique("toolbar-search");
        let search_focused = ui.memory(|m| m.has_focus(search_id));
        ui.painter().rect_filled(search_rect, radius(6), p.search_fill);
        ui.painter().rect_stroke(
            search_rect,
            radius(6),
            if search_focused {
                Stroke::new(1.5, p.accent)
            } else {
                line(p.border)
            },
            StrokeKind::Middle,
        );
        if self.search.is_empty() {
            ui.painter().text(
                pos2(search_rect.min.x + 10.0, search_rect.center().y),
                Align2::LEFT_CENTER,
                "搜索应用…",
                text(12.5),
                p.faint,
            );
        }
        let clear_space = if self.search.is_empty() { 8.0 } else { 24.0 };
        let edit = TextEdit::singleline(&mut self.search)
            .id(search_id)
            .text_color(p.text)
            .frame(egui::Frame::NONE)
            .desired_width(search_rect.width() - 10.0 - clear_space);
        let mut child = ui.new_child(
            egui::UiBuilder::new().max_rect(Rect::from_min_max(
                pos2(search_rect.min.x + 8.0, search_rect.center().y - 9.0),
                pos2(search_rect.max.x - clear_space, search_rect.center().y + 9.0),
            )),
        );
        child.add(edit);
        if !self.search.is_empty() {
            let clr = Rect::from_center_size(
                pos2(search_rect.max.x - 13.0, search_rect.center().y),
                vec2(16.0, 16.0),
            );
            let clr_hit = ui.interact(clr, ui.scope_id().with("search-clear"), Sense::click());
            if clr_hit.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            let st = Stroke::new(1.3, if clr_hit.hovered() { p.text } else { p.muted });
            let c = clr.center();
            ui.painter()
                .line_segment([pos2(c.x - 3.5, c.y - 3.5), pos2(c.x + 3.5, c.y + 3.5)], st);
            ui.painter()
                .line_segment([pos2(c.x + 3.5, c.y - 3.5), pos2(c.x - 3.5, c.y + 3.5)], st);
            if clr_hit.clicked() {
                self.search.clear();
                self.state.scroll = 0.0;
            }
        }

        // Sort dropdown trigger.
        let sort_w = 132.0;
        let sort_rect = Rect::from_min_max(
            pos2(search_rect.min.x - 8.0 - sort_w, top),
            pos2(search_rect.min.x - 8.0, bottom),
        );
        let resp_sort = ui.interact(sort_rect, ui.scope_id().with("sort"), Sense::click());
        if resp_sort.hovered() {
            ui.painter().rect_filled(sort_rect, radius(6), p.surface);
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        ui.painter().rect_stroke(
            sort_rect,
            radius(6),
            if resp_sort.hovered() || self.sort_open {
                Stroke::new(1.2, p.accent)
            } else {
                line(p.border)
            },
            StrokeKind::Middle,
        );
        let sort_name = match self.state.sort {
            SortBy::Name => "名称 (A-Z)",
            SortBy::Updated => "最后更新",
        };
        ui.painter().text(
            pos2(sort_rect.min.x + 10.0, sort_rect.center().y),
            Align2::LEFT_CENTER,
            sort_name,
            text(12.5),
            p.text,
        );
        caret_down(
            ui.painter(),
            Rect::from_center_size(
                pos2(sort_rect.max.x - 14.0, sort_rect.center().y),
                vec2(9.0, 9.0),
            ),
            p.muted,
        );
        if resp_sort.clicked() {
            self.sort_open = !self.sort_open;
            self.menu_for = None;
        }

        // Compact segmented status filter pills in the middle when there is room.
        let total_sub = (0..repos().len())
            .filter(|&i| self.state.subscribed[i])
            .count();
        let total_inst = (0..repos().len())
            .filter(|&i| self.state.subscribed[i] && self.state.is_installed(i))
            .count();
        let total_upd = (0..repos().len())
            .filter(|&i| self.state.subscribed[i] && self.state.has_update(i))
            .count();

        let pills = [
            (Filter::All, format!("全部 {total_sub}"), false),
            (Filter::Installed, format!("已安装 {total_inst}"), false),
            (
                Filter::Updatable,
                if total_upd > 0 {
                    format!("可更新 {total_upd}")
                } else {
                    "可更新".to_string()
                },
                total_upd > 0,
            ),
        ];
        let pill_widths: Vec<f32> = pills
            .iter()
            .map(|(_, label_s, _)| measure(ui, label_s, text(12.0)) + 20.0)
            .collect();
        let group_w: f32 = pill_widths.iter().sum::<f32>();
        let group_x = left_x + 8.0;
        if group_x + group_w + 16.0 < sort_rect.min.x {
            let group_rect = Rect::from_min_max(pos2(group_x, top), pos2(group_x + group_w, bottom));
            ui.painter().rect_filled(group_rect, radius(6), p.search_fill);
            ui.painter()
                .rect_stroke(group_rect, radius(6), line(p.border), StrokeKind::Middle);
            let mut px = group_x;
            for ((kind, caption, highlight), pw) in pills.iter().zip(pill_widths.iter()) {
                let cell = Rect::from_min_max(pos2(px, top), pos2(px + *pw, bottom));
                let active = self.filter == *kind;
                let hit = ui.interact(cell, ui.scope_id().with(("filter", *kind as u8)), Sense::click());
                if hit.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if active {
                    let fill = if *kind == Filter::Updatable && *highlight {
                        p.amber
                    } else {
                        p.text
                    };
                    ui.painter().rect_filled(cell.shrink(1.5), radius(4), fill);
                } else if hit.hovered() {
                    ui.painter().rect_filled(cell.shrink(1.5), radius(4), p.surface);
                }
                let fg = if active {
                    p.bg
                } else if *highlight {
                    p.amber
                } else if hit.hovered() {
                    p.text
                } else {
                    p.muted
                };
                ui.painter()
                    .text(cell.center(), Align2::CENTER_CENTER, caption, text(12.0), fg);
                if hit.clicked() {
                    self.filter = *kind;
                    self.state.scroll = 0.0;
                }
                px += *pw;
            }
            // One-click "全部更新" button when updates are available and space permits.
            if total_upd > 0 {
                let upd_x = group_x + group_w + 8.0;
                let upd_w = 76.0;
                if upd_x + upd_w + 12.0 < sort_rect.min.x {
                    let upd_rect =
                        Rect::from_min_max(pos2(upd_x, top), pos2(upd_x + upd_w, bottom));
                    let painter = ui.painter().clone();
                    if primary_button(
                        ui,
                        &painter,
                        upd_rect,
                        ui.scope_id().with("update-all"),
                        "全部更新",
                        12.0,
                        rect,
                        p,
                    ) {
                        self.update_all();
                    }
                }
            }
        }

        if self.sort_open {
            self.pending_sort = Some(sort_rect);
        }
    }

    // ----------------------------------------------------------------- body

    fn body(&mut self, ui: &mut Ui, rect: Rect, p: &Palette) {
        // The cards stop just short of the footer, and the strip they run
        // in is what scrolls.
        let band = Rect::from_min_max(
            pos2(rect.min.x, TOPBAR_H),
            pos2(rect.max.x, rect.max.y - 8.0),
        );

        match self.page {
            Page::Settings => {
                self.settings_page(ui, band, p);
                return;
            }
            Page::Detail(idx) => {
                self.detail(ui, band, idx, p);
                return;
            }
            Page::Grid => {}
        }

        // The toolbar sits at the top of the body, and everything else
        // starts below it.
        self.toolbar(ui, rect, p);
        let grid_top = rect.min.y + TOOLBAR_H;
        let band = Rect::from_min_max(
            pos2(rect.min.x, grid_top),
            pos2(rect.max.x, rect.max.y - 8.0),
        );

        let apps = self.visible();

        if apps.is_empty() {
            let empty_msg = if self.filter == Filter::Updatable {
                "所有已安装应用均为最新版本"
            } else {
                "没有找到匹配的应用"
            };
            ui.painter().text(
                pos2(rect.center().x, grid_top + 64.0),
                Align2::CENTER_CENTER,
                empty_msg,
                text(14.0),
                p.muted,
            );
            if self.filter != Filter::All || !self.search.is_empty() {
                let reset_rect = Rect::from_center_size(
                    pos2(rect.center().x, grid_top + 100.0),
                    vec2(104.0, 28.0),
                );
                let painter = ui.painter().clone();
                if action_button(
                    ui,
                    &painter,
                    reset_rect,
                    egui::Id::unique("empty-reset"),
                    "查看全部",
                    12.5,
                    band,
                    p,
                ) {
                    self.filter = Filter::All;
                    self.search.clear();
                    self.state.scroll = 0.0;
                }
            }
            return;
        }

        let avail_w = (band.width() - MARGIN - RIGHT_GUTTER).max(200.0);
        let cols = grid_columns(avail_w);

        // How tall the whole thing is, before the window clips it.
        let content_h = match self.state.layout {
            View::Grid => self.grid_height(&apps, cols),
            View::List => self.list_height(&apps),
        };

        // A repository that has just been added sits at the end of the
        // list, so the grid goes there to show it.
        if self.pending_scroll_end {
            self.state.scroll = (content_h - band.height()).max(0.0);
            self.pending_scroll_end = false;
        }

        // The wheel moves the grid, in the same direction the Mac's own
        // scrollbars move.
        self.state.scroll = self.apply_wheel(ui, band, content_h, self.state.scroll);

        match self.state.layout {
            View::Grid => self.grid(ui, band, &apps, p),
            View::List => self.list(ui, band, &apps, p),
        }

        self.state.scroll =
            self.scrollbar(ui, band, content_h, self.state.scroll, "grid", p);
    }

    /// The wheel moves the content in the same direction the Mac's own
    /// scrollbars move.
    pub(crate) fn apply_wheel(&self, ui: &Ui, band: Rect, content_h: f32, scroll: f32) -> f32 {
        let max_scroll = (content_h - band.height()).max(0.0);
        if max_scroll <= 0.0 {
            return 0.0;
        }
        if !ui.rect_contains_pointer(band) {
            return scroll;
        }
        let dy = ui.ctx().input(|i| i.smooth_scroll_delta.y);
        if dy != 0.0 {
            (scroll - dy).clamp(0.0, max_scroll)
        } else {
            scroll
        }
    }

    /// The scrollbar down the right of the content. The track is the same
    /// two-pixel grey strip the picture shows; the thumb in it can be
    /// dragged, and clicking the track pages up or down.
    pub(crate) fn scrollbar(
        &self,
        ui: &mut Ui,
        band: Rect,
        content_h: f32,
        scroll: f32,
        which: &str,
        p: &Palette,
    ) -> f32 {
        let view_h = band.height();
        let max_scroll = (content_h - view_h).max(0.0);
        let track = Rect::from_min_max(
            pos2(band.max.x - 22.0, band.min.y),
            pos2(band.max.x - 20.0, band.max.y),
        );
        ui.painter()
            .rect_filled(track, 0, Color32::from_rgb(0x9E, 0x9E, 0x9E));
        if max_scroll <= 0.0 {
            return 0.0;
        }
        let thumb_h = (view_h * view_h / content_h).clamp(24.0, view_h);
        let t = (scroll / max_scroll).clamp(0.0, 1.0);
        let thumb_y = band.min.y + t * (view_h - thumb_h);
        let thumb = Rect::from_min_max(
            pos2(track.min.x, thumb_y),
            pos2(track.max.x, thumb_y + thumb_h),
        );
        ui.painter().rect_filled(thumb, 0, p.text);
        let hit = thumb.expand(5.0);
        let resp = ui.interact(
            hit,
            ui.scope_id().with((which, "thumb")),
            Sense::click_and_drag(),
        );
        if resp.dragged() {
            let dy = resp.drag_delta().y;
            return (scroll - dy * (content_h / view_h)).clamp(0.0, max_scroll);
        }
        let resp_track = ui
            .interact(track, ui.scope_id().with((which, "track")), Sense::click());
        if let Some(pos) = resp_track.interact_pointer_pos() {
            if resp_track.clicked() {
                let step = view_h * 0.8;
                return if pos.y < thumb_y {
                    scroll - step
                } else {
                    scroll + step
                }
                .clamp(0.0, max_scroll);
            }
        }
        scroll
    }

    /// The three-column grid of cards. Each card carries its own frame, and
    /// the cards sit side by side, so their frames read as one table — the
    /// way the original draws it. The columns share whatever width the
    /// window has, and the whole thing rides the scroll offset.
    fn grid(&mut self, ui: &mut Ui, band: Rect, apps: &[usize], p: &Palette) {
        let avail_w = (band.width() - MARGIN - RIGHT_GUTTER).max(200.0);
        let cols = grid_columns(avail_w);
        let col_w = (avail_w - CARD_GAP * (cols.saturating_sub(1)) as f32) / cols as f32;
        let scroll = self.state.scroll;
        let pointer = ui.input(|i| i.pointer.hover_pos());
        // Installed apps come first; uninstalled ones sit below a compact 未安装 heading.
        let installed: Vec<usize> = apps
            .iter()
            .copied()
            .filter(|&i| self.state.is_installed(i))
            .collect();
        let missing: Vec<usize> = apps
            .iter()
            .copied()
            .filter(|&i| !self.state.is_installed(i))
            .collect();

        let mut top = band.min.y - scroll;
        for (group, dim) in [(&installed, false), (&missing, true)] {
            if group.is_empty() {
                continue;
            }
            if dim {
                let head = Rect::from_min_size(pos2(MARGIN, top), vec2(avail_w, SECTION_H));
                if head.intersects(band) {
                    let heading = format!("未安装 · {}", group.len());
                    ui.painter().text(
                        pos2(head.min.x + 2.0, head.center().y),
                        Align2::LEFT_CENTER,
                        &heading,
                        label(11.5),
                        p.faint,
                    );
                    let hw = measure(ui, &heading, label(11.5));
                    ui.painter().hline(
                        (head.min.x + hw + 12.0)..=head.max.x,
                        head.center().y,
                        line(p.border),
                    );
                }
                top += SECTION_H;
            }
            for chunk in group.chunks(cols) {
                for (col, &idx) in chunk.iter().enumerate() {
                    let x = MARGIN + col as f32 * (col_w + CARD_GAP);
                    let card = Rect::from_min_size(pos2(x, top), vec2(col_w, CARD_H));
                    self.card(ui, card, idx, band, p);
                    if dim {
                        let over = pointer.map(|pos| card.contains(pos)).unwrap_or(false);
                        if !over {
                            let vis = card.intersect(band);
                            if vis.is_positive() {
                                ui.painter().rect_filled(
                                    vis,
                                    radius(8),
                                    Color32::from_rgba_unmultiplied(
                                        p.bg.r(),
                                        p.bg.g(),
                                        p.bg.b(),
                                        120,
                                    ),
                                );
                            }
                        }
                    }
                }
                top += CARD_H + CARD_GAP;
            }
        }
    }

    /// How tall the grid content is, for the scrollbar and the wheel.
    fn grid_height(&self, apps: &[usize], cols: usize) -> f32 {
        let installed = apps
            .iter()
            .filter(|&&i| self.state.is_installed(i))
            .count();
        let missing = apps.len() - installed;
        let mut h = 0.0;
        for (count, is_missing) in [(installed, false), (missing, true)] {
            if count == 0 {
                continue;
            }
            if is_missing {
                h += SECTION_H;
            }
            let rows = count.div_ceil(cols.max(1));
            h += rows as f32 * (CARD_H + CARD_GAP);
        }
        h
    }

    /// Compact app card: icon + title/version/owner + 2-line summary + date + action bar.
    fn card(&mut self, ui: &mut Ui, card: Rect, idx: usize, band: Rect, p: &Palette) {
        let vis = card.intersect(band);
        if !vis.is_positive() {
            return;
        }
        let clip = ui.painter().with_clip_rect(vis);
        let painter = &clip;

        let app = &repos()[idx];
        let mut hovered = false;
        if vis.is_positive() {
            let resp = ui.interact(vis, ui.scope_id().with(("card", app.id)), Sense::click());
            hovered = resp.hovered() || self.hover_demo == Some(idx);
            if hovered {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.clicked() {
                self.open_page(idx);
            }
        }
        let installed = self.state.is_installed(idx);
        let has_update = self.state.has_update(idx);
        let (phase, progress) = self.state.repos[idx]
            .job
            .as_ref()
            .map(|j| (j.phase, j.progress))
            .unwrap_or((JobPhase::Downloading, 0.0));
        let busy = self.state.busy(idx);
        let (version_text, date_text) = match &self.state.repos[idx].fetch {
            FetchState::Pending => ("…".to_string(), "…".to_string()),
            FetchState::Done(Ok(rel)) => (
                format!("V{}", version::clean_tag(&rel.tag).to_uppercase()),
                version::fmt_date(&rel.published),
            ),
            FetchState::Done(Err(_)) => match &self.state.repos[idx].installed {
                Some(v) => (format!("V{}", v.to_uppercase()), "未获取到".to_string()),
                None => ("—".to_string(), "未获取到".to_string()),
            },
        };

        // Subtle elevated surface so separated cards read crisply in both dark & light themes.
        let dark = p.bg.r() < 128;
        let card_bg = if hovered {
            if dark {
                Color32::from_rgb(0x16, 0x18, 0x1D)
            } else {
                Color32::from_rgb(0xF3, 0xF6, 0xFB)
            }
        } else if dark {
            Color32::from_rgb(0x0C, 0x0D, 0x0F)
        } else {
            Color32::WHITE
        };
        let border_stroke = if hovered {
            Stroke::new(1.5, p.accent)
        } else if has_update {
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(p.amber.r(), p.amber.g(), p.amber.b(), 140))
        } else {
            line(p.border)
        };
        painter.rect_filled(card, radius(8), card_bg);
        painter.rect_stroke(card, radius(8), border_stroke, StrokeKind::Middle);

        let pad = 14.0;
        // Top-right external GitHub link button.
        let link_rect = Rect::from_min_size(
            pos2(card.max.x - pad - 24.0, card.min.y + 12.0),
            vec2(24.0, 24.0),
        );
        let link_hit = link_rect.intersect(band);
        let mut link_hover = false;
        if link_hit.is_positive() {
            let resp = ui.interact(
                link_hit,
                ui.scope_id().with(("link", app.id)),
                Sense::click(),
            );
            link_hover = resp.hovered();
            if link_hover {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                painter.rect_filled(link_rect, radius(5), p.surface);
            }
            if resp.clicked() {
                self.open_repo_page(idx);
            }
        }
        arrow_out(
            painter,
            Rect::from_center_size(link_rect.center(), vec2(12.0, 12.0)),
            if link_hover { p.accent } else { p.faint },
        );

        // App icon tile.
        let tile = Rect::from_min_size(pos2(card.min.x + pad, card.min.y + pad), vec2(TILE, TILE));
        let tile_hit = tile.intersect(band);
        if tile_hit.is_positive() {
            let resp = ui.interact(
                tile_hit,
                ui.scope_id().with(("tile", app.id)),
                Sense::click(),
            );
            if resp.clicked() {
                self.open_page(idx);
            }
        }
        self.draw_tile(painter, tile, idx, 10);

        // App name + version & owner meta line beside the icon.
        let text_x = tile.max.x + 12.0;
        let title_max_w = (link_rect.min.x - 8.0 - text_x).max(40.0);
        let shown_name = fit_text(ui, app.name, display(17.5), title_max_w);
        painter.text(
            pos2(text_x, tile.min.y + 12.0),
            Align2::LEFT_CENTER,
            shown_name,
            display(17.5),
            p.text,
        );

        let version_color = if has_update {
            p.amber
        } else {
            Color32::from_rgb(app.bright.0, app.bright.1, app.bright.2)
        };
        let meta_y = tile.min.y + 34.0;
        painter.text(
            pos2(text_x, meta_y),
            Align2::LEFT_CENTER,
            &version_text,
            label(11.5),
            version_color,
        );
        let ver_w = measure(ui, &version_text, label(11.5));
        let owner = catalog::owner_of(app.slug).to_uppercase();
        let owner_x = text_x + ver_w + 6.0;
        let owner_room = (card.max.x - pad - owner_x).max(0.0);
        if owner_room > 24.0 {
            let owner_str = fit_text(ui, &format!("· {owner}"), label(11.0), owner_room);
            painter.text(
                pos2(owner_x, meta_y),
                Align2::LEFT_CENTER,
                owner_str,
                label(11.0),
                p.faint,
            );
        }

        // 2-line clamped description with comfortable breathing room (cached).
        let desc_y = tile.max.y + 12.0;
        let wrap_w = (card.width() - pad * 2.0).max(40.0);
        let summary = self.summary_text(idx);
        let cache_key = (app.slug.to_string(), wrap_w.round() as u32, summary.len());
        let clamped = if let Some(cached) = self.clamp_cache.get(&cache_key) {
            cached.clone()
        } else {
            let computed = clamp_two_lines(ui, &summary, text(13.0), wrap_w);
            self.clamp_cache.insert(cache_key, computed.clone());
            computed
        };
        let galley = painter.layout(clamped, text(13.0), p.muted, wrap_w);
        painter.galley(pos2(card.min.x + pad, desc_y), galley, p.muted);

        // Action bar anchored at bottom, with date & status row directly above it.
        let btn_h = 30.0;
        let btn_y = card.max.y - pad - btn_h;
        let meta_row_y = btn_y - 16.0;

        painter.text(
            pos2(card.min.x + pad, meta_row_y),
            Align2::LEFT_CENTER,
            format!("更新于 {date_text}"),
            text(12.0),
            p.faint,
        );
        if has_update {
            let badge_w = measure(ui, "可更新", label(10.5)) + 14.0;
            let badge = Rect::from_min_max(
                pos2(card.max.x - pad - badge_w, meta_row_y - 9.5),
                pos2(card.max.x - pad, meta_row_y + 9.5),
            );
            painter.rect_filled(
                badge,
                radius(9),
                Color32::from_rgba_unmultiplied(p.amber.r(), p.amber.g(), p.amber.b(), 28),
            );
            painter.rect_stroke(badge, radius(9), Stroke::new(1.0, p.amber), StrokeKind::Middle);
            painter.text(badge.center(), Align2::CENTER_CENTER, "可更新", label(10.5), p.amber);
        } else if !installed {
            painter.text(
                pos2(card.max.x - pad, meta_row_y),
                Align2::RIGHT_CENTER,
                "未安装",
                label(10.5),
                p.faint,
            );
        }

        let more_w = 32.0;
        let right_edge = card.max.x - pad;
        let left_edge = card.min.x + pad;
        let more_rect = Rect::from_min_max(
            pos2(right_edge - more_w, btn_y),
            pos2(right_edge, btn_y + btn_h),
        );
        let main_rect = Rect::from_min_max(
            pos2(left_edge, btn_y),
            pos2(more_rect.min.x - 8.0, btn_y + btn_h),
        );

        let show_open = !busy && has_update && installed;
        let half = (main_rect.width() - 8.0) / 2.0;
        let mut main_left = main_rect.min.x;
        if show_open {
            let launch = Rect::from_min_max(
                pos2(left_edge, btn_y),
                pos2(left_edge + half, btn_y + btn_h),
            );
            if action_button(
                ui,
                painter,
                launch,
                egui::Id::unique(("launch", app.id)),
                "打开",
                13.5,
                band,
                p,
            ) {
                self.open_app(idx);
            }
            main_left = launch.max.x + 8.0;
        }
        let open_rect = Rect::from_min_max(
            pos2(main_left, btn_y),
            pos2(main_rect.max.x, btn_y + btn_h),
        );

        let label_text = self.button_label(idx).to_string();
        let update_waiting = has_update && installed && !busy;
        let pressed = if update_waiting {
            primary_button(
                ui,
                painter,
                open_rect,
                egui::Id::unique(("open", app.id)),
                &label_text,
                13.5,
                band,
                p,
            )
        } else {
            action_button(
                ui,
                painter,
                open_rect,
                egui::Id::unique(("open", app.id)),
                &label_text,
                13.5,
                band,
                p,
            )
        };
        if pressed {
            self.act(idx);
        }
        if busy && phase == JobPhase::Downloading && progress > 0.0 {
            let filled = Rect::from_min_max(
                open_rect.min,
                pos2(
                    open_rect.min.x + open_rect.width() * progress,
                    open_rect.max.y,
                ),
            );
            painter.rect_filled(filled, radius(5), p.surface);
            painter.rect_stroke(open_rect, radius(5), line(p.border), StrokeKind::Middle);
            painter.text(
                open_rect.center(),
                Align2::CENTER_CENTER,
                label_text,
                text(13.5),
                p.text,
            );
        }

        if action_button(
            ui,
            painter,
            more_rect,
            egui::Id::unique(("more", app.id)),
            "…",
            13.0,
            band,
            p,
        ) {
            self.menu_for = if self.menu_for == Some(idx) {
                None
            } else {
                Some(idx)
            };
            self.sort_open = false;
        }

        if self.menu_for == Some(idx) {
            self.pending_menu = Some((more_rect, idx));
        }
    }

    /// Open the version chooser for one app, asking GitHub for its whole
    /// release list on a worker thread.
    pub(crate) fn open_versions(&mut self, idx: usize) {
        self.menu_for = None;
        self.picker = Some(Picker {
            idx,
            loading: true,
            error: None,
            list: Vec::new(),
            chosen: 0,
            open: false,
        });
        let slug = repos()[idx].slug.to_string();
        let include_pre = self.settings.include_pre;
        let token = if self.settings.github_token.trim().is_empty() {
            None
        } else {
            Some(self.settings.github_token.clone())
        };
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let result = github::fetch_release_list(&slug, include_pre, token.as_deref());
            let _ = tx.send(Msg::Versions { idx, result });
            ctx.request_repaint();
        });
    }

    /// The notes of the newest release: its page on GitHub, where the whole
    /// write-up lives.
    pub(crate) fn open_notes(&mut self, idx: usize) {
        let url = match self.state.release(idx) {
            Some(rel) if !rel.html_url.is_empty() => rel.html_url.clone(),
            _ => format!("https://github.com/{}/releases", repos()[idx].slug),
        };
        let _ = std::process::Command::new("open").arg(url).spawn();
        self.state.status = format!("已在浏览器打开 {} 的更新说明", repos()[idx].name);
    }

    /// Open the importer. Whatever is on the clipboard is put in it
    /// straight away — its first address in the one-line box, the whole
    /// text in the list box — so something can be added without typing.
    fn open_batch(&mut self) {
        let clip = clipboard_text().unwrap_or_default();
        let first = clip
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty() && !line.starts_with('#'))
            .unwrap_or_default()
            .to_string();
        self.batch = Some(Batch {
            single: first,
            text: clip,
            report: None,
            expanded: false,
            focus: true,
        });
    }

    /// Read every address in the box and subscribe to all of them at once.
    /// It answers how many were added, how many were already there and how
    /// many lines could not be read, so the caller knows whether anything
    /// happened at all.
    pub(crate) fn import_repositories(&mut self, text: &str) -> (usize, usize, usize) {
        let mut added = 0usize;
        let mut restored = 0usize;
        let mut known = 0usize;
        let mut bad: Vec<String> = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            // An address that is already in the list but switched off is
            // switched back on rather than refused — this is where a
            // subscription deleted earlier is added again. The switched-off
            // list holds repository ids, not slugs.
            if let Some(idx) = catalog::lookup(line) {
                let id = repos()[idx].id.to_string();
                if self.settings.disabled.iter().any(|d| d == &id) {
                    self.settings.disabled.retain(|d| d != &id);
                    self.state.subscribed[idx] = true;
                    restored += 1;
                } else {
                    known += 1;
                }
                continue;
            }
            match catalog::add_custom(line) {
                Ok(idx) => {
                    let id = repos()[idx].id.to_string();
                    let slug = repos()[idx].slug.to_string();
                    if !self.settings.custom.iter().any(|s| s == &slug) {
                        self.settings.custom.push(slug);
                    }
                    self.settings.disabled.retain(|d| d != &id);
                    added += 1;
                }
                Err(e) => {
                    if e.contains("已经在订阅") {
                        known += 1;
                    } else {
                        bad.push(format!("{line} —— {e}"));
                    }
                }
            }
        }
        state::save_settings(&self.settings);
        if added > 0 || restored > 0 {
            self.rebuild_repos();
            self.pending_scroll_end = true;
        }
        let mut parts: Vec<String> = Vec::new();
        if added > 0 {
            parts.push(format!("新增 {added} 个"));
        }
        if restored > 0 {
            parts.push(format!("恢复订阅 {restored} 个"));
        }
        if known > 0 {
            parts.push(format!("{known} 个此前已经订阅"));
        }
        if !bad.is_empty() {
            parts.push(format!("{} 行无法识别", bad.len()));
        }
        let report = if parts.is_empty() {
            "没有可添加的地址".to_string()
        } else {
            format!("已导入：{}", parts.join("，"))
        };
        self.state.status = report.clone();
        if let Some(batch) = self.batch.as_mut() {
            batch.report = Some(if bad.is_empty() {
                report
            } else {
                format!("{report}\n\n{}", bad.join("\n"))
            });
        }
        (added + restored, known, bad.len())
    }

    /// Total height of the redesigned list view (header + grouped rows).
    fn list_height(&self, apps: &[usize]) -> f32 {
        let head_h = 34.0;
        let row_step = 56.0 + 6.0;
        let installed = apps
            .iter()
            .filter(|&&i| self.state.is_installed(i))
            .count();
        let missing = apps.len() - installed;
        let mut h = head_h + 6.0;
        if installed > 0 {
            h += installed as f32 * row_step;
        }
        if missing > 0 {
            h += SECTION_H + missing as f32 * row_step;
        }
        h + 8.0
    }

    /// Redesigned list view: sticky column header, installed/uninstalled grouping,
    /// elevated rounded row cards with hover lift, real GitHub descriptions,
    /// tinted status dots, and full-row click to open app details.
    fn list(&mut self, ui: &mut Ui, band: Rect, apps: &[usize], p: &Palette) {
        let head_h = 34.0;
        let row_h = 56.0;
        let row_gap = 6.0;
        let dark = p.bg.r() < 128;
        let green = Color32::from_rgb(0x3D, 0xC9, 0x7A);

        let left = band.min.x + MARGIN;
        let right = band.max.x - RIGHT_GUTTER;
        let row_w = (right - left).max(200.0);

        // Column right-anchors measured leftward from the row's right edge.
        let more_w = 30.0;
        let act_w = 88.0;
        let more_x = right - 12.0 - more_w;
        let act_x = more_x - 6.0 - act_w;
        let author_r = act_x - 20.0;
        let date_r = author_r - 96.0;
        let ver_r = date_r - 108.0;
        let state_r = ver_r - 106.0;

        // Sticky column header at the top of the band.
        let head_rect = Rect::from_min_max(band.min, pos2(band.max.x, band.min.y + head_h));
        let rows_band = Rect::from_min_max(pos2(band.min.x, head_rect.max.y), band.max);

        // Group installed first, uninstalled below a subtle divider.
        let installed: Vec<usize> = apps
            .iter()
            .copied()
            .filter(|&i| self.state.is_installed(i))
            .collect();
        let missing: Vec<usize> = apps
            .iter()
            .copied()
            .filter(|&i| !self.state.is_installed(i))
            .collect();

        let mut y = rows_band.min.y + 6.0 - self.state.scroll;
        for (group, dim) in [(&installed, false), (&missing, true)] {
            if group.is_empty() {
                continue;
            }
            if dim {
                let sec = Rect::from_min_size(pos2(left, y), vec2(row_w, SECTION_H));
                let vis = sec.intersect(rows_band);
                if vis.is_positive() {
                    let clip = ui.painter().with_clip_rect(vis);
                    let heading = format!("未安装 · {}", group.len());
                    clip.text(
                        pos2(sec.min.x + 2.0, sec.center().y),
                        Align2::LEFT_CENTER,
                        &heading,
                        label(11.5),
                        p.faint,
                    );
                    let hw = measure(ui, &heading, label(11.5));
                    clip.hline(
                        (sec.min.x + hw + 12.0)..=sec.max.x,
                        sec.center().y,
                        line(p.border),
                    );
                }
                y += SECTION_H;
            }

            for &idx in group {
                let row = Rect::from_min_size(pos2(left, y), vec2(row_w, row_h));
                y += row_h + row_gap;
                let vis = row.intersect(rows_band);
                if !vis.is_positive() {
                    continue;
                }

                let app = &repos()[idx];
                let clip = ui.painter().with_clip_rect(vis);
                let painter = &clip;

                let is_inst = self.state.is_installed(idx);
                let has_update = self.state.has_update(idx);
                let busy = self.state.busy(idx);

                // Full-row interaction excluding the right-side action buttons so clicks never bleed through.
                let row_click_area = Rect::from_min_max(vis.min, pos2((act_x - 8.0).max(vis.min.x), vis.max.y));
                let row_resp = if row_click_area.is_positive() {
                    ui.interact(
                        row_click_area,
                        ui.scope_id().with(("listrow", app.id)),
                        Sense::click(),
                    )
                } else {
                    ui.interact(vis, ui.scope_id().with(("listrow", app.id)), Sense::click())
                };
                let hovered = row_resp.hovered()
                    || ui
                        .input(|i| i.pointer.hover_pos())
                        .is_some_and(|pos| vis.contains(pos));
                if row_resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if row_resp.clicked() {
                    self.open_page(idx);
                }

                let row_bg = if hovered {
                    if dark {
                        Color32::from_rgb(0x16, 0x18, 0x1D)
                    } else {
                        Color32::from_rgb(0xF3, 0xF6, 0xFB)
                    }
                } else if dark {
                    Color32::from_rgb(0x0C, 0x0D, 0x0F)
                } else {
                    Color32::WHITE
                };
                let border_stroke = if hovered {
                    Stroke::new(1.5, p.accent)
                } else if has_update {
                    Stroke::new(
                        1.0,
                        Color32::from_rgba_unmultiplied(p.amber.r(), p.amber.g(), p.amber.b(), 140),
                    )
                } else {
                    line(p.border)
                };
                painter.rect_filled(row, radius(8), row_bg);
                painter.rect_stroke(row, radius(8), border_stroke, StrokeKind::Middle);

                // App icon
                let tile = Rect::from_min_size(
                    pos2(row.min.x + 12.0, row.center().y - 18.0),
                    vec2(36.0, 36.0),
                );
                self.draw_tile(painter, tile, idx, 9);

                // Name + real 1-line summary spanning up to the status column
                let text_x = tile.max.x + 12.0;
                let text_room = (state_r - 82.0 - text_x).max(60.0);
                let shown_name = fit_text(ui, app.name, display(15.5), text_room);
                painter.text(
                    pos2(text_x, row.center().y - 9.0),
                    Align2::LEFT_CENTER,
                    shown_name,
                    display(15.5),
                    p.text,
                );
                let summary = self.summary_text(idx);
                let shown_sum = fit_text(ui, &summary, text(12.0), text_room);
                painter.text(
                    pos2(text_x, row.center().y + 10.0),
                    Align2::LEFT_CENTER,
                    shown_sum,
                    text(12.0),
                    p.muted,
                );

                // Soft tinted status badge with dot
                let (st_word, st_color, filled_dot) = if has_update {
                    ("可更新", p.amber, true)
                } else if is_inst {
                    ("已安装", green, true)
                } else {
                    ("未安装", p.muted, false)
                };
                let pw = measure(ui, st_word, label(11.0)) + 26.0;
                let pill = Rect::from_min_max(
                    pos2(state_r - pw, row.center().y - 11.0),
                    pos2(state_r, row.center().y + 11.0),
                );
                painter.rect_filled(
                    pill,
                    radius(11),
                    Color32::from_rgba_unmultiplied(st_color.r(), st_color.g(), st_color.b(), 24),
                );
                let dot_c = pos2(pill.min.x + 10.0, pill.center().y);
                if filled_dot {
                    painter.circle_filled(dot_c, 3.0, st_color);
                } else {
                    painter.circle_stroke(dot_c, 3.0, Stroke::new(1.2, st_color));
                }
                painter.text(
                    pos2(pill.min.x + 18.0, pill.center().y),
                    Align2::LEFT_CENTER,
                    st_word,
                    label(11.0),
                    st_color,
                );

                // Version (with current -> latest hint when updatable)
                let (ver_str, date_str) = match &self.state.repos[idx].fetch {
                    FetchState::Pending => ("…".to_string(), "…".to_string()),
                    FetchState::Done(Ok(rel)) => {
                        let latest = version::clean_tag(&rel.tag).to_uppercase();
                        let v = if has_update {
                            if let Some(cur) = &self.state.repos[idx].installed {
                                format!("V{} → V{latest}", cur.to_uppercase())
                            } else {
                                format!("V{latest}")
                            }
                        } else {
                            format!("V{latest}")
                        };
                        (v, version::fmt_date(&rel.published))
                    }
                    FetchState::Done(Err(_)) => match &self.state.repos[idx].installed {
                        Some(v) => (format!("V{}", v.to_uppercase()), "—".to_string()),
                        None => ("—".to_string(), "—".to_string()),
                    },
                };
                painter.text(
                    pos2(ver_r, row.center().y),
                    Align2::RIGHT_CENTER,
                    fit_text(ui, &ver_str, label(12.0), 100.0),
                    label(12.0),
                    if has_update { p.amber } else { p.text },
                );
                painter.text(
                    pos2(date_r, row.center().y),
                    Align2::RIGHT_CENTER,
                    date_str,
                    text(12.5),
                    p.muted,
                );
                let author = catalog::owner_of(app.slug).to_uppercase();
                painter.text(
                    pos2(author_r, row.center().y),
                    Align2::RIGHT_CENTER,
                    fit_text(ui, &author, label(11.5), 86.0),
                    label(11.5),
                    p.faint,
                );

                // Action button with rounded progress fill
                let (phase, progress) = self.state.repos[idx]
                    .job
                    .as_ref()
                    .map(|j| (j.phase, j.progress))
                    .unwrap_or((JobPhase::Downloading, 0.0));
                let label_text = self.button_label(idx);
                let btn = Rect::from_min_size(pos2(act_x, row.center().y - 14.0), vec2(act_w, 28.0));
                let update_waiting = has_update && !busy;
                let pressed = if update_waiting {
                    primary_button(
                        ui,
                        painter,
                        btn,
                        egui::Id::unique(("row", app.id)),
                        &label_text,
                        12.5,
                        rows_band,
                        p,
                    )
                } else {
                    action_button(
                        ui,
                        painter,
                        btn,
                        egui::Id::unique(("row", app.id)),
                        &label_text,
                        12.5,
                        rows_band,
                        p,
                    )
                };
                if pressed {
                    self.act(idx);
                }
                if busy && phase == JobPhase::Downloading && progress > 0.0 {
                    let filled = Rect::from_min_max(
                        btn.min,
                        pos2(btn.min.x + btn.width() * progress, btn.max.y),
                    );
                    painter.rect_filled(filled, radius(5), p.surface);
                    painter.rect_stroke(btn, radius(5), line(p.border), StrokeKind::Middle);
                    painter.text(
                        btn.center(),
                        Align2::CENTER_CENTER,
                        &label_text,
                        text(12.5),
                        p.text,
                    );
                }

                // Overflow menu button
                let more =
                    Rect::from_min_size(pos2(more_x, row.center().y - 14.0), vec2(more_w, 28.0));
                if action_button(
                    ui,
                    painter,
                    more,
                    egui::Id::unique(("rowmore", app.id)),
                    "…",
                    13.0,
                    rows_band,
                    p,
                ) {
                    self.menu_for = if self.menu_for == Some(idx) {
                        None
                    } else {
                        Some(idx)
                    };
                    self.sort_open = false;
                }
                if self.menu_for == Some(idx) {
                    self.pending_menu = Some((more, idx));
                }

                // Subtle dim overlay for uninstalled rows when not hovered
                if dim && !hovered {
                    painter.rect_filled(
                        vis,
                        radius(8),
                        Color32::from_rgba_unmultiplied(p.bg.r(), p.bg.g(), p.bg.b(), 85),
                    );
                }
            }
        }

        // Paint sticky column header on top so scrolled rows slide cleanly underneath.
        let head_painter = ui.painter().with_clip_rect(head_rect);
        head_painter.rect_filled(head_rect, 0, p.bg);
        head_painter.hline(left..=right, head_rect.max.y - 0.5, line(p.border));
        let hy = head_rect.center().y;
        head_painter.text(
            pos2(left + 12.0, hy),
            Align2::LEFT_CENTER,
            "应用",
            label(11.0),
            p.faint,
        );
        for (word, xr) in [
            ("状态", state_r),
            ("版本", ver_r),
            ("更新日期", date_r),
            ("作者", author_r),
            ("操作", right - 12.0),
        ] {
            head_painter.text(pos2(xr, hy), Align2::RIGHT_CENTER, word, label(11.0), p.faint);
        }
    }

    // ---------------------------------------------------------- detail page

    /// One app's own page: what it is, what it does, and what its newest
    /// release changed. Everything sits in one scrolling column, laid out
    /// once to measure and again to draw.
    fn detail(&mut self, ui: &mut Ui, band: Rect, idx: usize, p: &Palette) {
        let content_h = self.detail_pass(ui, band, idx, p, false);
        self.detail_scroll = self.apply_wheel(ui, band, content_h, self.detail_scroll);
        self.detail_pass(ui, band, idx, p, true);
        self.detail_scroll =
            self.scrollbar(ui, band, content_h, self.detail_scroll, "detail", p);
    }

    /// The page's layout, run twice: once with `draw` off to learn how
    /// tall it is, then again to paint with the scroll applied.
    fn detail_pass(
        &mut self,
        ui: &mut Ui,
        band: Rect,
        idx: usize,
        p: &Palette,
        draw: bool,
    ) -> f32 {
        let app = &repos()[idx];
        let painter = ui.painter().with_clip_rect(band);
        // The content stops short of the scrollbar's own strip.
        let w = band.width() - DETAIL_MARGIN - RIGHT_GUTTER;
        let left = band.min.x + DETAIL_MARGIN;
        // The cursor runs in content space; the screen sits it under the
        // scroll offset.
        let scroll = self.detail_scroll;
        let at = |cy: f32| band.min.y + cy - scroll;
        let (phase, progress) = self
            .state
            .repos[idx]
            .job
            .as_ref()
            .map(|j| (j.phase, j.progress))
            .unwrap_or((JobPhase::Downloading, 0.0));
        let busy = self.state.busy(idx);

        // The release's pieces, gathered up front.
        enum Rel {
            Pending,
            Failed(String),
            Here {
                version: String,
                date: String,
                size: String,
                platform: String,
                changes: Vec<String>,
                url: String,
            },
        }
        let rel = match &self.state.repos[idx].fetch {
            FetchState::Pending => Rel::Pending,
            FetchState::Done(Err(e)) => Rel::Failed(e.clone()),
            FetchState::Done(Ok(r)) => Rel::Here {
                version: version::clean_tag(&r.tag).to_uppercase(),
                date: version::fmt_date(&r.published),
                size: r
                    .asset
                    .as_ref()
                    .map(|a| format!("{:.1} MB", a.size as f32 / 1_048_576.0))
                    .unwrap_or_else(|| "—".to_string()),
                platform: match r.asset.as_ref().map(|a| a.name.to_lowercase()) {
                    Some(n) if n.contains("universal") => "macOS 通用".to_string(),
                    Some(n) if n.contains("arm64") => "macOS arm64".to_string(),
                    _ => "macOS".to_string(),
                },
                changes: github::changelog_lines(&r.body),
                url: r.html_url.clone(),
            },
        };

        // ---------------------------------------------------------- header
        let icon = Rect::from_min_size(pos2(left, at(30.0)), vec2(72.0, 72.0));
        let text_x = left + 88.0;
        let version_line = match &rel {
            Rel::Here { version, date, .. } => format!("版本 V{version} · {date}"),
            Rel::Pending => "版本 …".to_string(),
            Rel::Failed(_) => match &self.state.repos[idx].installed {
                Some(v) => format!("版本 V{} · 未获取到发布日期", v.to_uppercase()),
                None => "版本 未知".to_string(),
            },
        };
        if draw {
            self.draw_tile(&painter, icon, idx, 14);
            painter.text(
                pos2(text_x, at(52.0)),
                Align2::LEFT_CENTER,
                app.name,
                display(30.0),
                p.text,
            );
            painter.text(
                pos2(text_x, at(84.0)),
                Align2::LEFT_CENTER,
                app.tagline,
                text(14.0),
                p.muted,
            );
            painter.text(
                pos2(text_x, at(108.0)),
                Align2::LEFT_CENTER,
                &version_line,
                label(12.0),
                p.accent,
            );
        }
        let mut cy = 140.0;

        // The action button and the little menu beside it, at the right.
        let more_w = 36.0;
        let more_rect = Rect::from_min_max(
            pos2(band.max.x - RIGHT_GUTTER - more_w, at(30.0)),
            pos2(band.max.x - RIGHT_GUTTER, at(66.0)),
        );
        let main_rect = Rect::from_min_max(
            pos2(more_rect.min.x - 8.0 - 150.0, at(30.0)),
            pos2(more_rect.min.x - 8.0, at(66.0)),
        );
        if draw {
            let label_text = self.button_label(idx).to_string();
            let update_waiting =
                self.state.has_update(idx) && self.state.is_installed(idx) && !busy;
            let pressed = if update_waiting {
                primary_button(
                    ui,
                    &painter,
                    main_rect,
                    egui::Id::unique(("dopen", app.id)),
                    &label_text,
                    15.0,
                    band,
                    p,
                )
            } else {
                action_button(
                    ui,
                    &painter,
                    main_rect,
                    egui::Id::unique(("dopen", app.id)),
                    &label_text,
                    15.0,
                    band,
                    p,
                )
            };
            if pressed {
                self.act(idx);
            }
            if busy && phase == JobPhase::Downloading && progress > 0.0 {
                let filled = Rect::from_min_max(
                    main_rect.min,
                    pos2(main_rect.min.x + main_rect.width() * progress, main_rect.max.y),
                );
                painter.rect_filled(filled, 0, p.surface);
                painter.rect_stroke(main_rect, 0, line(p.border), StrokeKind::Middle);
                painter.text(
                    pos2(main_rect.center().x, main_rect.center().y),
                    Align2::CENTER_CENTER,
                    label_text,
                    text(15.0),
                    p.text,
                );
            }
            if action_button(
                ui,
                &painter,
                more_rect,
                egui::Id::unique(("dmore", app.id)),
                "…",
                13.0,
                band,
                p,
            ) {
                self.menu_for = if self.menu_for == Some(idx) {
                    None
                } else {
                    Some(idx)
                };
                self.sort_open = false;
            }
            if self.menu_for == Some(idx) {
                self.pending_menu = Some((more_rect, idx));
            }
        }

        // The row of outline links: the site, the repo, the releases.
        let mut links: Vec<(&str, String)> = Vec::new();
        if let Some(site) = app.website {
            links.push(("网站", site.to_string()));
        }
        links.push(("GITHUB", format!("https://github.com/{}", app.slug)));
        links.push((
            "版本发布",
            match &rel {
                Rel::Here { url, .. } if !url.is_empty() => url.clone(),
                _ => format!("https://github.com/{}/releases", app.slug),
            },
        ));
        let mut lx = left;
        for (name, url) in &links {
            let lw = measure(ui, *name, text(13.0)) + 46.0;
            let r = Rect::from_min_size(pos2(lx, at(cy)), vec2(lw, 30.0));
            if draw {
                painter.rect_stroke(r, 0, line(p.border), StrokeKind::Middle);
                painter.text(
                    pos2(r.min.x + 14.0, r.center().y),
                    Align2::LEFT_CENTER,
                    *name,
                    text(13.0),
                    p.text,
                );
                arrow_out(
                    &painter,
                    Rect::from_center_size(
                        pos2(r.max.x - 14.0, r.center().y),
                        vec2(12.0, 12.0),
                    ),
                    p.muted,
                );
                let hit = r.intersect(band);
                if hit.is_positive() {
                    let resp = ui.interact(
                        hit,
                        ui.scope_id().with(("dlink", app.id, *name)),
                        Sense::click(),
                    );
                    if resp.clicked() {
                        let _ = std::process::Command::new("open").arg(url).spawn();
                        self.state.status = format!("已在浏览器打开 {}", app.name);
                    }
                }
            }
            lx += lw + 12.0;
        }
        cy += 30.0 + 26.0;

        // The divider before the sections.
        if draw {
            painter.hline(
                band.min.x + DETAIL_MARGIN..=band.max.x - RIGHT_GUTTER,
                at(cy),
                line(p.border),
            );
        }
        cy += 26.0;

        // ------------------------------------------------- 01 features
        if draw {
            painter.text(
                pos2(left, at(cy)),
                Align2::LEFT_CENTER,
                "01 / 功能特性",
                label(11.0),
                p.faint,
            );
        }
        cy += 30.0;
        let col_w = (w - 16.0) / 2.0;
        let card_h = 118.0;
        for (i, feature) in app.features.iter().enumerate() {
            let col = i % 2;
            let row = i / 2;
            let x = left + col as f32 * (col_w + 16.0);
            let y = cy + row as f32 * (card_h + 16.0);
            if draw {
                let r = Rect::from_min_size(pos2(x, at(y)), vec2(col_w, card_h));
                painter.rect_stroke(r, 0, line(p.border), StrokeKind::Middle);
                painter.text(
                    pos2(r.min.x + 16.0, at(y + 26.0)),
                    Align2::LEFT_CENTER,
                    format!("0{}", i + 1),
                    label(11.0),
                    p.accent,
                );
                painter.text(
                    pos2(r.min.x + 16.0, at(y + 50.0)),
                    Align2::LEFT_CENTER,
                    feature.title,
                    text(15.0),
                    p.text,
                );
                let galley = painter.layout(
                    feature.desc.to_string(),
                    text(13.0),
                    p.muted,
                    col_w - 32.0,
                );
                painter.galley(pos2(r.min.x + 16.0, at(y + 72.0)), galley, p.muted);
            }
        }
        let feature_rows = app.features.len().div_ceil(2);
        if app.features.is_empty() {
            if draw {
                painter.text(
                    pos2(left, at(cy)),
                    Align2::LEFT_CENTER,
                    "该仓库没有内置的功能介绍，版本与更新都在下面。",
                    text(13.0),
                    p.faint,
                );
            }
            cy += 30.0;
        }
        cy += feature_rows as f32 * (card_h + 16.0) + 26.0;

        // -------------------------------------------- 02 latest release
        if draw {
            painter.text(
                pos2(left, at(cy)),
                Align2::LEFT_CENTER,
                "02 / 最新发布",
                label(11.0),
                p.faint,
            );
        }
        cy += 30.0;
        match &rel {
            Rel::Pending | Rel::Failed(_) => {
                let word = match &rel {
                    Rel::Pending => "正在获取版本信息…".to_string(),
                    Rel::Failed(e) => format!("获取版本信息失败: {e}"),
                    _ => unreachable!(),
                };
                if draw {
                    painter.text(
                        pos2(left, at(cy)),
                        Align2::LEFT_CENTER,
                        word,
                        text(14.0),
                        p.muted,
                    );
                }
                cy += 30.0;
            }
            Rel::Here { version, date, size, platform, changes, url } => {
                // The four little info boxes.
                let boxes = [
                    ("版本", format!("V{version}")),
                    ("发布日期", date.clone()),
                    ("下载", size.clone()),
                    ("平台", platform.clone()),
                ];
                let bw = (w - 3.0 * 12.0) / 4.0;
                for (i, (cap, value)) in boxes.iter().enumerate() {
                    let x = left + i as f32 * (bw + 12.0);
                    let r = Rect::from_min_size(pos2(x, at(cy)), vec2(bw, 76.0));
                    if draw {
                        painter.rect_stroke(r, 0, line(p.border), StrokeKind::Middle);
                        painter.text(
                            pos2(r.min.x + 14.0, at(cy + 24.0)),
                            Align2::LEFT_CENTER,
                            *cap,
                            text(11.5),
                            p.faint,
                        );
                        painter.text(
                            pos2(r.min.x + 14.0, at(cy + 52.0)),
                            Align2::LEFT_CENTER,
                            value,
                            text(15.0),
                            p.text,
                        );
                    }
                }
                cy += 76.0 + 40.0;

                // The changelog, with the rest behind an expander.
                if draw {
                    painter.text(
                        pos2(left, at(cy)),
                        Align2::LEFT_CENTER,
                        "更新内容",
                        text(15.0),
                        p.text,
                    );
                }
                cy += 30.0;
                let shown = if self.detail_expanded || changes.len() <= 6 {
                    changes
                } else {
                    &changes[..6]
                };
                for change in shown {
                    let galley =
                        painter.layout(change.clone(), text(13.0), p.muted, w - 40.0);
                    let gh = galley.size().y;
                    if draw {
                        painter.galley(pos2(left + 12.0, at(cy) - 5.0), galley, p.muted);
                        painter.circle_filled(
                            pos2(left + 2.0, at(cy) + 5.0),
                            2.0,
                            p.faint,
                        );
                    }
                    cy += gh + 14.0;
                }
                if changes.len() > 6 {
                    let hidden = changes.len() - shown.len();
                    let entry_text: &str = if self.detail_expanded {
                        "完整更新日志"
                    } else {
                        &format!("▸ 显示另外 {hidden} 项更改")
                    };
                    if draw {
                        let r = Rect::from_min_size(
                            pos2(left + 12.0, at(cy)),
                            vec2(260.0, 22.0),
                        );
                        painter.text(
                            pos2(r.min.x, r.center().y),
                            Align2::LEFT_CENTER,
                            entry_text,
                            text(13.0),
                            p.accent,
                        );
                        let hit = r.intersect(band);
                        if hit.is_positive() {
                            let resp = ui.interact(
                                hit,
                                ui.scope_id().with(("dexpander", app.id)),
                                Sense::click(),
                            );
                            if resp.clicked() {
                                if self.detail_expanded {
                                    if !url.is_empty() {
                                        let _ =
                                            std::process::Command::new("open").arg(url).spawn();
                                    }
                                } else {
                                    self.detail_expanded = true;
                                }
                            }
                        }
                    }
                    cy += 30.0;
                    if self.detail_expanded && draw {
                        let r = Rect::from_min_size(
                            pos2(left + 12.0, at(cy)),
                            vec2(260.0, 22.0),
                        );
                        painter.text(
                            pos2(r.min.x, r.center().y),
                            Align2::LEFT_CENTER,
                            "完整更新日志",
                            text(13.0),
                            p.accent,
                        );
                        let hit = r.intersect(band);
                        if hit.is_positive() {
                            let resp = ui.interact(
                                hit,
                                ui.scope_id().with(("dchangelog", app.id)),
                                Sense::click(),
                            );
                            if resp.clicked() && !url.is_empty() {
                                let _ =
                                    std::process::Command::new("open").arg(url).spawn();
                            }
                        }
                    }
                    cy += if self.detail_expanded { 30.0 } else { 0.0 };
                } else if draw && !url.is_empty() {
                    painter.text(
                        pos2(left + 12.0, at(cy + 11.0)),
                        Align2::LEFT_CENTER,
                        "完整更新日志",
                        text(13.0),
                        p.accent,
                    );
                    let r = Rect::from_min_size(
                        pos2(left + 12.0, at(cy)),
                        vec2(260.0, 22.0),
                    );
                    let hit = r.intersect(band);
                    if hit.is_positive() {
                        let resp = ui.interact(
                            hit,
                            ui.scope_id().with(("dchangelog", app.id)),
                            Sense::click(),
                        );
                        if resp.clicked() {
                            let _ = std::process::Command::new("open").arg(url).spawn();
                        }
                    }
                    cy += 30.0;
                }
            }
        }

        cy + 48.0
    }

    // --------------------------------------------------------------- footer

    fn footer(&mut self, ui: &mut Ui, rect: Rect, p: &Palette) {
        let bar = Rect::from_min_max(
            pos2(rect.min.x, rect.max.y - FOOTER_H),
            pos2(rect.max.x, rect.max.y),
        );
        ui.painter()
            .hline(bar.min.x..=bar.max.x - 0.5, bar.min.y, line(p.border));
        ui.painter().text(
            pos2(bar.min.x + 20.0, bar.center().y),
            Align2::LEFT_CENTER,
            self.state.status.clone(),
            text(13.0),
            p.faint,
        );

        // One link, to the project itself; the other marks are gone.
        let url = "https://github.com/zdx8";
        let caption = "GITHUB";
        let w = measure(ui, caption, label(11.0)) + 34.0;
        let link = Rect::from_min_max(
            pos2(bar.max.x - 20.0 - w, bar.center().y - 10.0),
            pos2(bar.max.x - 20.0, bar.center().y + 10.0),
        );
        let hovered = link.contains(ui.input(|i| i.pointer.hover_pos().unwrap_or_default()));
        ui.painter().text(
            pos2(link.min.x, link.center().y),
            Align2::LEFT_CENTER,
            caption,
            label(11.0),
            if hovered { p.accent } else { p.faint },
        );
        arrow_out(
            ui.painter(),
            Rect::from_center_size(pos2(link.max.x - 8.0, link.center().y), vec2(11.0, 11.0)),
            if hovered { p.accent } else { p.faint },
        );
        let resp = ui.interact(link, egui::Id::unique("footer-github"), Sense::click());
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if resp.clicked() {
            let _ = std::process::Command::new("open").arg(url).spawn();
            self.state.status = "已在浏览器打开项目地址".to_string();
        }
    }
}

/// Where the fetched descriptions are kept between runs.
fn descriptions_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    std::path::PathBuf::from(home)
        .join("Library/Application Support/GitupLauncher/descriptions.json")
}

fn load_descriptions() -> std::collections::BTreeMap<String, String> {
    let Ok(text) = std::fs::read_to_string(descriptions_path()) else {
        return std::collections::BTreeMap::new();
    };
    let Ok(map) = serde_json::from_str::<std::collections::BTreeMap<String, String>>(&text) else {
        return std::collections::BTreeMap::new();
    };
    map
}

fn save_descriptions(map: &std::collections::BTreeMap<String, String>) {
    let path = descriptions_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, serde_json::to_string_pretty(map).unwrap_or_default());
}

/// The last release read for each repository, kept between runs.
fn releases_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    std::path::PathBuf::from(home)
        .join("Library/Application Support/GitupLauncher/releases.json")
}

fn load_releases() -> std::collections::BTreeMap<String, serde_json::Value> {
    let Ok(text) = std::fs::read_to_string(releases_path()) else {
        return std::collections::BTreeMap::new();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

fn save_releases(map: &std::collections::BTreeMap<String, serde_json::Value>) {
    let path = releases_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, serde_json::to_string_pretty(map).unwrap_or_default());
}

/// Cached HTTP ETags per repository, kept between runs to save GitHub API quota.
fn etags_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    std::path::PathBuf::from(home)
        .join("Library/Application Support/GitupLauncher/etags.json")
}

fn load_etags() -> std::collections::BTreeMap<String, String> {
    let Ok(text) = std::fs::read_to_string(etags_path()) else {
        return std::collections::BTreeMap::new();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

fn save_etags(map: &std::collections::BTreeMap<String, String>) {
    let path = etags_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, serde_json::to_string_pretty(map).unwrap_or_default());
}

/// The Chinese half of a description that carries two languages, e.g.
/// "中文简介 / An English summary". Anything else is left whole.
fn first_language(text: &str) -> &str {
    if let Some((head, tail)) = text.split_once(" / ") {
        let tail_is_english = tail
            .chars()
            .next()
            .map(|c| c.is_ascii_alphabetic())
            .unwrap_or(false);
        if tail_is_english {
            return head.trim();
        }
    }
    text
}

/// One of the outline buttons: hovered, it takes the accent colour in its
/// border, its fill and its caption; clicked, it says so. Everything else
/// about it stays as the picture has it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn action_button(
    ui: &mut Ui,
    painter: &egui::Painter,
    rect: Rect,
    id: egui::Id,
    caption: &str,
    size: f32,
    band: Rect,
    p: &Palette,
) -> bool {
    let hit = rect.intersect(band);
    let mut hovered = false;
    let mut clicked = false;
    if hit.is_positive() {
        let resp = ui.interact(hit, id, Sense::click());
        hovered = resp.hovered();
        clicked = resp.clicked();
        if hovered {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }
    if hovered {
        painter.rect_filled(rect, radius(5), p.surface);
    }
    painter.rect_stroke(
        rect,
        radius(5),
        if hovered {
            Stroke::new(1.5, p.accent)
        } else {
            line(p.border)
        },
        StrokeKind::Middle,
    );
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        caption,
        text(size),
        if hovered { p.accent } else { p.text },
    );
    clicked
}

/// The same button filled in: used for 更新, so a waiting update is the
/// one thing on the card that shouts.
#[allow(clippy::too_many_arguments)]
pub(crate) fn primary_button(
    ui: &mut Ui,
    painter: &egui::Painter,
    rect: Rect,
    id: egui::Id,
    caption: &str,
    size: f32,
    band: Rect,
    p: &Palette,
) -> bool {
    let hit = rect.intersect(band);
    let mut hovered = false;
    let mut clicked = false;
    if hit.is_positive() {
        let resp = ui.interact(hit, id, Sense::click());
        hovered = resp.hovered();
        clicked = resp.clicked();
        if hovered {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }
    let lighten = |c: Color32| {
        Color32::from_rgb(
            c.r().saturating_add(28),
            c.g().saturating_add(28),
            c.b().saturating_add(28),
        )
    };
    painter.rect_filled(
        rect,
        radius(5),
        if hovered { lighten(p.accent) } else { p.accent },
    );
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        caption,
        text(size),
        p.bg,
    );
    clicked
}

/// Responsive column count for the card grid.
fn grid_columns(avail_w: f32) -> usize {
    ((avail_w + CARD_GAP) / (CARD_MIN_W + CARD_GAP))
        .floor()
        .max(2.0) as usize
}

/// Clamps wrapped text to at most two lines, appending an ellipsis when truncated.
fn clamp_two_lines(ui: &Ui, text_in: &str, font: egui::FontId, wrap_w: f32) -> String {
    let full = ui
        .painter()
        .layout(text_in.to_string(), font.clone(), Color32::WHITE, wrap_w);
    if full.rows.len() <= 2 {
        return text_in.to_string();
    }
    let mut chars: Vec<char> = text_in.chars().collect();
    // Binary search the longest prefix that fits within 2 wrapped rows with "…".
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = (lo + hi + 1) / 2;
        let mut cand: String = chars[..mid].iter().collect();
        while cand.ends_with(char::is_whitespace) {
            cand.pop();
        }
        cand.push('…');
        let g = ui.painter().layout(cand, font.clone(), Color32::WHITE, wrap_w);
        if g.rows.len() <= 2 {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    chars.truncate(lo);
    let mut out: String = chars.into_iter().collect();
    while out.ends_with(char::is_whitespace) {
        out.pop();
    }
    out.push('…');
    out
}

/// A transfer rate, said the way a person reads it.
fn human_speed(bytes_per_sec: f32) -> String {
    if bytes_per_sec >= 1_048_576.0 {
        format!("{:.1} MB/s", bytes_per_sec / 1_048_576.0)
    } else if bytes_per_sec >= 1024.0 {
        format!("{:.0} KB/s", bytes_per_sec / 1024.0)
    } else {
        format!("{:.0} B/s", bytes_per_sec)
    }
}

/// A file size, for the progress the footer shows.
fn human_size(bytes: u64) -> String {
    let m = bytes as f32 / 1_048_576.0;
    if m >= 1.0 {
        format!("{m:.1} MB")
    } else {
        format!("{:.0} KB", bytes as f32 / 1024.0)
    }
}

/// The Mac's clipboard as text, read straight from `pbpaste` so pasting
/// works even if a keyboard shortcut never reaches the field.
fn clipboard_text() -> Option<String> {
    let out = std::process::Command::new("pbpaste").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?;
    let trimmed = text.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// Switches the window system's "a drag on the background moves the
/// window" on or off, while the window is already open. It has to be off
/// while the pointer is over a text box: with it on, AppKit claims the drag
/// before the field ever sees it, and selecting text moves the window
/// instead.
#[cfg(target_os = "macos")]
fn set_movable_by_background(movable: bool) -> bool {
    use std::ffi::{c_char, c_void};

    #[link(name = "objc")]
    extern "C" {
        fn objc_getClass(name: *const c_char) -> *mut c_void;
        fn sel_registerName(name: *const c_char) -> *mut c_void;
        fn objc_msgSend();
    }

    // SAFETY: `objc_msgSend` is called through function pointers whose
    // signatures match the messages being sent (two ids in, an id back;
    // two ids and a BOOL in, nothing back), which is how the Objective-C
    // runtime is meant to be reached.
    unsafe {
        let sel = |name: &[u8]| sel_registerName(name.as_ptr() as *const c_char);
        let get: extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
            std::mem::transmute(objc_msgSend as *const ());
        let app = get(
            objc_getClass(b"NSApplication\0".as_ptr() as *const c_char),
            sel(b"sharedApplication\0"),
        );
        if app.is_null() {
            return false;
        }
        // The key window while the app is in use; the main one, then the
        // first window in the list, when it is not.
        let mut window = get(app, sel(b"keyWindow\0"));
        if window.is_null() {
            window = get(app, sel(b"mainWindow\0"));
        }
        if window.is_null() {
            let windows = get(app, sel(b"windows\0"));
            if !windows.is_null() {
                window = get(windows, sel(b"firstObject\0"));
            }
        }
        if window.is_null() {
            return false;
        }
        let set: extern "C" fn(*mut c_void, *mut c_void, bool) =
            std::mem::transmute(objc_msgSend as *const ());
        set(window, sel(b"setMovableByWindowBackground:\0"), movable);
        if std::env::var("GITUP_DEBUG_DRAG").is_ok() {
            let read: extern "C" fn(*mut c_void, *mut c_void) -> bool =
                std::mem::transmute(objc_msgSend as *const ());
            let now = read(window, sel(b"isMovableByWindowBackground\0"));
            eprintln!("movable_by_background -> {movable}, reads back {now}");
        }
        true
    }
}

#[cfg(not(target_os = "macos"))]
fn set_movable_by_background(_movable: bool) -> bool {
    false
}

/// A string shortened with an ellipsis until it fits the given width.
pub(crate) fn fit_text(ui: &Ui, text_in: &str, font: egui::FontId, max_w: f32) -> String {
    if measure(ui, text_in, font.clone()) <= max_w {
        return text_in.to_string();
    }
    let mut chars: Vec<char> = text_in.chars().collect();
    while !chars.is_empty() {
        chars.pop();
        let candidate: String = chars.iter().collect::<String>() + "…";
        if measure(ui, &candidate, font.clone()) <= max_w {
            return candidate;
        }
    }
    "…".to_string()
}

/// The egui preference for one of the appearance settings.
pub(crate) fn theme_preference(mode: state::ThemeMode) -> egui::ThemePreference {
    match mode {
        state::ThemeMode::System => egui::ThemePreference::System,
        state::ThemeMode::Dark => egui::ThemePreference::Dark,
        state::ThemeMode::Light => egui::ThemePreference::Light,
    }
}

/// The caption for one of the ways the list can be sorted.
pub(crate) fn sort_option_label(sort: SortBy) -> &'static str {
    match sort {
        SortBy::Name => "名称 (A-Z)",
        SortBy::Updated => "最后更新",
    }
}

/// A one-pixel line in the given colour.
pub(crate) fn line(color: Color32) -> Stroke {
    Stroke::new(1.0, color)
}

/// The width of a piece of text, measured with the font it will be drawn in.
pub(crate) fn measure(ui: &Ui, text: &str, font: egui::FontId) -> f32 {
    ui.painter()
        .layout_no_wrap(text.to_string(), font, Color32::WHITE)
        .size()
        .x
}

fn main() -> eframe::Result<()> {
    let mut args = std::env::args().skip(1);
    let mut size = Vec2::new(1120.0, 760.0);
    let mut screenshot: Option<std::path::PathBuf> = None;
    let mut demo = Demo::None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--screenshot" => {
                screenshot = args.next().map(std::path::PathBuf::from);
            }
            "--demo" => {
                demo = match args.next().as_deref() {
                    Some("list") => Demo::List,
                    Some("menu") => Demo::Menu,
                    Some("sort") => Demo::Sort,
                    Some("search") => Demo::Search,
                    Some("detail") => Demo::Detail,
                    Some("settings") => Demo::Settings,
                    Some("hover") => Demo::Hover,
                    Some("picker") => Demo::Picker,
                    Some("batch") => Demo::Batch,
                    Some("batch-run") => Demo::BatchRun,
                    _ => Demo::None,
                };
            }
            "--size" => {
                if let Some(v) = args.next() {
                    let parts: Vec<f32> = v.split('x').filter_map(|p| p.parse().ok()).collect();
                    if parts.len() == 2 {
                        size = Vec2::new(parts[0], parts[1]);
                    }
                }
            }
            // Draws the app's own icon once, without opening a window.
            "--make-icon" => {
                if let Some(path) = args.next() {
                    let image = app_icon::render_icon(1024);
                    app_icon::write_png(std::path::Path::new(&path), &image);
                    return Ok(());
                }
            }
            // Checks one repository's newest release and downloads its
            // asset, without opening a window: the command-line answer to
            // "the list works but the download does not".
            "--fetch-test" => {
                if let Some(slug) = args.next() {
                    match github::fetch_latest_release(&slug, false, None, None) {
                        Ok(github::ReleaseFetch::Fresh { info: rel, .. }) => {
                            println!("tag:      {}", rel.tag);
                            match rel.asset {
                                Some(asset) => {
                                    println!("asset:    {} ({})", asset.name, human_size(asset.size));
                                    println!("api url:  {}", asset.url);
                                    println!("browser:  {}", asset.browser_url);
                                    let dest =
                                        std::env::temp_dir().join("gitup-fetch-test.bin");
                                    let cancel = AtomicBool::new(false);
                                    let started = std::time::Instant::now();
                                    let got = github::download_file(
                                        &[asset.url.clone(), asset.browser_url.clone()],
                                        &dest,
                                        None,
                                        &cancel,
                                        |_, _| {},
                                    );
                                    let bytes = std::fs::metadata(&dest)
                                        .map(|m| m.len())
                                        .unwrap_or(0);
                                    let secs = started.elapsed().as_secs_f32().max(0.001);
                                    match got {
                                        Ok(()) => println!(
                                            "download: ok, {bytes} bytes in {secs:.1}s ({})",
                                            human_speed(bytes as f32 / secs)
                                        ),
                                        Err(e) => println!("download: failed — {e}"),
                                    }
                                    let _ = std::fs::remove_file(&dest);
                                }
                                None => println!("this release has no macOS asset"),
                            }
                        }
                        Ok(github::ReleaseFetch::NotModified) => {
                            println!("release:  304 not modified");
                        }
                        Err(e) => println!("release:  failed — {e}"),
                    }
                    return Ok(());
                }
            }
            _ => {}
        }
    }

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Gitup Launcher")
        .with_inner_size(size)
        .with_min_inner_size(vec2(900.0, 600.0))
        // One bar of our own: the system titlebar is transparent and the
        // content runs underneath it, the title text is hidden, and the
        // traffic lights sit at the top-left, left of the wordmark.
        .with_title_shown(false)
        .with_titlebar_shown(false)
        .with_titlebar_buttons_shown(true)
        .with_fullsize_content_view(true)
        // The whole background drags the window, the way the original's
        // does. It is switched off at runtime while the pointer is over a
        // text box, so a drag there selects text instead of moving the
        // window (see `set_movable_by_background`).
        .with_movable_by_background(true);
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")) {
        viewport = viewport.with_icon(std::sync::Arc::new(icon));
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "Gitup Launcher",
        options,
        Box::new(move |cc| {
            let mut launcher = Launcher::new(cc);
            launcher.demo = demo;
            match screenshot {
                Some(path) => {
                    // A picture left over from an earlier run must not make
                    // the watchdog think the job is done already.
                    let _ = std::fs::remove_file(&path);
                    let arrived = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                    // A watchdog thread guarantees the process stops well
                    // before the checker's 90 s limit: as soon as the
                    // picture is on disk, or at 80 s at the very latest.
                    let watch = arrived.clone();
                    std::thread::spawn(move || {
                        for _second in 0..80 {
                            std::thread::sleep(std::time::Duration::from_secs(1));
                            if watch.load(std::sync::atomic::Ordering::SeqCst) {
                                std::process::exit(0);
                            }
                        }
                        eprintln!("screenshot watchdog: the picture never arrived");
                        std::process::exit(4);
                    });
                    Ok(Box::new(ScreenshotApp {
                        inner: App {
                            launcher,
                            demo_applied: false,
                            last_dark: None,
                        },
                        path,
                        frame: 0,
                        pass: 0,
                        // Frames to let the worker threads answer before the
                        // picture is taken; longer waits show live data.
                        warmup: std::env::var("GITUP_SHOT_FRAMES")
                            .ok()
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(10),
                        arrived,
                    }))
                }
                None => Ok(Box::new(App {
                    launcher,
                    demo_applied: false,
                    last_dark: None,
                })),
            }
        }),
    )
}

/// The same launcher, but it saves the window picture after a moment and quits.
struct ScreenshotApp {
    inner: App,
    path: std::path::PathBuf,
    frame: u32,
    /// Frames seen by the raw-input hook, which runs before each pass.
    pass: u32,
    warmup: u32,
    /// Set the moment the picture is on disk, so the watchdog knows it can
    /// stop the process right away.
    arrived: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl eframe::App for ScreenshotApp {
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        self.inner.clear_color(visuals)
    }

    /// Test hooks for looking at the window without a mouse: GITUP_HOVER_TEST
    /// parks the pointer somewhere, and GITUP_UI_TEST types into the
    /// add-repository field and then presses 添加, so the whole flow can be
    /// checked from a picture.
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if std::env::var("GITUP_PASTE_TEST").is_ok() && self.pass == 10 {
            raw_input.events.push(egui::Event::Paste(
                "https://github.com/diamondfsd/luna-ai-cut".to_string(),
            ));
        }
        // The real app's hook still runs, so a test exercises the real path.
        self.inner.raw_input_hook(ctx, raw_input);
        // A test hook for the bar's double-click: two clicks in quick
        // succession on the empty part of the bar. A picture taken after
        // them is the size of the screen when the zoom worked; the value
        // says how many double-clicks to make, so the way back can be
        // checked as well.
        if let Ok(times) = std::env::var("GITUP_DBLCLICK_TEST") {
            let times: u32 = times.parse().unwrap_or(1);
            let at = egui::pos2(600.0, 23.0);
            for k in 0..times {
                // Far enough apart that the second pair is a fresh double
                // click rather than the third click of the first one.
                let base = 12 + 32 * k;
                match self.pass {
                    pass if pass == base => {
                        raw_input.events.push(egui::Event::PointerMoved(at))
                    }
                    pass if pass == base + 1 || pass == base + 4 => raw_input
                        .events
                        .push(egui::Event::PointerButton {
                            pos: at,
                            button: egui::PointerButton::Primary,
                            pressed: true,
                            modifiers: egui::Modifiers::default(),
                        }),
                    pass if pass == base + 2 || pass == base + 5 => raw_input
                        .events
                        .push(egui::Event::PointerButton {
                            pos: at,
                            button: egui::PointerButton::Primary,
                            pressed: false,
                            modifiers: egui::Modifiers::default(),
                        }),
                    _ => {}
                }
            }
        }
        self.pass += 1;
        let click = |pass: u32, pos: egui::Pos2, raw_input: &mut egui::RawInput| {
            if self.pass == pass {
                raw_input.events.push(egui::Event::PointerMoved(pos));
            } else if self.pass == pass + 1 {
                raw_input.events.push(egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::default(),
                });
            } else if self.pass == pass + 2 {
                raw_input.events.push(egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::default(),
                });
            }
        };
        if let Ok(where_) = std::env::var("GITUP_HOVER_TEST") {
            if self.pass == 30 {
                let mut parts = where_.split(',');
                let x: f32 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
                let y: f32 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
                raw_input.events.push(egui::Event::PointerMoved(egui::pos2(x, y)));
            }
            return;
        }
        if std::env::var("GITUP_UI_TEST").is_ok() {
            // A click on the importer's 添加 button: the panel should close
            // itself and the footer say what happened.
            click(20, egui::pos2(818.0, 463.0), raw_input);
        }
    }

    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        // A test hook: with GITUP_READD_TEST set, a built-in subscription is
        // switched off and its address is added again, which must put it
        // back on rather than answer "already subscribed".
        if std::env::var("GITUP_READD_TEST").is_ok() && self.frame == 6 {
            let idx = 2usize;
            let slug = repos()[idx].slug.to_string();
            let id = repos()[idx].id.to_string();
            if !self
                .inner
                .launcher
                .settings
                .disabled
                .iter()
                .any(|d| d == &id)
            {
                self.inner.launcher.toggle_subscription(idx);
            }
            let off = self.inner.launcher.state.subscribed[idx];
            let (added, known, bad) = self.inner.launcher.import_repositories(&slug);
            let on = self.inner.launcher.state.subscribed[idx];
            eprintln!(
                "readd {slug}: subscribed_before_import={off} -> added={added} known={known} bad={bad} subscribed_now={on}"
            );
        }
        // A test hook: with GITUP_DEL_TEST set, a repository the person
        // added is removed on one frame, so a picture can show whether the
        // settings page stays where it is.
        if std::env::var("GITUP_DEL_TEST").is_ok() && self.frame == 6 {
            let custom = repos()
                .iter()
                .position(|repo| !catalog::is_builtin(repo.slug));
            if let Some(idx) = custom {
                self.inner.launcher.remove_repository(idx);
            }
        }
        // A test hook: with GITUP_NAV_TEST set, one built-in subscription is
        // switched off and the detail page's arrows are walked, printing
        // where they land — a removed subscription must never appear.
        if std::env::var("GITUP_NAV_TEST").is_ok() && self.frame == 6 {
            self.inner.launcher.toggle_subscription(1);
            let mut idx = 0usize;
            let mut trail = Vec::new();
            for _ in 0..14 {
                self.inner.launcher.step_page(idx, true);
                if let Page::Detail(next) = self.inner.launcher.page {
                    idx = next;
                    trail.push(format!(
                        "{}({})",
                        repos()[next].name,
                        if self.inner.launcher.state.subscribed[next] {
                            "sub"
                        } else {
                            "NOT"
                        }
                    ));
                }
            }
            eprintln!("nav walk: {}", trail.join(" → "));
            self.inner.launcher.toggle_subscription(1);
        }
        // A test hook: with GITUP_TYPE_TEST set, the search field is focused
        // and fed some text on one frame, so a picture can show whether it
        // really takes keystrokes.
        if std::env::var("GITUP_TYPE_TEST").is_ok() {
            self.frame += 1;
            if self.frame == 20 {
                let id = egui::Id::unique("toolbar-search");
                ui.ctx().memory_mut(|m| m.request_focus(id));
                ui.ctx().input_mut(|i| {
                    i.events
                        .push(egui::Event::Text("luna".to_string()));
                });
            }
            self.frame -= 1;
        }
        self.inner.ui(ui, frame);
        self.frame += 1;
        if self.frame < 5 {
            // A picture must be taken even if another window comes to the
            // front: ask for the window to be shown and focused, so the
            // frames keep coming and the request below is not left waiting.
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        if self.frame >= self.warmup {
            // Ask afresh on every frame until the picture is saved. The
            // window system stops painting a window that is covered, and a
            // request made on the last painted frame would then never be
            // answered; asking again each frame means the first painted
            // frame after the warm-up gets it.
            {
                let path = self.path.clone();
                let shared = self.arrived.clone();
                // The picture must be on disk before the process stops, so
                // the exit happens inside the callback, right after the write.
                ui.ctx().request_screenshot(move |image| {
                    if shared.swap(true, std::sync::atomic::Ordering::SeqCst) {
                        return;
                    }
                    app_icon::write_png(&path, &image);
                    std::process::exit(0);
                });
            }
        }
        // Keep painting until the callback above has written the picture and
        // stopped the process. The program is never asked to close here: on
        // some runs that closed the window before the picture arrived, and
        // the watchdog then ended it with no picture at all.
        ui.ctx().request_repaint();
    }
}
