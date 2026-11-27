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
mod cache;
mod catalog;
mod chrome_ui;
mod detail_ui;
mod fonts;
mod github;
mod grid_ui;
mod icon;
mod install;
mod jobs;
mod modals_ui;
mod self_update;
mod settings_ui;
mod state;
mod theme;
mod version;
mod widgets;

pub(crate) use widgets::{
    action_button, clipboard_text, fit_text, human_size, human_speed, line, measure, primary_button,
    set_movable_by_background, sort_option_label, theme_preference,
};

use catalog::repos;
use eframe::egui::{self, pos2, vec2, Rect, Stroke, Ui, Vec2};
use state::{FetchState, Layout as View, Msg, State};
use std::sync::atomic::AtomicBool;
use theme::*;

/// The height of the top bar.
pub(crate) const TOPBAR_H: f32 = 44.0;
/// The height of the toolbar above the card grid.
pub(crate) const TOOLBAR_H: f32 = 50.0;
/// The height of the footer strip.
pub(crate) const FOOTER_H: f32 = 30.0;
/// Where the wordmark starts, clear of the traffic lights.
pub(crate) const WORDMARK_X: f32 = 92.0;
/// A few addresses, for the bulk importer's demo screen.
pub(crate) const SAMPLE_LIST: &str = "https://github.com/diamondfsd/luna-ai-cut
zdx8/NetScope-macOS
# 以 # 开头的行会被跳过
不是链接的一行";

/// The height of the 未安装 heading in the grid.
pub(crate) const SECTION_H: f32 = 32.0;
/// The width of one card's app tile.
pub(crate) const TILE: f32 = 44.0;
/// Balanced card height so 3 full rows fit comfortably in the default window.
pub(crate) const CARD_H: f32 = 192.0;
/// Gap between cards in the grid.
pub(crate) const CARD_GAP: f32 = 12.0;
/// Minimum width of a single card when computing responsive column count.
pub(crate) const CARD_MIN_W: f32 = 274.0;
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
pub(crate) enum Filter {
    All,
    Installed,
    Updatable,
}

pub(crate) struct Launcher {
    pub(crate) state: State,
    pub(crate) demo: Demo,
    pub(crate) pending_menu: Option<(Rect, usize)>,
    pub(crate) pending_sort: Option<Rect>,
    pub(crate) search: String,
    pub(crate) filter: Filter,
    pub(crate) menu_for: Option<usize>,
    pub(crate) sort_open: bool,
    pub(crate) tx: std::sync::mpsc::Sender<Msg>,
    pub(crate) rx: std::sync::mpsc::Receiver<Msg>,
    pub(crate) ctx: egui::Context,
    pub(crate) icons: Vec<Option<egui::TextureHandle>>,
    pub(crate) icon_uv: Vec<(f32, f32, f32, f32)>,
    pub(crate) page: Page,
    pub(crate) detail_scroll: f32,
    pub(crate) settings_scroll: f32,
    pub(crate) detail_expanded: bool,
    pub(crate) settings: state::Settings,
    pub(crate) self_update: self_update::SelfUpdateState,
    pub(crate) descriptions: std::collections::BTreeMap<String, String>,
    pub(crate) releases: std::collections::BTreeMap<String, serde_json::Value>,
    /// Cached HTTP ETags per repository slug for 304 Not Modified responses.
    pub(crate) etags: std::collections::BTreeMap<String, String>,
    /// Cached 2-line clamped descriptions keyed by (slug, width_px, summary_len).
    pub(crate) clamp_cache: std::collections::HashMap<(String, u32, usize), String>,
    pub(crate) hover_demo: Option<usize>,
    pub(crate) pending_scroll_end: bool,
    pub(crate) picker: Option<Picker>,
    pub(crate) batch: Option<Batch>,
    pub(crate) window_zoomed: bool,
    pub(crate) search_rect: Rect,
    /// Text input rects on the settings page (token, install_dir) to suppress window drag.
    pub(crate) settings_text_rects: [Rect; 2],
    pub(crate) drag_blocked: bool,
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
            descriptions: cache::load_descriptions(),
            releases: cache::load_releases(),
            etags: cache::load_etags(),
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
