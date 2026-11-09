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

mod catalog;
mod fonts;
mod github;
mod icon;
mod install;
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

/// The height of the top bar. The picture puts its lower rule at 47 pt.
const TOPBAR_H: f32 = 47.0;
/// The height of the toolbar. The picture puts the search box's lower rule
/// at 99 pt, and the cards start at 124 pt.
const TOOLBAR_H: f32 = 77.0;
/// The height of the footer strip. The picture puts its upper rule at 726 pt.
const FOOTER_H: f32 = 34.0;
/// Where the wordmark starts, clear of the traffic lights.
const WORDMARK_X: f32 = 92.0;
/// The height of the strip at the top of a card (its rule is at 163 pt).
const CARD_HEAD_H: f32 = 39.0;
/// The width of one card's app tile.
const TILE: f32 = 56.0;
/// How many cards sit side by side.
const COLUMNS: usize = 3;
/// The grid's margin at each side; the picture puts the edges at 20 pt.
const MARGIN: f32 = 20.0;
/// The side margin of the detail page.
const DETAIL_MARGIN: f32 = 20.0;
/// How much room the detail page keeps clear on its right, so its own
/// scrollbar never sits on a button.
const RIGHT_GUTTER: f32 = 34.0;

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
}

/// What the window is showing: the grid, one app's page, or settings.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Grid,
    Detail(usize),
    Settings,
}

struct Launcher {
    state: State,
    /// The screen shown first, for looking at the other views.
    demo: Demo,
    /// Where the little menu is drawn, once the cards are done.
    pending_menu: Option<(Rect, usize)>,
    /// Where the sort dropdown is drawn, once the cards are done.
    pending_sort: Option<Rect>,
    search: String,
    /// The card whose small menu is open.
    menu_for: Option<usize>,
    /// Which sort the dropdown is showing.
    sort_open: bool,
    /// What the worker threads report back through.
    tx: std::sync::mpsc::Sender<Msg>,
    rx: std::sync::mpsc::Receiver<Msg>,
    /// A handle the workers use to ask for another frame.
    ctx: egui::Context,
    /// Each app's own icon, once it has been read off the disk; a card
    /// falls back to its drawn mark until then.
    icons: Vec<Option<egui::TextureHandle>>,
    /// For each icon, the texture coordinates of the artwork inside its
    /// transparent padding, so the picture can fill the whole tile.
    icon_uv: Vec<(f32, f32, f32, f32)>,
    /// What the window is showing.
    page: Page,
    /// How far the detail page is scrolled down.
    detail_scroll: f32,
    /// Whether the detail page's changelog shows every entry.
    detail_expanded: bool,
    /// What the person has set, kept between runs.
    settings: state::Settings,
    /// What is typed in the add-repository box on the settings page.
    settings_input: String,
    /// Each repository's own description, kept on disk so the API is
    /// asked at most once per repository.
    descriptions: std::collections::BTreeMap<String, String>,
    /// The last release read for each repository, kept on disk. It is what
    /// the cards show while the network is still answering, and what they
    /// fall back to when GitHub refuses to answer at all.
    releases: std::collections::BTreeMap<String, serde_json::Value>,
    /// A card to draw as though the pointer were over it. Only set by the
    /// hover demo, so the accent frame can be checked in a picture.
    hover_demo: Option<usize>,
    /// Why the last attempt to add a repository was refused, if it was.
    settings_error: Option<String>,
    /// Set after a repository is added, so the grid scrolls to the end and
    /// the new card is actually on screen.
    pending_scroll_end: bool,
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
        state.rescan_installed();
        let mut launcher = Self {
            state,
            demo: Demo::None,
            pending_menu: None,
            pending_sort: None,
            search: String::new(),
            menu_for: None,
            sort_open: false,
            tx,
            rx,
            ctx: cc.egui_ctx.clone(),
            icons: repos().iter().map(|_| None).collect(),
            icon_uv: repos().iter().map(|_| (0.0, 0.0, 1.0, 1.0)).collect(),
            page: Page::Grid,
            detail_scroll: 0.0,
            detail_expanded: false,
            settings,
            settings_input: String::new(),
            descriptions: load_descriptions(),
            releases: load_releases(),
            hover_demo: None,
            settings_error: None,
            pending_scroll_end: false,
        };
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
        } else {
            launcher.state.status = "未自动检查更新，点左上角按钮立即检查".to_string();
        }
        launcher.spawn_icon_load();
        launcher
    }

    /// Whether this repository is subscribed, and a toggle that keeps the
    /// setting file in step.
    fn toggle_subscription(&mut self, idx: usize) {
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

    /// Leave whatever pop-ups are open and show an app's page.
    fn open_page(&mut self, idx: usize) {
        self.page = Page::Detail(idx);
        self.detail_scroll = 0.0;
        self.detail_expanded = false;
        self.menu_for = None;
        self.sort_open = false;
    }

    /// One app's tile, wherever it is drawn. The app's own icon covers
    /// the whole tile with no colour behind it; an app without one gets
    /// its first letter on a colour drawn from its id.
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

    /// Read every installed app's own icon, off the window's thread. The
    /// answers arrive as `Msg::Icon` and become textures on the next
    /// frame; an app without an installed copy keeps its drawn mark.
    fn spawn_icon_load(&mut self) {
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        let bundles: Vec<(usize, String)> = repos()
            .iter()
            .enumerate()
            .filter(|(i, _)| self.state.is_installed(*i))
            .map(|(i, repo)| (i, repo.bundle.to_string()))
            .collect();
        std::thread::spawn(move || {
            for (idx, bundle) in bundles {
                let Some(bytes) = install::bundle_icon_png(&bundle) else {
                    continue;
                };
                if let Ok((width, height, mut rgba)) = icon::decode_png_rgba(&bytes) {
                    // An icon that ships on a white canvas would otherwise
                    // drop a white card into the tile.
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

    /// Ask GitHub about every repository, each on its own thread.
    fn spawn_meta_fetch_all(&mut self) {
        self.state.set_all_fetching();
        self.state.status = "正在检查更新…".to_string();
        let include_pre = self.settings.include_pre;
        for idx in 0..repos().len() {
            if !self.state.subscribed[idx] {
                continue;
            }
            let tx = self.tx.clone();
            let ctx = self.ctx.clone();
            let slug = repos()[idx].slug.to_string();
            let cached = self.descriptions.contains_key(&slug);
            std::thread::spawn(move || {
                let result = github::fetch_latest_release(&slug, include_pre);
                let _ = tx.send(Msg::Meta { idx, result });
                if !cached {
                    if let Some(text) = github::fetch_repo_description(&slug) {
                        let _ = tx.send(Msg::Description { slug, text });
                    }
                }
                ctx.request_repaint();
            });
        }
    }


    /// Ask GitHub about one repository only, on its own thread.
    fn spawn_meta_one(&mut self, idx: usize) {
        let list = repos();
        if idx >= list.len() {
            return;
        }
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        let slug = list[idx].slug.to_string();
        let include_pre = self.settings.include_pre;
        let cached = self.descriptions.contains_key(&slug);
        std::thread::spawn(move || {
            let result = github::fetch_latest_release(&slug, include_pre);
            let _ = tx.send(Msg::Meta { idx, result });
            if !cached {
                if let Some(text) = github::fetch_repo_description(&slug) {
                    let _ = tx.send(Msg::Description { slug, text });
                }
            }
            ctx.request_repaint();
        });
    }

    /// Keep the per-row side tables the same length as the live list.
    fn sync_arrays(&mut self) {
        let n = repos().len();
        self.icons.resize(n, None);
        self.icon_uv.resize(n, (0.0, 0.0, 1.0, 1.0));
    }

    /// Rebuild every per-row table after the live list has changed shape.
    /// Any running download is cancelled first, so no late message lands
    /// on the wrong row.
    fn rebuild_repos(&mut self) {
        for repo in self.state.repos.iter() {
            if let Some(job) = &repo.job {
                job.cancel.store(true, Ordering::SeqCst);
            }
        }
        let mut fresh = State::new();
        fresh.layout = self.state.layout;
        fresh.sort = self.state.sort;
        for (i, repo) in repos().iter().enumerate() {
            fresh.subscribed[i] = !self.settings.disabled.iter().any(|d| d == repo.id);
        }
        fresh.rescan_installed();
        self.state = fresh;
        self.icons = repos().iter().map(|_| None).collect();
        self.icon_uv = repos().iter().map(|_| (0.0, 0.0, 1.0, 1.0)).collect();
        self.page = Page::Grid;
        self.spawn_meta_fetch_all();
        self.spawn_icon_load();
    }

    /// Add a repository typed into the settings page. It joins the list
    /// by the same rules as everything else: its newest release is read
    /// straight away, its installed copy is looked for, and its icon is
    /// loaded if the app is already here.
    fn add_repository(&mut self, input: &str) {
        match catalog::add_custom(input) {
            Ok(idx) => {
                self.settings_error = None;
                let slug = repos()[idx].slug.to_string();
                self.settings.custom.push(slug.clone());
                state::save_settings(&self.settings);
                self.sync_arrays();
                let installed = install::installed_version(repos()[idx].bundle);
                self.state.repos.push(state::RepoState {
                    fetch: FetchState::Pending,
                    installed,
                    job: None,
                });
                self.state.subscribed.push(true);
                self.state.status =
                    format!("已添加订阅 {}（新卡片在列表末尾）", repos()[idx].name);
                self.pending_scroll_end = true;
                self.settings_input.clear();
                self.spawn_meta_one(idx);
                self.spawn_icon_load();
            }
            Err(e) => {
                self.state.status = e.clone();
                self.settings_error = Some(e);
            }
        }
    }

    /// Remove a repository the person added. Built-ins are only ever
    /// switched off, never deleted.
    fn remove_repository(&mut self, idx: usize) {
        let list = repos();
        if idx >= list.len() || catalog::is_builtin(list[idx].slug) {
            return;
        }
        if self.state.busy(idx) {
            self.state.status = "该仓库正在下载，请稍后再移除".to_string();
            return;
        }
        let slug = list[idx].slug.to_string();
        catalog::remove_custom(&slug);
        self.settings.custom.retain(|s| s != &slug);
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
    fn button_label(&self, idx: usize) -> &str {
        if let Some(job) = self.state.repos[idx].job.as_ref() {
            if job.phase == JobPhase::Installing {
                "正在安装…"
            } else {
                "正在下载…"
            }
        } else if self.state.has_update(idx) {
            "更新"
        } else if self.state.is_installed(idx) {
            "打开"
        } else {
            "安装"
        }
    }

    /// Take whatever the worker threads have said since the last frame.
    fn handle_messages(&mut self) {
        let mut done_seen = false;
        while let Ok(msg) = self.rx.try_recv() {
            // A list resize may have left an old answer on the way; it
            // must not land on whatever row happens to sit at that index.
            let idx_ok = |idx: usize, repos: &Vec<state::RepoState>| idx < repos.len();
            match msg {
                Msg::Meta { idx, result } if idx_ok(idx, &self.state.repos) => {
                    let slug = repos()[idx].slug.to_string();
                    match result {
                        Ok(rel) => {
                            // Fresh from GitHub: keep it, so the next run
                            // and any rate-limited spell can use it.
                            self.releases
                                .insert(slug, github::to_json(&rel));
                            save_releases(&self.releases);
                            self.state.apply_meta(idx, Ok(rel));
                        }
                        Err(e) => match self
                            .releases
                            .get(&slug)
                            .and_then(github::from_json)
                        {
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
                Msg::Done { idx, result } if idx_ok(idx, &self.state.repos) => {
                    let name = repos()[idx].name;
                    match result {
                        Ok(ver) => {
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
                Msg::Description { slug, text } => {
                    self.descriptions.insert(slug, text);
                    save_descriptions(&self.descriptions);
                }
                // Anything arriving for a row that no longer exists is
                // simply dropped.
                _ => {}
            }
        }
        if done_seen {
            self.state.rescan_installed();
            // A freshly installed app can now show its own icon.
            self.spawn_icon_load();
        }
    }

    /// Download the newest release and put it in /Applications.
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
        let bundle = repos()[idx].bundle.to_string();
        std::thread::spawn(move || {
            let dir = std::env::temp_dir().join("gitup-launcher/download");
            let _ = std::fs::create_dir_all(&dir);
            let file = dir.join(&asset.name);
            let _ = std::fs::remove_file(&file);
            let result = github::download_file(&asset.url, &file, &cancel, |done, total| {
                let _ = tx.send(Msg::Progress { idx, done, total });
                ctx.request_repaint();
            })
            .and_then(|_| {
                let _ = tx.send(Msg::Installing { idx });
                ctx.request_repaint();
                install::install_asset(&file, asset.kind, &bundle)
            });
            let _ = std::fs::remove_file(&file);
            let result = result.map(|_| {
                install::installed_version(&bundle).unwrap_or_default()
            });
            let _ = tx.send(Msg::Done { idx, result });
            ctx.request_repaint();
        });
    }

    /// Open the installed app.
    fn open_app(&mut self, idx: usize) {
        match install::open_app(repos()[idx].bundle) {
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
    fn copy_link(&mut self, idx: usize) {
        let url = format!("https://github.com/{}", repos()[idx].slug);
        self.ctx.copy_text(url);
        self.state.status = "已复制仓库链接".to_string();
    }

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
}

impl eframe::App for App {
    /// Command-V while the settings page is open. The field is the only
    /// text in the window, so the paste goes there whatever happens to have
    /// the keyboard, and the field is given the focus afterwards. The
    /// window manager's own paste event, when it arrives, is consumed here
    /// so nothing is pasted twice.
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if self.launcher.page != Page::Settings {
            return;
        }
        let mut pasted: Option<String> = None;
        let mut cmd_v = false;
        for event in &raw_input.events {
            match event {
                egui::Event::Paste(text) => pasted = Some(text.clone()),
                egui::Event::Key {
                    key: egui::Key::V,
                    pressed: true,
                    modifiers,
                    ..
                } if modifiers.command => cmd_v = true,
                _ => {}
            }
        }
        let text = match pasted {
            Some(text) => Some(text),
            None if cmd_v => clipboard_text(),
            None => None,
        };
        let Some(text) = text else {
            return;
        };
        self.launcher.settings_input = text.trim().to_string();
        ctx.memory_mut(|m| m.request_focus(egui::Id::unique("settings-add-field")));
        raw_input.events.retain(|event| {
            !matches!(event, egui::Event::Paste(_))
                && !matches!(
                    event,
                    egui::Event::Key {
                        key: egui::Key::V,
                        pressed: true,
                        modifiers,
                        ..
                    } if modifiers.command
                )
        });
    }


    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        // The window's own backdrop, matching the page colour.
        egui::Color32::from_rgb(0x12, 0x13, 0x16).to_normalized_gamma_f32()
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let dark = ctx.theme() == egui::Theme::Dark;
        let p = palette(dark);

        {
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

        // The demo screens.
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
            Demo::None => {}
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
            });
    }
}

impl Launcher {
    // ------------------------------------------------------------- top bar

    fn top_bar(&mut self, ui: &mut Ui, rect: Rect, p: &Palette) {
        let bar = Rect::from_min_max(rect.min, pos2(rect.max.x, rect.min.y + TOPBAR_H));
        ui.painter().rect_filled(bar, 0, p.bg);
        ui.painter()
            .hline(bar.min.x..=bar.max.x - 0.5, 47.0, line(p.border));

        let mid = 23.0;

        match self.page {
            Page::Grid => {
                // The wordmark, sitting clear of the traffic lights: the
                // bird mark and the letters, drawn here rather than taken
                // from anyone's artwork.
                draw_g_mark(
                    ui.painter(),
                    Rect::from_center_size(pos2(WORDMARK_X + 9.0, mid), vec2(18.0, 16.0)),
                    p.text,
                );
                ui.painter().text(
                    pos2(WORDMARK_X + 19.0, mid + 0.5),
                    Align2::LEFT_CENTER,
                    "ITUP LAUNCHER",
                    display(24.0),
                    p.text,
                );

                // Nothing more on the left: the wordmark stands alone.
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
                // The detail page's own header: a caption at the left, and
                // the little navigation cluster at the right.
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

        // A divider before the cog at the far right. It rides the right
        // edge, wherever the window's edge now is.
        let gear_x = rect.max.x - 30.0;
        ui.painter()
            .vline(rect.max.x - 48.0, 0.0..=47.0, line(p.border));

        // The cog button, which opens the settings page.
        let gear_rect = Rect::from_center_size(pos2(gear_x, mid), vec2(22.0, 22.0));
        let resp = ui.interact(gear_rect, ui.scope_id().with("gear"), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(gear_rect, radius(4), p.surface);
        }
        if resp.clicked() {
            self.page = Page::Settings;
            self.menu_for = None;
            self.sort_open = false;
        }
        gear_icon(ui.painter(), gear_rect, p.muted);
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
                    0 => {
                        let next = (idx + repos().len() - 1) % repos().len();
                        self.open_page(next);
                    }
                    1 => {
                        let next = (idx + 1) % repos().len();
                        self.open_page(next);
                    }
                    _ => self.page = Page::Grid,
                }
            } else if which == 2 {
                self.page = Page::Grid;
            }
        }
    }

    /// The settings page's close cell, at the far right of its header.
    fn draw_close_button(&mut self, ui: &mut Ui, rect: Rect, mid: f32, p: &Palette) {
        let nav_end = rect.max.x - 56.0;
        self.draw_nav_cell(ui, 2, nav_end - 28.0, mid, p, 28.0);
    }

    // -------------------------------------------------------------- toolbar

    fn toolbar(&mut self, ui: &mut Ui, rect: Rect, p: &Palette) {
        // The small square button at the far left. The picture puts it at
        // 20..54 pt across and 66..99 pt down. One click asks GitHub about
        // every repository again.
        let btn = Rect::from_min_max(pos2(MARGIN, 66.0), pos2(54.0, 99.0));
        ui.painter()
            .rect_stroke(btn, 0, line(p.border), StrokeKind::Middle);
        refresh_icon(
            ui.painter(),
            Rect::from_center_size(btn.center(), vec2(16.0, 16.0)),
            p.muted,
        );
        let resp = ui.interact(btn, ui.scope_id().with("check-updates"), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(btn, radius(4), p.surface);
            refresh_icon(
                ui.painter(),
                Rect::from_center_size(btn.center(), vec2(16.0, 16.0)),
                p.text,
            );
        }
        if resp.clicked() {
            self.spawn_meta_fetch_all();
        }

        // On the right: the view switch, then searching, then sorting. All
        // of them hang off the right edge, so they stay put however wide
        // the window is made.
        let right = rect.max.x - MARGIN;

        // The view switch: two cells, the chosen one filled. The cells are
        // a little wider than a bare tap target so the marks inside can be
        // drawn large.
        let toggle_w = 78.0;
        let toggle_h = 33.0;
        let toggle =
            Rect::from_min_max(pos2(right - toggle_w, 66.0), pos2(right, 66.0 + toggle_h));
        let grid_rect = Rect::from_min_max(toggle.min, pos2(toggle.center().x, toggle.max.y));
        let list_rect = Rect::from_min_max(pos2(toggle.center().x, toggle.min.y), toggle.max);
        ui.painter().rect_filled(toggle, 0, p.bg);
        let hot = match self.state.layout {
            View::Grid => grid_rect,
            View::List => list_rect,
        };
        ui.painter().rect_filled(hot, 0, p.text);
        ui.painter()
            .rect_stroke(toggle, 0, line(p.border), StrokeKind::Middle);
        grid_icon(
            ui.painter(),
            Rect::from_center_size(grid_rect.center(), vec2(19.0, 19.0)),
            if self.state.layout == View::Grid {
                p.bg
            } else {
                p.muted
            },
        );
        list_icon(
            ui.painter(),
            Rect::from_center_size(list_rect.center(), vec2(19.0, 19.0)),
            if self.state.layout == View::List {
                p.bg
            } else {
                p.muted
            },
        );
        let resp = ui.interact(toggle, ui.scope_id().with("toggle"), Sense::click());
        if resp.clicked() {
            self.state.layout = match self.state.layout {
                View::Grid => View::List,
                View::List => View::Grid,
            };
        }

        // The search box, a hand's width left of the toggle.
        let search_w = 206.0;
        let search_h = 33.0;
        let search_rect = Rect::from_min_max(
            pos2(toggle.min.x - 6.0 - search_w, 66.0),
            pos2(toggle.min.x - 6.0, 66.0 + search_h),
        );
        // The picture's box is filled near-black inside its border.
        ui.painter()
            .rect_filled(search_rect, 0, p.search_fill);
        ui.painter()
            .rect_stroke(search_rect, 0, line(p.border), StrokeKind::Middle);
        // The hint sits at the left edge, on the box's middle line: the
        // field's own hint would do the same, but drawing it keeps the
        // spacing exact.
        if self.search.is_empty() {
            ui.painter().text(
                pos2(search_rect.min.x + 12.0, search_rect.center().y),
                Align2::LEFT_CENTER,
                "搜索应用",
                text(13.0),
                p.muted,
            );
        }
        let edit = TextEdit::singleline(&mut self.search)
            .id(egui::Id::unique("toolbar-search"))
            .text_color(p.text)
            .frame(egui::Frame::NONE)
            .desired_width(search_rect.width() - 16.0);
        // The box is drawn by hand, so the real field sits on top of it,
        // riding the box's middle line.
        let mut child = ui.new_child(
            egui::UiBuilder::new().max_rect(Rect::from_min_max(
                pos2(search_rect.min.x + 8.0, search_rect.center().y - 9.5),
                pos2(search_rect.max.x - 8.0, search_rect.center().y + 9.5),
            )),
        );
        child.add(edit);

        // The sort control: a label and a bordered box, left of the search.
        let sort_w = 190.0;
        let sort_rect = Rect::from_min_max(
            pos2(search_rect.min.x - 8.0 - sort_w, 66.0),
            pos2(search_rect.min.x - 8.0, 66.0 + search_h),
        );
        ui.painter()
            .rect_stroke(sort_rect, 0, line(p.border), StrokeKind::Middle);
        let sort_name = match self.state.sort {
            SortBy::Name => "名称 (A-Z)",
            SortBy::Updated => "最后更新",
        };
        ui.painter().text(
            pos2(sort_rect.min.x + 12.0, sort_rect.center().y),
            Align2::LEFT_CENTER,
            sort_name,
            text(14.0),
            p.text,
        );
        caret_down(
            ui.painter(),
            Rect::from_center_size(
                pos2(sort_rect.max.x - 17.0, sort_rect.center().y),
                vec2(11.0, 11.0),
            ),
            p.text,
        );
        let resp_sort = ui.interact(sort_rect, ui.scope_id().with("sort"), Sense::click());
        if resp_sort.clicked() {
            self.sort_open = !self.sort_open;
            self.menu_for = None;
        }

        // The "排序" label, just left of the box.
        ui.painter().text(
            pos2(sort_rect.min.x - 4.5, sort_rect.center().y + 0.5),
            Align2::RIGHT_CENTER,
            "排序",
            text(13.0),
            p.faint,
        );

        // The dropdown is drawn last, on top of the cards, so only its
        // position is recorded here.
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
            ui.painter().text(
                pos2(rect.center().x, grid_top + 60.0),
                Align2::CENTER_CENTER,
                "没有找到应用",
                text(14.0),
                p.muted,
            );
            return;
        }

        // How tall the whole thing is, before the window clips it.
        let content_h = match self.state.layout {
            View::Grid => {
                let rows = apps.len().div_ceil(COLUMNS);
                if rows == 0 {
                    0.0
                } else {
                    313.0 + 289.0 * (rows - 1) as f32
                }
            }
            View::List => 44.0 + apps.len() as f32 * 70.0,
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
    fn apply_wheel(&self, ui: &Ui, band: Rect, content_h: f32, scroll: f32) -> f32 {
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
    fn scrollbar(
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
        let col_w = (band.width() - MARGIN - RIGHT_GUTTER) / COLUMNS as f32;
        let scroll = self.state.scroll;

        let mut row_top = band.min.y - scroll;
        for (i, &idx) in apps.iter().enumerate() {
            let col = i % COLUMNS;
            let row = i / COLUMNS;
            // The picture's rows: the first is 313 pt tall and the rest
            // 289 pt.
            let row_h = if row == 0 { 313.0 } else { 289.0 };
            let x = MARGIN + col as f32 * col_w;
            // The picture's grid stops just short of the scrollbar, and
            // the last column has no rule down its right-hand side.
            let w = if col == COLUMNS - 1 {
                col_w - 2.5
            } else {
                col_w
            };
            let card = Rect::from_min_size(pos2(x, row_top), vec2(w, row_h));
            self.card(ui, card, idx, band, p);
            if col == COLUMNS - 1 || i == apps.len() - 1 {
                row_top += row_h;
            }
        }
    }

    /// One app card: a header line, the app itself, then the action button.
    /// Anything outside the visible strip is neither drawn nor clickable,
    /// so a card scrolled away cannot be pressed through the footer.
    fn card(&mut self, ui: &mut Ui, card: Rect, idx: usize, band: Rect, p: &Palette) {
        let vis = card.intersect(band);
        if !vis.is_positive() {
            return;
        }
        // Everything inside a card is clipped to the card's own column —
        // and to the visible strip — so a long line never runs into the
        // next card, nor off the top of the window.
        let clip = ui.painter().with_clip_rect(vis);
        let painter = &clip;

        let app = &repos()[idx];
        // The whole card is one click target: anywhere inside it opens the
        // app's page. The small links and buttons inside are registered
        // after it, so they still win the click where they sit. While the
        // pointer is over the card its frame takes the accent colour.
        let mut hovered = false;
        if vis.is_positive() {
            let resp = ui.interact(
                vis,
                ui.scope_id().with(("card", app.id)),
                Sense::click(),
            );
            hovered = resp.hovered() || self.hover_demo == Some(idx);
            if hovered {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.clicked() {
                self.open_page(idx);
            }
        }
        // Everything read off the state is copied out up front, so the
        // borrows are done before anything calls back into it.
        let installed = self.state.is_installed(idx);
        let has_update = self.state.has_update(idx);
        let (phase, progress) = self
            .state
            .repos[idx]
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
            // Nothing came back: say what is on this Mac rather than
            // leaving the card blank.
            FetchState::Done(Err(_)) => match &self.state.repos[idx].installed {
                Some(v) => (format!("V{}", v.to_uppercase()), "未获取到".to_string()),
                None => ("—".to_string(), "未获取到".to_string()),
            },
        };

        // The card's frame. On hover it is tinted and outlined in the
        // accent colour, so the pointer's target is obvious.
        if hovered {
            painter.rect_filled(card, 0, p.surface);
            painter.rect_stroke(
                card,
                0,
                Stroke::new(1.5, p.accent),
                StrokeKind::Middle,
            );
        } else {
            painter.rect_stroke(card, 0, line(p.border), StrokeKind::Middle);
        }

        // The header strip: the version on the left, a link on the right.
        let header = Rect::from_min_max(card.min, pos2(card.max.x, card.min.y + CARD_HEAD_H));
        painter.hline(header.min.x..=header.max.x, header.max.y, line(p.border));
        let version_color = if has_update {
            p.amber
        } else {
            Color32::from_rgb(app.bright.0, app.bright.1, app.bright.2)
        };
        painter.text(
            pos2(header.min.x + 20.0, header.center().y + 1.5),
            Align2::LEFT_CENTER,
            &version_text,
            label(12.0),
            version_color,
        );
        // The little link, top right. The picture puts it at 297..360 pt.
        let link_x = header.max.x - 82.0;
        let link_hit = Rect::from_min_max(
            pos2(link_x - 6.0, header.min.y),
            pos2(header.max.x - 14.0, header.max.y),
        )
        .intersect(band);
        let mut link_hover = false;
        if link_hit.is_positive() {
            let resp = ui.interact(
                link_hit,
                ui.scope_id().with(("link", app.id)),
                Sense::click(),
            );
            link_hover = resp.hovered();
            if resp.clicked() {
                self.open_repo_page(idx);
            }
        }
        painter.text(
            pos2(link_x, header.center().y),
            Align2::LEFT_CENTER,
            "GITHUB",
            label(12.0),
            if link_hover { p.accent } else { p.muted },
        );
        arrow_out(
            painter,
            Rect::from_center_size(pos2(link_x + 72.0, header.center().y), vec2(12.0, 12.0)),
            p.muted,
        );

        // The app's tile, then the name and the badge. The picture puts the
        // tile at 40..96 pt across and 182..238 pt down. Clicking the tile
        // opens the app's own page.
        let tile = Rect::from_min_size(
            pos2(card.min.x + 20.0, header.max.y + 19.0),
            vec2(TILE, TILE),
        );
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

        let text_x = tile.max.x + 15.0;
        let name_y = tile.min.y + 19.0;
        // The name, plain white as asked for.
        let name_w = measure(ui, app.name, display(22.0));
        painter.text(
            pos2(text_x, name_y),
            Align2::LEFT_CENTER,
            app.name,
            display(22.0),
            p.text,
        );

        // The badge that says who publishes it. A long owner name would
        // run past the card's rule, so it is shortened, then dropped, when
        // there is no room beside the app's name.
        let owner = catalog::owner_of(app.slug).to_uppercase();
        let badge_x = text_x + name_w + 14.0;
        let room = card.max.x - 16.0 - badge_x;
        for (word, size) in [
            (format!("由 {owner} 提供"), 11.0),
            (format!("由 {owner}"), 10.0),
        ] {
            let badge_w = measure(ui, &word, label(size)) + 22.0;
            if badge_w <= room {
                let badge = Rect::from_min_size(
                    pos2(badge_x, name_y - 11.0),
                    vec2(badge_w, 22.0),
                );
                painter.rect_stroke(badge, 0, line(p.border), StrokeKind::Middle);
                painter.text(
                    pos2(badge.center().x, badge.center().y),
                    Align2::CENTER_CENTER,
                    &word,
                    label(size),
                    p.accent,
                );
                break;
            }
        }

        // The Chinese one-liner, under the name.
        painter.text(
            pos2(text_x, name_y + 23.0),
            Align2::LEFT_CENTER,
            app.tagline,
            text(14.0),
            p.muted,
        );

        // The English description, on the left below the tile.
        let desc_y = header.max.y + TILE + 46.0;
        // Text starts twenty points in and stops twenty short, so no line
        // ever runs into the card's rule.
        let wrap_w = card.width() - 40.0;
        let summary = self.summary_text(idx);
        let galley = painter.layout(summary, text(14.0), p.muted, wrap_w);
        let galley_h = galley.size().y;
        painter.galley(pos2(card.min.x + 20.0, desc_y - 7.0), galley, p.muted);

        // The date of the newest release, below the description.
        painter.text(
            pos2(card.min.x + 20.0, desc_y + galley_h - 7.0 + 18.0),
            Align2::LEFT_CENTER,
            format!("最后更新: {date_text}"),
            text(13.0),
            p.faint,
        );

        // The action button, and the small menu beside it. The picture
        // puts them at 40..363 pt and 383..363 pt across. The row sits a
        // fixed distance above the card's own bottom edge, so it can
        // never spill onto the next card's frame. An app with an update
        // waiting gets a second, narrow 打开 button at the left, so the
        // copy on this Mac can still be started while the new one
        // downloads.
        let btn_h = 33.0;
        let btn_y = card.max.y - 53.0;
        let more_w = 37.0;
        let right_edge = card.max.x - 18.0;
        let left_edge = card.min.x + 20.0;
        let more_rect = Rect::from_min_max(
            pos2(right_edge - more_w, btn_y),
            pos2(right_edge, btn_y + btn_h),
        );
        let main_rect = Rect::from_min_max(
            pos2(left_edge, btn_y),
            pos2(more_rect.min.x - 8.0, btn_y + btn_h),
        );

        // An app with an update waiting gets 打开 and 更新 side by side,
        // each exactly half the row.
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
                15.0,
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
                15.0,
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
                15.0,
                band,
                p,
            )
        };
        if pressed {
            self.act(idx);
        }
        if busy && phase == JobPhase::Downloading && progress > 0.0 {
            // Fill the button in step with the real download.
            let filled = Rect::from_min_max(
                open_rect.min,
                pos2(
                    open_rect.min.x + open_rect.width() * progress,
                    open_rect.max.y,
                ),
            );
            painter.rect_filled(filled, 0, p.surface);
            painter.rect_stroke(open_rect, 0, line(p.border), StrokeKind::Middle);
            painter.text(
                pos2(open_rect.center().x, open_rect.center().y),
                Align2::CENTER_CENTER,
                label_text,
                text(15.0),
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

        // The little menu that opens beside the button is drawn last, on
        // top of the cards.
        if self.menu_for == Some(idx) {
            self.pending_menu = Some((more_rect, idx));
        }
    }

    /// The small menu beside a card's button, drawn on top of the cards.
    fn draw_menu(&mut self, ui: &mut Ui, anchor: Rect, idx: usize, body: Rect, p: &Palette) {
        let painter = ui.painter();
        let app = &repos()[idx];
        let installed = self.state.is_installed(idx);
        let entries: Vec<&str> = if installed {
            vec!["打开", "重新安装", "复制链接"]
        } else {
            vec!["安装", "复制链接"]
        };
        let w = 140.0;
        let h = entries.len() as f32 * 28.0;
        // The menu opens below the button when there is room, and above it
        // when there is not.
        let below = anchor.max.y + 4.0 + h;
        let top = if below > body.max.y {
            anchor.min.y - 4.0 - h
        } else {
            anchor.max.y + 4.0
        };
        // It opens to the left of the button when there is no room on the
        // right, as the last column would run off the window.
        let left = if anchor.min.x + w > body.max.x - MARGIN {
            anchor.max.x - w
        } else {
            anchor.min.x
        };
        let rect = Rect::from_min_max(pos2(left, top), pos2(left + w, top + h));
        painter.rect_filled(rect, 0, p.surface);
        painter.rect_stroke(rect, 0, line(p.border), StrokeKind::Middle);

        for (i, entry) in entries.iter().enumerate() {
            let row = Rect::from_min_max(
                pos2(rect.min.x, rect.min.y + i as f32 * 28.0),
                pos2(rect.max.x, rect.min.y + (i as f32 + 1.0) * 28.0),
            );
            let resp = ui.interact(
                row,
                ui.scope_id().with(("menu", app.id, i)),
                Sense::click(),
            );
            if resp.clicked() {
                match *entry {
                    "打开" => self.open_app(idx),
                    "安装" | "重新安装" => self.start_job(idx),
                    "复制链接" => self.copy_link(idx),
                    _ => {}
                }
                self.menu_for = None;
            }
            if resp.hovered() {
                painter.rect_filled(row, 0, p.border);
            }
            painter.text(
                pos2(row.min.x + 10.0, row.center().y),
                Align2::LEFT_CENTER,
                *entry,
                text(12.5),
                p.text,
            );
        }
    }

    /// The sort dropdown, drawn on top of the cards.
    fn draw_sort_dropdown(&mut self, ui: &mut Ui, sort_rect: Rect, p: &Palette) {
        let painter = ui.painter();
        let mut chosen = self.state.sort;
        let list_rect = Rect::from_min_max(
            pos2(sort_rect.min.x, sort_rect.max.y),
            pos2(sort_rect.max.x, sort_rect.max.y + 56.0),
        );
        painter.rect_filled(list_rect, 0, p.surface);
        painter.rect_stroke(list_rect, 0, line(p.border), StrokeKind::Middle);
        for (i, option) in [SortBy::Name, SortBy::Updated].iter().enumerate() {
            let row = Rect::from_min_max(
                pos2(list_rect.min.x, list_rect.min.y + i as f32 * 28.0),
                pos2(list_rect.max.x, list_rect.min.y + (i as f32 + 1.0) * 28.0),
            );
            let resp = ui.interact(row, ui.scope_id().with(("sort", i)), Sense::click());
            if resp.clicked() {
                chosen = *option;
                self.sort_open = false;
            }
            if resp.hovered() {
                painter.rect_filled(row, 0, p.border);
            }
            painter.text(
                pos2(row.min.x + 12.0, row.center().y),
                Align2::LEFT_CENTER,
                sort_option_label(*option),
                text(13.0),
                p.text,
            );
        }
        self.state.sort = chosen;
    }

    /// The settings page: appearance on the left, updates and folders on
    /// the right, with the exit and done buttons along the bottom.
    fn settings_page(&mut self, ui: &mut Ui, band: Rect, p: &Palette) {
        let painter = ui.painter().with_clip_rect(band);
        let w = band.width() - 2.0 * DETAIL_MARGIN;
        let left = band.min.x + DETAIL_MARGIN;
        let col_w = (w - 40.0) / 2.0;
        let right_x = left + col_w + 40.0;

        // The page's own heading.
        draw_g_mark(
            &painter,
            Rect::from_center_size(pos2(left + 11.0, band.min.y + 33.0), vec2(22.0, 20.0)),
            p.text,
        );
        painter.text(
            pos2(left + 32.0, band.min.y + 34.0),
            Align2::LEFT_CENTER,
            "设置",
            display(26.0),
            p.text,
        );
        painter.text(
            pos2(left + 100.0, band.min.y + 36.0),
            Align2::LEFT_CENTER,
            format!("GITUP LAUNCHER V{}", env!("CARGO_PKG_VERSION").to_uppercase()),
            label(11.0),
            p.muted,
        );
        painter.hline(
            band.min.x + DETAIL_MARGIN..=band.max.x - DETAIL_MARGIN,
            band.min.y + 66.0,
            line(p.border),
        );

        let mut y = band.min.y + 96.0;
        // ------------------------------------------------ left: appearance
        painter.text(
            pos2(left, y),
            Align2::LEFT_CENTER,
            "外观",
            text(15.0),
            p.text,
        );
        y += 34.0;
        // The segmented control.
        let seg_w = col_w;
        let cell_w = seg_w / 3.0;
        let seg = Rect::from_min_size(pos2(left, y), vec2(seg_w, 32.0));
        for (i, (word, mode)) in [
            ("跟随系统", state::ThemeMode::System),
            ("深色", state::ThemeMode::Dark),
            ("浅色", state::ThemeMode::Light),
        ]
        .iter()
        .enumerate()
        {
            let cell = Rect::from_min_max(
                pos2(left + i as f32 * cell_w, seg.min.y),
                pos2(left + (i as f32 + 1.0) * cell_w, seg.max.y),
            );
            let chosen = self.settings.theme == *mode;
            if chosen {
                painter.rect_filled(cell, 0, p.text);
            } else {
                painter.rect_stroke(cell, 0, line(p.border), StrokeKind::Middle);
            }
            painter.text(
                pos2(cell.center().x, cell.center().y),
                Align2::CENTER_CENTER,
                *word,
                text(13.0),
                if chosen { p.bg } else { p.text },
            );
            let hit = cell.intersect(band);
            if hit.is_positive() {
                let resp = ui.interact(
                    hit,
                    ui.scope_id().with(("settings-theme", i)),
                    Sense::click(),
                );
                if resp.clicked() && self.settings.theme != *mode {
                    self.settings.theme = *mode;
                    self.ctx.set_theme(theme_preference(*mode));
                    state::save_settings(&self.settings);
                }
            }
        }
        y += 54.0;

        // ------------------------------------------------- left: updates
        painter.text(
            pos2(left, y),
            Align2::LEFT_CENTER,
            "更新",
            text(15.0),
            p.text,
        );
        y += 34.0;
        for (key, word, hint) in [
            ("auto_check", "启动时自动检查更新", "打开启动器时自动向 GitHub 查询各仓库的最新发布。"),
            ("include_pre", "包含预发布版本", "把测试版和候选版也当作可安装的更新。"),
        ] {
            let checked = if key == "auto_check" {
                self.settings.auto_check
            } else {
                self.settings.include_pre
            };
            let box_r = Rect::from_min_size(pos2(left, y - 9.0), vec2(18.0, 18.0));
            painter.rect_stroke(box_r, radius(4), line(p.border), StrokeKind::Middle);
            if checked {
                painter.rect_filled(box_r, radius(4), p.text);
                let c = box_r.center();
                let s = Stroke::new(2.0, p.bg);
                painter.line_segment(
                    [egui::pos2(c.x - 4.0, c.y + 0.5), egui::pos2(c.x - 1.5, c.y + 3.5)],
                    s,
                );
                painter.line_segment(
                    [egui::pos2(c.x - 1.5, c.y + 3.5), egui::pos2(c.x + 4.5, c.y - 3.0)],
                    s,
                );
            }
            painter.text(
                pos2(box_r.max.x + 10.0, box_r.center().y),
                Align2::LEFT_CENTER,
                word,
                text(14.0),
                p.text,
            );
            painter.text(
                pos2(box_r.max.x + 10.0, box_r.center().y + 18.0),
                Align2::LEFT_CENTER,
                hint,
                text(11.5),
                p.faint,
            );
            let hit = Rect::from_min_size(
                pos2(left, y - 14.0),
                vec2(measure(ui, word, text(14.0)) + 34.0, 30.0),
            )
            .intersect(band);
            if hit.is_positive() {
                let resp = ui.interact(
                    hit,
                    ui.scope_id().with(("settings-check", key)),
                    Sense::click(),
                );
                if resp.clicked() {
                    if key == "auto_check" {
                        self.settings.auto_check = !self.settings.auto_check;
                    } else {
                        self.settings.include_pre = !self.settings.include_pre;
                    }
                    state::save_settings(&self.settings);
                }
            }
            y += 52.0;
        }
        // The check-now button.
        let check = Rect::from_min_size(pos2(left, y - 6.0), vec2(140.0, 32.0));
        painter.rect_stroke(check, 0, line(p.border), StrokeKind::Middle);
        painter.text(
            pos2(check.center().x, check.center().y),
            Align2::CENTER_CENTER,
            "立即检查更新",
            text(13.0),
            p.text,
        );
        let hit = check.intersect(band);
        if hit.is_positive() {
            let resp = ui.interact(hit, ui.scope_id().with("settings-check-now"), Sense::click());
            if resp.clicked() {
                self.spawn_meta_fetch_all();
            }
        }

        // --------------------------------------------- right: install folder
        let mut ry = band.min.y + 96.0;
        painter.text(
            pos2(right_x, ry),
            Align2::LEFT_CENTER,
            "默认安装文件夹",
            text(15.0),
            p.text,
        );
        ry += 34.0;
        let folder = Rect::from_min_size(pos2(right_x, ry), vec2(col_w, 36.0));
        painter.rect_stroke(folder, 0, line(p.border), StrokeKind::Middle);
        painter.text(
            pos2(folder.min.x + 14.0, folder.center().y),
            Align2::LEFT_CENTER,
            &self.settings.install_dir,
            text(14.0),
            p.text,
        );
        ry += 52.0;
        painter.text(
            pos2(right_x, ry),
            Align2::LEFT_CENTER,
            "应用会放置在此文件夹内。新安装的应用使用此文件夹，更新时保留当前位置。",
            text(11.5),
            p.faint,
        );
        ry += 30.0;
        painter.text(
            pos2(right_x, ry),
            Align2::LEFT_CENTER,
            "安装到「应用程序」需要一次管理员授权；也可以改为用户目录下的文件夹。",
            text(11.5),
            p.faint,
        );
        // A rule and some air between the two sections, so the folder's
        // fine print never runs into the subscriptions heading.
        ry += 26.0;
        painter.hline(right_x..=right_x + col_w, ry, line(p.border));
        ry += 32.0;

        // ------------------------------------- right: managed subscriptions
        painter.text(
            pos2(right_x, ry),
            Align2::LEFT_CENTER,
            "订阅仓库",
            text(15.0),
            p.text,
        );
        ry += 30.0;
        painter.text(
            pos2(right_x, ry),
            Align2::LEFT_CENTER,
            "列出的仓库会被检查发布并显示在主界面；点 × 从订阅中移除。",
            text(11.5),
            p.faint,
        );
        ry += 28.0;
        let list = repos();
        let sub_cols = 2;
        let sub_cell_w = (col_w - 12.0) / sub_cols as f32;
        let mut slot = 0usize;
        for (i, repo) in list.iter().enumerate() {
            if i >= self.state.subscribed.len() || !self.state.subscribed[i] {
                continue;
            }
            let col = slot % sub_cols;
            let row = slot / sub_cols;
            slot += 1;
            let x = right_x + col as f32 * (sub_cell_w + 12.0);
            let yy = ry + row as f32 * 30.0;
            painter.text(
                pos2(x, yy),
                Align2::LEFT_CENTER,
                repo.name,
                text(13.0),
                p.text,
            );
            // The cross sits right after the name, and removes the row.
            let name_w = measure(ui, repo.name, text(13.0));
            let cx_x = (x + name_w + 10.0).min(x + sub_cell_w - 24.0);
            let cx = Rect::from_min_size(pos2(cx_x, yy - 10.0), vec2(20.0, 20.0));
            let over = cx.contains(ui.input(|i| i.pointer.hover_pos().unwrap_or_default()));
            painter.rect_stroke(
                cx,
                radius(4),
                if over {
                    Stroke::new(1.0, p.amber)
                } else {
                    line(p.border)
                },
                StrokeKind::Middle,
            );
            let c = cx.center();
            let st = Stroke::new(1.5, if over { p.amber } else { p.text });
            painter.line_segment(
                [pos2(c.x - 4.0, c.y - 4.0), pos2(c.x + 4.0, c.y + 4.0)],
                st,
            );
            painter.line_segment(
                [pos2(c.x + 4.0, c.y - 4.0), pos2(c.x - 4.0, c.y + 4.0)],
                st,
            );
            let hit = cx.intersect(band).expand(4.0);
            if hit.is_positive() {
                let resp = ui.interact(
                    hit,
                    egui::Id::unique(("settings-remove", repo.slug)),
                    Sense::click(),
                );
                if resp.clicked() {
                    if catalog::is_builtin(repo.slug) {
                        self.toggle_subscription(i);
                    } else {
                        self.remove_repository(i);
                    }
                }
            }
        }
        let sub_rows = slot.div_ceil(sub_cols) as f32;
        ry += sub_rows * 30.0 + 4.0;
        // A way back for built-in repositories that were removed.
        let removed_builtin = list.iter().enumerate().any(|(i, r)| {
            catalog::is_builtin(r.slug)
                && i < self.state.subscribed.len()
                && !self.state.subscribed[i]
        });
        if removed_builtin {
            let caption = "恢复默认仓库";
            let r = Rect::from_min_size(
                pos2(right_x, ry),
                vec2(measure(ui, caption, text(12.0)) + 8.0, 20.0),
            );
            let over = r.contains(ui.input(|i| i.pointer.hover_pos().unwrap_or_default()));
            painter.text(
                pos2(r.min.x, r.center().y),
                Align2::LEFT_CENTER,
                caption,
                text(12.0),
                if over { p.accent } else { p.muted },
            );
            let resp = ui.interact(
                r.intersect(band),
                egui::Id::unique("settings-restore"),
                Sense::click(),
            );
            if resp.clicked() {
                self.settings.disabled.clear();
                for i in 0..self.state.subscribed.len() {
                    self.state.subscribed[i] = true;
                }
                state::save_settings(&self.settings);
                self.spawn_meta_fetch_all();
                self.state.status = "已恢复全部内置仓库".to_string();
            }
            ry += 28.0;
        }
        ry += 12.0;

        // The add box: a link or a slug goes in, the same rules take over
        // from there. The field has a fixed id so it keeps its focus (and
        // therefore keeps working) across frames, a click anywhere on the
        // box puts the caret in it, and a 粘贴 button reads the clipboard
        // itself so adding never depends on a keyboard shortcut working.
        let add_w = col_w - 178.0;
        let add_box = Rect::from_min_size(pos2(right_x, ry), vec2(add_w, 32.0));
        painter.rect_filled(add_box, 0, p.search_fill);
        let field_id = egui::Id::unique("settings-add-field");
        let field_focused = ui.memory(|m| m.has_focus(field_id));
        painter.rect_stroke(
            add_box,
            0,
            if field_focused {
                Stroke::new(1.0, p.accent)
            } else {
                line(p.border)
            },
            StrokeKind::Middle,
        );
        if self.settings_input.is_empty() {
            painter.text(
                pos2(add_box.min.x + 10.0, add_box.center().y),
                Align2::LEFT_CENTER,
                "粘贴仓库链接，或填写 owner/repo",
                text(12.0),
                p.faint,
            );
        }
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(Rect::from_min_max(
                    pos2(add_box.min.x + 8.0, add_box.min.y + 1.0),
                    pos2(add_box.max.x - 8.0, add_box.max.y - 1.0),
                ))
                .id_salt("settings-add-field-ui"),
        );
        let field_resp = child.add_sized(
            vec2(add_box.width() - 16.0, 28.0),
            TextEdit::singleline(&mut self.settings_input)
                .id(field_id)
                .text_color(p.text)
                .frame(egui::Frame::NONE)
                .desired_width(add_box.width() - 16.0),
        );
        if ui.rect_contains_pointer(add_box)
            && ui.input(|i| i.pointer.primary_pressed())
        {
            ui.memory_mut(|m| m.request_focus(field_id));
        }
        // Command-V, handled here when the window manager has not already
        // turned it into a paste: the field is the one on screen, and the
        // clipboard is read directly.
        if field_focused
            && ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::V))
        {
            let already = ui.input(|i| {
                i.events
                    .iter()
                    .any(|e| matches!(e, egui::Event::Paste(_)))
            });
            if !already {
                if let Some(text) = clipboard_text() {
                    self.settings_input.push_str(&text);
                }
            }
        }

        let paste_btn = Rect::from_min_size(
            pos2(add_box.max.x + 8.0, ry),
            vec2(96.0, 32.0),
        );
        let add_btn = Rect::from_min_size(
            pos2(paste_btn.max.x + 8.0, ry),
            vec2(64.0, 32.0),
        );
        let mut submit = field_resp.lost_focus()
            && ui.input(|i| i.key_pressed(egui::Key::Enter));
        for (rect, caption, id, paste) in [
            (paste_btn, "粘贴并添加", "settings-paste", true),
            (add_btn, "添加", "settings-add", false),
        ] {
            let hit = rect.intersect(band);
            let mut hovered = false;
            let mut clicked = false;
            if hit.is_positive() {
                let resp = ui.interact(hit, egui::Id::unique(id), Sense::click());
                hovered = resp.hovered();
                clicked = resp.clicked();
            }
            if hovered {
                painter.rect_filled(rect, 0, p.surface);
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            painter.rect_stroke(
                rect,
                0,
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
                text(13.0),
                if hovered { p.accent } else { p.text },
            );
            if clicked {
                if paste {
                    match clipboard_text() {
                        Some(text) => {
                            self.settings_input = text;
                            submit = true;
                        }
                        None => {
                            self.settings_error =
                                Some("剪贴板里没有文字，先复制仓库链接".to_string());
                        }
                    }
                } else {
                    submit = true;
                }
            }
        }
        if submit {
            let typed = self.settings_input.clone();
            self.add_repository(&typed);
        }
        // Whatever went wrong with the last attempt, said plainly under the
        // box, so a rejected address never looks like a dead button.
        if let Some(err) = &self.settings_error {
            painter.text(
                pos2(right_x, ry + 46.0),
                Align2::LEFT_CENTER,
                err,
                text(12.0),
                p.amber,
            );
        }
        ry += 44.0;        ry += 44.0;
        painter.text(
            pos2(right_x, ry),
            Align2::LEFT_CENTER,
            "支持完整的 GitHub 链接；添加后自动拉取发布、查找本机版本并加载图标。",
            text(11.5),
            p.faint,
        );

        // --------------------------------------------- the bottom buttons
        let by = band.max.y - 44.0;
        let quit = Rect::from_min_size(pos2(left, by), vec2(210.0, 36.0));
        painter.rect_stroke(quit, 0, line(p.border), StrokeKind::Middle);
        painter.text(
            pos2(quit.center().x, quit.center().y),
            Align2::CENTER_CENTER,
            format!("退出 GITUP LAUNCHER"),
            text(13.0),
            p.muted,
        );
        let hit = quit.intersect(band);
        if hit.is_positive() {
            let resp = ui.interact(hit, ui.scope_id().with("settings-quit"), Sense::click());
            if resp.clicked() {
                std::process::exit(0);
            }
        }
        let done = Rect::from_min_size(pos2(band.max.x - DETAIL_MARGIN - 120.0, by), vec2(120.0, 36.0));
        painter.rect_filled(done, radius(4), p.text);
        painter.text(
            pos2(done.center().x, done.center().y),
            Align2::CENTER_CENTER,
            "完成",
            text(14.0),
            p.bg,
        );
        let hit = done.intersect(band);
        if hit.is_positive() {
            let resp = ui.interact(hit, ui.scope_id().with("settings-done"), Sense::click());
            if resp.clicked() {
                self.page = Page::Grid;
            }
        }
    }


    /// The list view: a table with a header row, one row per app, riding
    /// the same scroll offset. The columns run 应用, 状态, 版本, 发布日期,
    /// 作者, then the action button and the little menu.
    fn list(&mut self, ui: &mut Ui, band: Rect, apps: &[usize], p: &Palette) {
        let painter = ui.painter().with_clip_rect(band);
        // Roomy rows: the table was too tight to read at a glance.
        let row_h = 62.0;
        let head_h = 44.0;
        let right = band.max.x - RIGHT_GUTTER;
        // The right-hand column edges, working leftward from the window's
        // right edge: the little menu, the action button, then the author,
        // the date, the version and the status pill.
        let more_x = right - 37.0;
        let act_x = more_x - 12.0 - 92.0;
        let author_r = act_x - 30.0;
        let date_r = author_r - 104.0;
        let ver_r = date_r - 124.0;
        let state_r = ver_r - 90.0;

        // The header row, pinned above the rows.
        if band.height() > 40.0 {
            painter.hline(band.min.x + MARGIN..=right, band.min.y + head_h, line(p.border));
            let hy = band.min.y + head_h * 0.5;
            painter.text(
                pos2(band.min.x + MARGIN + 2.0, hy),
                Align2::LEFT_CENTER,
                "应用",
                text(12.0),
                p.faint,
            );
            for (word, xr) in [
                ("状态", state_r),
                ("版本", ver_r),
                ("发布日期", date_r),
                ("作者", author_r),
            ] {
                painter.text(
                    pos2(xr, hy),
                    Align2::RIGHT_CENTER,
                    word,
                    text(12.0),
                    p.faint,
                );
            }
        }

        let scroll = self.state.scroll;
        for (i, &idx) in apps.iter().enumerate() {
            let app = &repos()[idx];
            let y = band.min.y + head_h + i as f32 * (row_h + 8.0) - scroll;
            if y + row_h < band.min.y + head_h || y > band.max.y {
                continue;
            }
            let row = Rect::from_min_size(
                pos2(band.min.x + MARGIN, y),
                vec2(band.width() - MARGIN - RIGHT_GUTTER, row_h),
            );
            let vis = row.intersect(band);
            let clip = ui.painter().with_clip_rect(vis);
            clip.rect_stroke(row, 0, line(p.border), StrokeKind::Middle);

            let tile = Rect::from_min_size(
                pos2(row.min.x + 10.0, row.center().y - 18.0),
                vec2(36.0, 36.0),
            );
            // Clicking the tile opens the app's own page.
            let tile_hit = tile.intersect(band);
            if tile_hit.is_positive() {
                let resp = ui.interact(
                    tile_hit,
                    ui.scope_id().with(("rowtile", app.id)),
                    Sense::click(),
                );
                if resp.clicked() {
                    self.open_page(idx);
                }
            }
            self.draw_tile(&clip, tile, idx, 9);

            clip.text(
                pos2(tile.max.x + 14.0, row.center().y - 9.0),
                Align2::LEFT_CENTER,
                app.name,
                display(15.0),
                p.text,
            );
            clip.text(
                pos2(tile.max.x + 14.0, row.center().y + 10.0),
                Align2::LEFT_CENTER,
                app.tagline,
                text(11.5),
                p.muted,
            );

            // The status pill. The window never shows a lie: an app with
            // an update waiting reads 可更新 even though it is installed.
            let installed = self.state.is_installed(idx);
            let has_update = self.state.has_update(idx);
            let (word, color) = if has_update {
                ("可更新", p.amber)
            } else if installed {
                ("已安装", Color32::from_rgb(0x4A, 0xD9, 0x8B))
            } else {
                ("可安装", p.muted)
            };
            let pw = measure(ui, word, label(11.0)) + 22.0;
            let pill = Rect::from_min_max(
                pos2(state_r - pw, row.center().y - 11.0),
                pos2(state_r, row.center().y + 11.0),
            );
            clip.rect_stroke(pill, radius(11), Stroke::new(1.0, color), StrokeKind::Middle);
            clip.text(
                pos2(pill.center().x, pill.center().y),
                Align2::CENTER_CENTER,
                word,
                label(11.0),
                color,
            );

            // Version and date, from the live release when it has arrived.
            let (version, date) = match &self.state.repos[idx].fetch {
                FetchState::Pending => ("…".to_string(), "…".to_string()),
                FetchState::Done(Ok(rel)) => (
                    format!("V{}", version::clean_tag(&rel.tag).to_uppercase()),
                    version::fmt_date(&rel.published),
                ),
                FetchState::Done(Err(_)) => ("—".to_string(), "—".to_string()),
            };
            clip.text(
                pos2(ver_r, row.center().y),
                Align2::RIGHT_CENTER,
                version,
                text(13.0),
                p.text,
            );
            clip.text(
                pos2(date_r, row.center().y),
                Align2::RIGHT_CENTER,
                date,
                text(13.0),
                p.muted,
            );
            let author = catalog::owner_of(app.slug).to_uppercase();
            clip.text(
                pos2(author_r, row.center().y),
                Align2::RIGHT_CENTER,
                fit_text(ui, &author, text(13.0), 86.0),
                text(13.0),
                p.muted,
            );

            // The action button, with its fill for running downloads.
            let busy = self.state.busy(idx);
            let (phase, progress) = self
                .state
                .repos[idx]
                .job
                .as_ref()
                .map(|j| (j.phase, j.progress))
                .unwrap_or((JobPhase::Downloading, 0.0));
            let label_text = self.button_label(idx).to_string();
            let btn = Rect::from_min_size(pos2(act_x, row.center().y - 13.0), vec2(86.0, 26.0));
            let update_waiting = self.state.has_update(idx) && !busy;
            let pressed = if update_waiting {
                primary_button(
                    ui,
                    &clip,
                    btn,
                    egui::Id::unique(("row", app.id)),
                    &label_text,
                    12.5,
                    band,
                    p,
                )
            } else {
                action_button(
                    ui,
                    &clip,
                    btn,
                    egui::Id::unique(("row", app.id)),
                    &label_text,
                    12.5,
                    band,
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
                clip.rect_filled(filled, 0, p.surface);
                clip.rect_stroke(btn, 0, line(p.border), StrokeKind::Middle);
                clip.text(
                    pos2(btn.center().x, btn.center().y),
                    Align2::CENTER_CENTER,
                    &label_text,
                    text(12.5),
                    p.text,
                );
            }

            // The little menu at the far right.
            let more = Rect::from_min_size(pos2(more_x, row.center().y - 13.0), vec2(37.0, 26.0));
            if action_button(
                ui,
                &clip,
                more,
                egui::Id::unique(("rowmore", app.id)),
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
                self.pending_menu = Some((more, idx));
            }
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
fn action_button(
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
        painter.rect_filled(rect, 0, p.surface);
    }
    painter.rect_stroke(
        rect,
        0,
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
fn primary_button(
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
        0,
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

/// A string shortened with an ellipsis until it fits the given width.
fn fit_text(ui: &Ui, text_in: &str, font: egui::FontId, max_w: f32) -> String {
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
fn theme_preference(mode: state::ThemeMode) -> egui::ThemePreference {
    match mode {
        state::ThemeMode::System => egui::ThemePreference::System,
        state::ThemeMode::Dark => egui::ThemePreference::Dark,
        state::ThemeMode::Light => egui::ThemePreference::Light,
    }
}

/// The caption for one of the ways the list can be sorted.
fn sort_option_label(sort: SortBy) -> &'static str {
    match sort {
        SortBy::Name => "名称 (A-Z)",
        SortBy::Updated => "最后更新",
    }
}

/// A one-pixel line in the given colour.
fn line(color: Color32) -> Stroke {
    Stroke::new(1.0, color)
}

/// The width of a piece of text, measured with the font it will be drawn in.
fn measure(ui: &Ui, text: &str, font: egui::FontId) -> f32 {
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
                    let image = render_icon(1024);
                    write_png(std::path::Path::new(&path), &image);
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
                        inner: App { launcher },
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
                None => Ok(Box::new(App { launcher })),
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
            if self.pass == 20 {
                raw_input
                    .events
                    .push(egui::Event::PointerMoved(egui::pos2(800.0, 500.0)));
            }
            click(21, egui::pos2(800.0, 500.0), raw_input);
            if self.pass == 26 {
                raw_input.events.push(egui::Event::Text(
                    "https://github.com/diamondfsd/luna-ai-cut".to_string(),
                ));
            }
            click(40, egui::pos2(1070.0, 500.0), raw_input);
        }
    }

    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        // A test hook: with GITUP_TYPE_TEST set, the add-repository field is
        // focused and fed some text on one frame, so a picture can show
        // whether it really takes keystrokes.
        if std::env::var("GITUP_TYPE_TEST").is_ok() {
            self.frame += 1;
            if self.frame == 20 {
                let id = egui::Id::unique("settings-add-field");
                ui.ctx().memory_mut(|m| m.request_focus(id));
                ui.ctx().input_mut(|i| {
                    i.events.push(egui::Event::Text(
                        "diamondfsd/luna-ai-cut".to_string(),
                    ));
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
                    write_png(&path, &image);
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

/// Draws the launcher's icon: a green rounded plate with the head of a
/// bird in profile on it — white, black-outlined, hatched like an
/// engraving, with a black eye and a white glint. Every edge is smoothed
/// with four-by-four samples per pixel. Pixels are straight RGBA, which is
/// what the PNG writer expects.
fn render_icon(size: usize) -> egui::ColorImage {
    let s = size as f32;
    let inside_poly = |x: f32, y: f32, poly: &[(f32, f32)]| -> bool {
        let mut inside = false;
        let mut j = poly.len() - 1;
        for i in 0..poly.len() {
            let (xi, yi) = poly[i];
            let (xj, yj) = poly[j];
            if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
                inside = !inside;
            }
            j = i;
        }
        inside
    };
    // The head and neck, facing right: crown, forehead, beak, throat, the
    // notch under the beak, then straight down the front and the back.
    let head: [(f32, f32); 13] = [
        (0.130, 0.905), // neck, bottom left
        (0.118, 0.330), // back of the head
        (0.190, 0.140), // crown, rear
        (0.320, 0.058), // crown
        (0.480, 0.072), // brow
        (0.600, 0.162), // forehead
        (0.655, 0.278), // top of the beak
        (0.905, 0.408), // beak, tip
        (0.658, 0.472), // beak, underside
        (0.612, 0.560), // cheek
        (0.700, 0.678), // the notch at the throat
        (0.636, 0.772),
        (0.652, 0.905), // neck, bottom right
    ];
    // The bird sits a little inside the plate, so its outline never runs
    // into the rounded edge.
    let shrink = |p: (f32, f32)| -> (f32, f32) {
        (0.5 + (p.0 - 0.5) * 0.80, 0.5 + (p.1 - 0.5) * 0.80)
    };
    let head = head.map(shrink);
    // The engraved hatching across the neck and cheek.
    let hatches: [(f32, f32, f32, f32); 11] = [
        (0.200, 0.400, 0.268, 0.516),
        (0.272, 0.362, 0.338, 0.478),
        (0.342, 0.452, 0.408, 0.568),
        (0.198, 0.556, 0.266, 0.672),
        (0.280, 0.582, 0.348, 0.698),
        (0.360, 0.622, 0.428, 0.738),
        (0.222, 0.700, 0.290, 0.816),
        (0.310, 0.742, 0.378, 0.858),
        (0.420, 0.500, 0.482, 0.604),
        (0.440, 0.660, 0.508, 0.776),
        (0.498, 0.300, 0.556, 0.400),
    ];
    let hatches = hatches.map(|(x0, y0, x1, y1)| {
        let a = shrink((x0, y0));
        let b = shrink((x1, y1));
        (a.0, a.1, b.0, b.1)
    });
    let eye = shrink((0.452, 0.252));
    let glint = shrink((0.466, 0.238));
    let dist_to_seg = |x: f32, y: f32, x0: f32, y0: f32, x1: f32, y1: f32| -> f32 {
        let (dx, dy) = (x1 - x0, y1 - y0);
        let len2 = dx * dx + dy * dy;
        if len2 <= f32::EPSILON {
            return ((x - x0).powi(2) + (y - y0).powi(2)).sqrt();
        }
        let t = (((x - x0) * dx + (y - y0) * dy) / len2).clamp(0.0, 1.0);
        ((x - (x0 + t * dx)).powi(2) + (y - (y0 + t * dy)).powi(2)).sqrt()
    };
    // The plate's rounded rectangle, as a signed distance.
    let plate_sd = |x: f32, y: f32| -> f32 {
        let (hw, hh, r) = (0.455, 0.455, 0.225);
        let qx = (x - 0.5).abs() - (hw - r);
        let qy = (y - 0.5).abs() - (hh - r);
        let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
        outside + qx.max(qy).min(0.0) - r
    };
    // A white plate with a hairline rim, so the icon still reads on a
    // white background; the bird keeps its black outline.
    let top = (0.988f32, 0.988, 0.982);
    let bottom = (0.957f32, 0.957, 0.949);
    let paper = (0.973f32, 0.973, 0.961);
    let ink = (0.075f32, 0.078, 0.082);
    let outline_w = 0.0105;
    let mut pixels = Vec::with_capacity(size * size);
    const N: usize = 4;
    for py in 0..size {
        for pxi in 0..size {
            let mut acc = [0.0f32; 4];
            let mut acc_a = 0.0f32;
            for sy in 0..N {
                for sx in 0..N {
                    let x = (pxi as f32 + (sx as f32 + 0.5) / N as f32) / s;
                    let y = (py as f32 + (sy as f32 + 0.5) / N as f32) / s;
                    let sd = plate_sd(x, y);
                    if sd > 0.0 {
                        acc_a += 1.0;
                        continue;
                    }
                    // The plate: white, with a soft shadow towards the
                    // bottom and a hairline rim just inside the edge.
                    let t = y.clamp(0.0, 1.0);
                    let mut c = if sd > -0.0038 {
                        (0.878f32, 0.878, 0.870, 1.0)
                    } else {
                        (
                            top.0 + (bottom.0 - top.0) * t,
                            top.1 + (bottom.1 - top.1) * t,
                            top.2 + (bottom.2 - top.2) * t,
                            1.0,
                        )
                    };
                    // The bird, then its outline and hatching on top.
                    let near = x > 0.09 && x < 0.93 && y > 0.03 && y < 0.93;
                    if near {
                        let inside = inside_poly(x, y, &head);
                        let mut edge = false;
                        for i in 0..head.len() {
                            let a = head[i];
                            let b = head[(i + 1) % head.len()];
                            if dist_to_seg(x, y, a.0, a.1, b.0, b.1) < outline_w {
                                edge = true;
                                break;
                            }
                        }
                        if inside {
                            c = (paper.0, paper.1, paper.2, 1.0);
                        }
                        // The eye: a black bead with a white glint.
                        let de = ((x - eye.0).powi(2) + (y - eye.1).powi(2)).sqrt();
                        if de < 0.0345 {
                            c = (ink.0, ink.1, ink.2, 1.0);
                        }
                        let dg = ((x - glint.0).powi(2) + (y - glint.1).powi(2)).sqrt();
                        if dg < 0.0100 {
                            c = (paper.0, paper.1, paper.2, 1.0);
                        }
                        if inside {
                            for (x0, y0, x1, y1) in hatches {
                                if dist_to_seg(x, y, x0, y0, x1, y1) < 0.0058 {
                                    c = (ink.0, ink.1, ink.2, 1.0);
                                }
                            }
                        }
                        if edge {
                            c = (ink.0, ink.1, ink.2, 1.0);
                        }
                    }
                    acc[0] += c.0 * c.3;
                    acc[1] += c.1 * c.3;
                    acc[2] += c.2 * c.3;
                    acc[3] += c.3;
                    acc_a += 1.0;
                }
            }
            let a = acc_a.max(1.0);
            let alpha = acc[3] / a;
            pixels.push(if alpha <= 0.001 {
                Color32::TRANSPARENT
            } else {
                let r = (acc[0] / acc[3]).clamp(0.0, 1.0);
                let g = (acc[1] / acc[3]).clamp(0.0, 1.0);
                let b = (acc[2] / acc[3]).clamp(0.0, 1.0);
                Color32::from_rgba_unmultiplied(
                    (r * 255.0).round() as u8,
                    (g * 255.0).round() as u8,
                    (b * 255.0).round() as u8,
                    (alpha * 255.0).round() as u8,
                )
            });
        }
    }
    egui::ColorImage {
        size: [size, size],
        pixels,
        source_size: Vec2::new(size as f32, size as f32),
    }
}

/// Writes a PNG without pulling in an image crate.
fn write_png(path: &std::path::Path, image: &egui::ColorImage) {
    let (w, h) = (image.width() as u32, image.height() as u32);
    let mut raw = Vec::with_capacity((w * 4 + 1) as usize * h as usize);
    for y in 0..h {
        raw.push(0);
        for x in 0..w {
            let c = image[(x as usize, y as usize)];
            // The stored bytes are straight (unmultiplied) RGBA.
            let (r, g, b, a) = (c.r(), c.g(), c.b(), c.a());
            let un = |v: u8| if a == 0 { 0 } else { ((v as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8 };
            raw.extend_from_slice(&[un(r), un(g), un(b), a]);
        }
    }
    let png = encode_png_rgba(w, h, &raw);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // Write beside the target and move it into place in one step, so the
    // watchdog never sees a half-written file.
    let temp = path.with_extension("png.part");
    if std::fs::write(&temp, &png).is_ok() {
        let _ = std::fs::rename(&temp, path);
        return;
    }
    let _ = std::fs::write(path, png);
}

fn encode_png_rgba(w: u32, h: u32, raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    let compressed = zlib_stored(raw);
    chunk(&mut out, b"IDAT", &compressed);
    chunk(&mut out, b"IEND", &[]);
    out
}

/// zlib stream using stored (uncompressed) deflate blocks. Valid, if bulky.
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let mut i = 0;
    while i < data.len() {
        let n = (data.len() - i).min(65535);
        let last = i + n >= data.len();
        out.push(if last { 1 } else { 0 });
        out.extend_from_slice(&(n as u16).to_le_bytes());
        out.extend_from_slice(&(!(n as u16)).to_le_bytes());
        out.extend_from_slice(&data[i..i + n]);
        i += n;
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    out.extend_from_slice(&((b << 16) | a).to_be_bytes());
    out
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, slot) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *slot = c;
    }
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

