//! Redesigned Settings screen: grouped elevated cards, self-updater hero card,
//! toggle switches, GitHub token config, install directory selector, and
//! clean chip-grid subscription management with smooth scrolling.

use crate::catalog::{self, repos};
use crate::self_update::{self, SelfUpdateState};
use crate::state;
use crate::theme::*;
use crate::{
    action_button, fit_text, line, measure, primary_button, theme_preference, Launcher, Page,
    DETAIL_MARGIN, RIGHT_GUTTER,
};
use eframe::egui::{self, pos2, vec2, Align2, Color32, Rect, Sense, Stroke, StrokeKind, TextEdit, Ui};

impl Launcher {
    /// Scrollable grouped-card settings page with an anchored bottom action bar.
    pub(crate) fn settings_page(&mut self, ui: &mut Ui, band: Rect, p: &Palette) {
        let bottom_bar_h = 56.0;
        let scroll_band = Rect::from_min_max(
            band.min,
            pos2(band.max.x, (band.max.y - bottom_bar_h).max(band.min.y + 80.0)),
        );

        let content_h = self.settings_pass(ui, scroll_band, p, false);
        self.settings_scroll =
            self.apply_wheel(ui, scroll_band, content_h, self.settings_scroll);
        self.settings_pass(ui, scroll_band, p, true);
        self.settings_scroll = self.scrollbar(
            ui,
            scroll_band,
            content_h,
            self.settings_scroll,
            "settings",
            p,
        );

        // Anchored bottom bar: Quit on the left, Done on the right.
        let bar = Rect::from_min_max(pos2(band.min.x, scroll_band.max.y), band.max);
        let painter = ui.painter().with_clip_rect(band);
        painter.rect_filled(bar, 0, p.bg);
        painter.hline(bar.min.x..=bar.max.x, bar.min.y, line(p.border));

        let left = band.min.x + DETAIL_MARGIN;
        let right = band.max.x - RIGHT_GUTTER;
        let mid_y = bar.center().y;

        let quit_rect = Rect::from_min_size(pos2(left, mid_y - 16.0), vec2(156.0, 32.0));
        if action_button(
            ui,
            &painter,
            quit_rect,
            ui.scope_id().with("settings-quit"),
            "退出 Gitup Launcher",
            12.5,
            bar,
            p,
        ) {
            std::process::exit(0);
        }

        let done_rect = Rect::from_min_size(pos2(right - 104.0, mid_y - 16.0), vec2(104.0, 32.0));
        if primary_button(
            ui,
            &painter,
            done_rect,
            ui.scope_id().with("settings-done"),
            "完成",
            13.0,
            bar,
            p,
        ) {
            self.page = Page::Grid;
        }
    }

    /// Two-pass layout for the settings content (measure pass + draw pass).
    fn settings_pass(&mut self, ui: &mut Ui, band: Rect, p: &Palette, draw: bool) -> f32 {
        let painter = ui.painter().with_clip_rect(band);
        let dark = p.bg.r() < 128;
        let card_bg = if dark {
            Color32::from_rgb(0x0C, 0x0D, 0x0F)
        } else {
            Color32::WHITE
        };
        let green = Color32::from_rgb(0x3D, 0xC9, 0x7A);

        let left = band.min.x + DETAIL_MARGIN;
        let total_w = (band.width() - DETAIL_MARGIN - RIGHT_GUTTER).max(400.0);
        let col_gap = 18.0;
        let col_w = (total_w - col_gap) / 2.0;
        let right_x = left + col_w + col_gap;
        let scroll = self.settings_scroll;
        let at = |cy: f32| band.min.y + cy - scroll;

        // ------------------------------------------------------------ header
        if draw {
            draw_g_mark(
                &painter,
                Rect::from_center_size(pos2(left + 10.0, at(28.0)), vec2(20.0, 18.0)),
                p.text,
            );
            painter.text(
                pos2(left + 28.0, at(28.0)),
                Align2::LEFT_CENTER,
                "设置",
                display(22.0),
                p.text,
            );
            let ver_text = format!("V{}", self_update::current_version().to_uppercase());
            let ver_w = measure(ui, &ver_text, label(10.5)) + 14.0;
            let ver_pill = Rect::from_min_max(
                pos2(left + 84.0, at(28.0) - 9.5),
                pos2(left + 84.0 + ver_w, at(28.0) + 9.5),
            );
            painter.rect_filled(ver_pill, radius(9), p.search_fill);
            painter.rect_stroke(ver_pill, radius(9), line(p.border), StrokeKind::Middle);
            painter.text(
                ver_pill.center(),
                Align2::CENTER_CENTER,
                ver_text,
                label(10.5),
                p.muted,
            );
        }

        let start_y = 58.0;
        let card_pad = 16.0;
        let card_gap = 14.0;

        // ============================================================ LEFT COLUMN
        let mut ly = start_y;

        // ---------------------------------------- Card L1: Launcher Self-Update
        let l1_h = 154.0;
        let l1_rect = Rect::from_min_size(pos2(left, at(ly)), vec2(col_w, l1_h));
        if draw {
            painter.rect_filled(l1_rect, radius(10), card_bg);
            let highlight_border = matches!(
                self.self_update,
                SelfUpdateState::Available(_) | SelfUpdateState::Ready { .. }
            );
            painter.rect_stroke(
                l1_rect,
                radius(10),
                if highlight_border {
                    Stroke::new(1.2, p.accent)
                } else {
                    line(p.border)
                },
                StrokeKind::Middle,
            );

            let ix = l1_rect.min.x + card_pad;
            let iy = l1_rect.min.y + card_pad;
            painter.text(
                pos2(ix, iy + 8.0),
                Align2::LEFT_CENTER,
                "启动器自动更新",
                text(14.5),
                p.text,
            );

            // Live status pill at top-right of card
            let (badge_txt, badge_color) = match &self.self_update {
                SelfUpdateState::Idle => ("等待检查".to_string(), p.muted),
                SelfUpdateState::Checking => ("正在检查…".to_string(), p.accent),
                SelfUpdateState::UpToDate => ("已是最新".to_string(), green),
                SelfUpdateState::Available(rel) => (
                    format!("新版本 V{}", crate::version::clean_tag(&rel.tag).to_uppercase()),
                    p.amber,
                ),
                SelfUpdateState::Downloading { .. } => {
                    let pct = self.self_update.progress_pct().unwrap_or(0);
                    (format!("下载中 {pct}%"), p.accent)
                }
                SelfUpdateState::Staging => ("准备中…".to_string(), p.accent),
                SelfUpdateState::Ready { version, .. } => {
                    (format!("V{} 已就绪", version.to_uppercase()), green)
                }
                SelfUpdateState::Error(_) => ("检查失败".to_string(), p.amber),
            };
            let bw = measure(ui, &badge_txt, label(10.5)) + 16.0;
            let badge = Rect::from_min_max(
                pos2(l1_rect.max.x - card_pad - bw, iy - 2.0),
                pos2(l1_rect.max.x - card_pad, iy + 18.0),
            );
            painter.rect_filled(
                badge,
                radius(9),
                Color32::from_rgba_unmultiplied(
                    badge_color.r(),
                    badge_color.g(),
                    badge_color.b(),
                    28,
                ),
            );
            painter.text(
                badge.center(),
                Align2::CENTER_CENTER,
                &badge_txt,
                label(10.5),
                badge_color,
            );

            // Subtitle line
            let sub = match &self.self_update {
                SelfUpdateState::Error(e) => fit_text(ui, e, text(12.0), col_w - card_pad * 2.0),
                SelfUpdateState::Ready { version, .. } => {
                    format!("Gitup Launcher V{} 已下载完成，重启后自动生效。", version.to_uppercase())
                }
                SelfUpdateState::Available(rel) => {
                    format!(
                        "发现新版本 V{}（当前 V{}），可一键下载并重启替换。",
                        crate::version::clean_tag(&rel.tag).to_uppercase(),
                        self_update::current_version().to_uppercase()
                    )
                }
                _ => format!(
                    "当前版本 V{} · 来自 {}",
                    self_update::current_version().to_uppercase(),
                    self_update::self_repo_slug()
                ),
            };
            painter.text(
                pos2(ix, iy + 32.0),
                Align2::LEFT_CENTER,
                fit_text(ui, &sub, text(12.0), col_w - card_pad * 2.0),
                text(12.0),
                p.muted,
            );

            // Divider inside card
            painter.hline(ix..=l1_rect.max.x - card_pad, iy + 52.0, line(p.border));

            // Toggle: auto-download self updates
            let row_y = iy + 74.0;
            painter.text(
                pos2(ix, row_y),
                Align2::LEFT_CENTER,
                "发现新版本时自动后台下载",
                text(13.0),
                p.text,
            );
            let sw = Rect::from_min_size(
                pos2(l1_rect.max.x - card_pad - 36.0, row_y - 10.0),
                vec2(36.0, 20.0),
            );
            if toggle_switch(
                ui,
                &painter,
                sw,
                ui.scope_id().with("sw-self-auto"),
                self.settings.self_auto_update,
                band,
                p,
            ) {
                self.settings.self_auto_update = !self.settings.self_auto_update;
                state::save_settings(&self.settings);
            }

            // Action button row
            let act_y = l1_rect.max.y - card_pad - 30.0;
            let mut self_action: u8 = 0; // 1 = check, 2 = download, 3 = restart
            match &self.self_update {
                SelfUpdateState::Ready { .. } => {
                    let btn = Rect::from_min_size(pos2(ix, act_y), vec2(148.0, 30.0));
                    if primary_button(
                        ui,
                        &painter,
                        btn,
                        ui.scope_id().with("self-upd-restart"),
                        "立即重启更新",
                        12.5,
                        band,
                        p,
                    ) {
                        self_action = 3;
                    }
                }
                SelfUpdateState::Available(_) => {
                    let btn = Rect::from_min_size(pos2(ix, act_y), vec2(132.0, 30.0));
                    if primary_button(
                        ui,
                        &painter,
                        btn,
                        ui.scope_id().with("self-upd-download"),
                        "下载并更新",
                        12.5,
                        band,
                        p,
                    ) {
                        self_action = 2;
                    }
                }
                SelfUpdateState::Downloading { done, total, cancel } => {
                    let progress = if *total > 0 {
                        (*done as f32 / *total as f32).clamp(0.0, 1.0)
                    } else {
                        0.3
                    };
                    let cancel_w = 56.0;
                    let bar_w = (col_w - card_pad * 2.0 - cancel_w - 10.0).max(60.0);
                    let bar_r = Rect::from_min_size(pos2(ix, act_y + 11.0), vec2(bar_w, 8.0));
                    painter.rect_filled(bar_r, radius(4), p.search_fill);
                    let fill_r = Rect::from_min_size(
                        bar_r.min,
                        vec2(bar_r.width() * progress, bar_r.height()),
                    );
                    painter.rect_filled(fill_r, radius(4), p.accent);
                    let cancel_btn = Rect::from_min_size(
                        pos2(ix + bar_w + 10.0, act_y + 1.0),
                        vec2(cancel_w, 28.0),
                    );
                    if action_button(
                        ui,
                        &painter,
                        cancel_btn,
                        ui.scope_id().with("self-upd-cancel"),
                        "取消",
                        12.0,
                        band,
                        p,
                    ) {
                        cancel.store(true, std::sync::atomic::Ordering::SeqCst);
                    }
                }
                SelfUpdateState::Checking | SelfUpdateState::Staging => {
                    let btn = Rect::from_min_size(pos2(ix, act_y), vec2(124.0, 30.0));
                    painter.rect_stroke(btn, radius(5), line(p.border), StrokeKind::Middle);
                    painter.text(
                        btn.center(),
                        Align2::CENTER_CENTER,
                        if matches!(self.self_update, SelfUpdateState::Checking) {
                            "正在检查…"
                        } else {
                            "正在准备…"
                        },
                        text(12.5),
                        p.faint,
                    );
                }
                SelfUpdateState::Idle | SelfUpdateState::UpToDate | SelfUpdateState::Error(_) => {
                    let btn = Rect::from_min_size(pos2(ix, act_y), vec2(124.0, 30.0));
                    if action_button(
                        ui,
                        &painter,
                        btn,
                        ui.scope_id().with("self-upd-check"),
                        "检查启动器更新",
                        12.5,
                        band,
                        p,
                    ) {
                        self_action = 1;
                    }
                }
            }
            match self_action {
                1 => self.check_self_update(),
                2 => self.start_self_update_download(),
                3 => self.apply_self_update_and_relaunch(),
                _ => {}
            }
        }
        ly += l1_h + card_gap;

        // ---------------------------------------- Card L2: Appearance & Updates
        let l2_h = 248.0;
        let l2_rect = Rect::from_min_size(pos2(left, at(ly)), vec2(col_w, l2_h));
        if draw {
            painter.rect_filled(l2_rect, radius(10), card_bg);
            painter.rect_stroke(l2_rect, radius(10), line(p.border), StrokeKind::Middle);

            let ix = l2_rect.min.x + card_pad;
            let inner_w = col_w - card_pad * 2.0;
            let mut cy = l2_rect.min.y + card_pad;

            painter.text(pos2(ix, cy + 8.0), Align2::LEFT_CENTER, "外观与检查策略", text(14.5), p.text);
            cy += 28.0;

            // Recessed segmented theme control
            let seg = Rect::from_min_size(pos2(ix, cy), vec2(inner_w, 32.0));
            painter.rect_filled(seg, radius(7), p.search_fill);
            painter.rect_stroke(seg, radius(7), line(p.border), StrokeKind::Middle);
            let cell_w = inner_w / 3.0;
            for (i, (word, mode)) in [
                ("跟随系统", state::ThemeMode::System),
                ("深色", state::ThemeMode::Dark),
                ("浅色", state::ThemeMode::Light),
            ]
            .iter()
            .enumerate()
            {
                let cell = Rect::from_min_max(
                    pos2(seg.min.x + i as f32 * cell_w, seg.min.y),
                    pos2(seg.min.x + (i as f32 + 1.0) * cell_w, seg.max.y),
                );
                let chosen = self.settings.theme == *mode;
                let hit = cell.intersect(band);
                let mut hovered = false;
                if hit.is_positive() {
                    let resp = ui.interact(
                        hit,
                        ui.scope_id().with(("settings-theme", i)),
                        Sense::click(),
                    );
                    hovered = resp.hovered();
                    if hovered {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if resp.clicked() && !chosen {
                        self.settings.theme = *mode;
                        self.ctx.set_theme(theme_preference(*mode));
                        state::save_settings(&self.settings);
                    }
                }
                if chosen {
                    painter.rect_filled(cell.shrink(2.0), radius(5), p.text);
                } else if hovered {
                    painter.rect_filled(cell.shrink(2.0), radius(5), p.surface);
                }
                painter.text(
                    cell.center(),
                    Align2::CENTER_CENTER,
                    *word,
                    text(12.5),
                    if chosen { p.bg } else { p.text },
                );
            }
            cy += 48.0;
            painter.hline(ix..=ix + inner_w, cy, line(p.border));
            cy += 18.0;

            // Toggle rows
            for (key, title, hint) in [
                (
                    "auto_check",
                    "启动时自动检查应用更新",
                    "打开启动器时自动向 GitHub 查询各仓库最新发布",
                ),
                (
                    "include_pre",
                    "包含预发布版本 (Beta / RC)",
                    "把测试版与候选版纳入更新检测",
                ),
            ] {
                let checked = if key == "auto_check" {
                    self.settings.auto_check
                } else {
                    self.settings.include_pre
                };
                painter.text(pos2(ix, cy + 4.0), Align2::LEFT_CENTER, title, text(13.0), p.text);
                painter.text(
                    pos2(ix, cy + 21.0),
                    Align2::LEFT_CENTER,
                    fit_text(ui, hint, text(11.5), inner_w - 48.0),
                    text(11.5),
                    p.faint,
                );
                let sw = Rect::from_min_size(pos2(ix + inner_w - 36.0, cy + 2.0), vec2(36.0, 20.0));
                if toggle_switch(
                    ui,
                    &painter,
                    sw,
                    ui.scope_id().with(("settings-sw", key)),
                    checked,
                    band,
                    p,
                ) {
                    if key == "auto_check" {
                        self.settings.auto_check = !self.settings.auto_check;
                    } else {
                        self.settings.include_pre = !self.settings.include_pre;
                        self.etags.clear();
                    }
                    state::save_settings(&self.settings);
                }
                cy += 46.0;
            }

            let check_btn = Rect::from_min_size(pos2(ix, l2_rect.max.y - card_pad - 30.0), vec2(132.0, 30.0));
            if action_button(
                ui,
                &painter,
                check_btn,
                ui.scope_id().with("settings-check-now"),
                "立即检查全部应用",
                12.5,
                band,
                p,
            ) {
                self.spawn_meta_fetch_all();
            }
        }
        ly += l2_h + card_gap;

        // ---------------------------------------- Card L3: GitHub API Token
        let l3_h = 128.0;
        let l3_rect = Rect::from_min_size(pos2(left, at(ly)), vec2(col_w, l3_h));
        if draw {
            painter.rect_filled(l3_rect, radius(10), card_bg);
            painter.rect_stroke(l3_rect, radius(10), line(p.border), StrokeKind::Middle);

            let ix = l3_rect.min.x + card_pad;
            let inner_w = col_w - card_pad * 2.0;
            let iy = l3_rect.min.y + card_pad;

            painter.text(
                pos2(ix, iy + 8.0),
                Align2::LEFT_CENTER,
                "GitHub Token（可选）",
                text(14.5),
                p.text,
            );

            let has_token = !self.settings.github_token.trim().is_empty();
            let (st_txt, st_col) = if has_token {
                ("已配置 · 5000次/时", green)
            } else {
                ("匿名 · 60次/时", p.muted)
            };
            let sw = measure(ui, st_txt, label(10.5)) + 14.0;
            let st_pill = Rect::from_min_max(
                pos2(ix + inner_w - sw, iy - 1.5),
                pos2(ix + inner_w, iy + 17.5),
            );
            painter.rect_filled(
                st_pill,
                radius(9),
                Color32::from_rgba_unmultiplied(st_col.r(), st_col.g(), st_col.b(), 24),
            );
            painter.text(st_pill.center(), Align2::CENTER_CENTER, st_txt, label(10.5), st_col);

            let token_rect = Rect::from_min_size(pos2(ix, iy + 30.0), vec2(inner_w, 34.0));
            self.settings_text_rects[0] = token_rect;
            let token_id = egui::Id::unique("settings-token");
            let token_focused = ui.memory(|m| m.has_focus(token_id));
            painter.rect_filled(token_rect, radius(6), p.search_fill);
            painter.rect_stroke(
                token_rect,
                radius(6),
                if token_focused {
                    Stroke::new(1.5, p.accent)
                } else {
                    line(p.border)
                },
                StrokeKind::Middle,
            );
            if self.settings.github_token.is_empty() {
                painter.text(
                    pos2(token_rect.min.x + 12.0, token_rect.center().y),
                    Align2::LEFT_CENTER,
                    "粘贴 ghp_xxx 或 github_pat_xxx",
                    text(12.5),
                    p.faint,
                );
            }
            let clear_w = if has_token { 26.0 } else { 10.0 };
            let prev_token = self.settings.github_token.clone();
            let token_inner = Rect::from_min_max(
                pos2(token_rect.min.x + 10.0, token_rect.center().y - 9.0),
                pos2(token_rect.max.x - clear_w, token_rect.center().y + 9.0),
            );
            let mut token_child = ui.new_child(egui::UiBuilder::new().max_rect(token_inner));
            token_child.add(
                TextEdit::singleline(&mut self.settings.github_token)
                    .id(token_id)
                    .password(true)
                    .text_color(p.text)
                    .frame(egui::Frame::NONE)
                    .desired_width(token_inner.width()),
            );
            if has_token {
                let clr = Rect::from_center_size(
                    pos2(token_rect.max.x - 14.0, token_rect.center().y),
                    vec2(18.0, 18.0),
                );
                let hit = clr.intersect(band);
                if hit.is_positive() {
                    let resp = ui.interact(hit, ui.scope_id().with("token-clear"), Sense::click());
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    let st = Stroke::new(1.3, if resp.hovered() { p.text } else { p.muted });
                    let c = clr.center();
                    painter.line_segment([pos2(c.x - 3.5, c.y - 3.5), pos2(c.x + 3.5, c.y + 3.5)], st);
                    painter.line_segment([pos2(c.x + 3.5, c.y - 3.5), pos2(c.x - 3.5, c.y + 3.5)], st);
                    if resp.clicked() {
                        self.settings.github_token.clear();
                    }
                }
            }
            if self.settings.github_token != prev_token {
                self.settings.github_token = self.settings.github_token.trim().to_string();
                state::save_settings(&self.settings);
            }

            painter.text(
                pos2(ix, iy + 82.0),
                Align2::LEFT_CENTER,
                "仅保存在本机 settings.json，用于提高 GitHub API 速率上限。",
                text(11.5),
                p.faint,
            );
        }
        ly += l3_h + card_gap;

        // ============================================================ RIGHT COLUMN
        let mut ry = start_y;

        // ---------------------------------------- Card R1: Install Folder
        let r1_h = 128.0;
        let r1_rect = Rect::from_min_size(pos2(right_x, at(ry)), vec2(col_w, r1_h));
        if draw {
            painter.rect_filled(r1_rect, radius(10), card_bg);
            painter.rect_stroke(r1_rect, radius(10), line(p.border), StrokeKind::Middle);

            let ix = r1_rect.min.x + card_pad;
            let inner_w = col_w - card_pad * 2.0;
            let iy = r1_rect.min.y + card_pad;

            painter.text(
                pos2(ix, iy + 8.0),
                Align2::LEFT_CENTER,
                "默认安装目录",
                text(14.5),
                p.text,
            );

            // Preset pills right-aligned
            let mut qx = ix + inner_w;
            for preset in ["~/Applications", "/Applications"] {
                let pw = measure(ui, preset, label(10.5)) + 16.0;
                let pr = Rect::from_min_max(pos2(qx - pw, iy - 2.0), pos2(qx, iy + 18.0));
                let active = self.settings.install_dir == preset;
                let hit = pr.intersect(band);
                let mut over = false;
                if hit.is_positive() {
                    let resp = ui.interact(
                        hit,
                        ui.scope_id().with(("install-preset", preset)),
                        Sense::click(),
                    );
                    over = resp.hovered();
                    if over {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if resp.clicked() && !active {
                        self.settings.install_dir = preset.to_string();
                        state::save_settings(&self.settings);
                        self.state.rescan_installed(
                            &self.settings.install_dir,
                            &self.settings.bundle_overrides,
                        );
                        self.spawn_icon_load();
                        self.state.status = format!("默认安装目录已切换为 {preset}");
                    }
                }
                if active {
                    painter.rect_filled(pr, radius(5), p.text);
                } else if over {
                    painter.rect_filled(pr, radius(5), p.surface);
                    painter.rect_stroke(pr, radius(5), Stroke::new(1.0, p.accent), StrokeKind::Middle);
                } else {
                    painter.rect_stroke(pr, radius(5), line(p.border), StrokeKind::Middle);
                }
                painter.text(
                    pr.center(),
                    Align2::CENTER_CENTER,
                    preset,
                    label(10.5),
                    if active { p.bg } else { p.muted },
                );
                qx = pr.min.x - 6.0;
            }

            let folder = Rect::from_min_size(pos2(ix, iy + 30.0), vec2(inner_w, 34.0));
            self.settings_text_rects[1] = folder;
            let dir_id = egui::Id::unique("settings-install-dir");
            let dir_focused = ui.memory(|m| m.has_focus(dir_id));
            painter.rect_filled(folder, radius(6), p.search_fill);
            painter.rect_stroke(
                folder,
                radius(6),
                if dir_focused {
                    Stroke::new(1.5, p.accent)
                } else {
                    line(p.border)
                },
                StrokeKind::Middle,
            );
            let prev_dir = self.settings.install_dir.clone();
            let dir_inner = Rect::from_min_max(
                pos2(folder.min.x + 12.0, folder.center().y - 9.0),
                pos2(folder.max.x - 12.0, folder.center().y + 9.0),
            );
            let mut dir_child = ui.new_child(egui::UiBuilder::new().max_rect(dir_inner));
            let dir_resp = dir_child.add(
                TextEdit::singleline(&mut self.settings.install_dir)
                    .id(dir_id)
                    .text_color(p.text)
                    .frame(egui::Frame::NONE)
                    .desired_width(dir_inner.width()),
            );
            if self.settings.install_dir != prev_dir {
                state::save_settings(&self.settings);
            }
            if dir_resp.lost_focus()
                || (dir_resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
            {
                state::save_settings(&self.settings);
                self.state.rescan_installed(
                    &self.settings.install_dir,
                    &self.settings.bundle_overrides,
                );
                self.spawn_icon_load();
            }

            painter.text(
                pos2(ix, iy + 82.0),
                Align2::LEFT_CENTER,
                "新安装应用放入此目录；更新已有应用时保持其当前所在位置。",
                text(11.5),
                p.faint,
            );
        }
        ry += r1_h + card_gap;

        // ---------------------------------------- Card R2: Subscribed Repos
        let list = repos();
        let active_indices: Vec<usize> = list
            .iter()
            .enumerate()
            .filter(|(i, _)| *i < self.state.subscribed.len() && self.state.subscribed[*i])
            .map(|(i, _)| i)
            .collect();
        let removed_builtin = list.iter().enumerate().any(|(i, r)| {
            catalog::is_builtin(r.slug)
                && i < self.state.subscribed.len()
                && !self.state.subscribed[i]
        });

        let sub_cols = 2usize;
        let chip_h = 34.0;
        let chip_gap = 8.0;
        let sub_rows = active_indices.len().div_ceil(sub_cols).max(1);
        let grid_h = sub_rows as f32 * chip_h + (sub_rows.saturating_sub(1)) as f32 * chip_gap;
        let r2_h = (card_pad * 2.0 + 48.0 + grid_h).max(416.0);
        let r2_rect = Rect::from_min_size(pos2(right_x, at(ry)), vec2(col_w, r2_h));

        if draw {
            painter.rect_filled(r2_rect, radius(10), card_bg);
            painter.rect_stroke(r2_rect, radius(10), line(p.border), StrokeKind::Middle);

            let ix = r2_rect.min.x + card_pad;
            let inner_w = col_w - card_pad * 2.0;
            let iy = r2_rect.min.y + card_pad;

            painter.text(pos2(ix, iy + 8.0), Align2::LEFT_CENTER, "已订阅仓库", text(14.5), p.text);

            let cnt_txt = format!("{} 个", active_indices.len());
            let cw = measure(ui, &cnt_txt, label(10.5)) + 14.0;
            let cnt_pill = Rect::from_min_max(
                pos2(ix + 82.0, iy - 1.5),
                pos2(ix + 82.0 + cw, iy + 17.5),
            );
            painter.rect_filled(cnt_pill, radius(9), p.search_fill);
            painter.text(cnt_pill.center(), Align2::CENTER_CENTER, &cnt_txt, label(10.5), p.muted);

            if removed_builtin {
                let caption = "恢复默认仓库";
                let rw = measure(ui, caption, text(12.0)) + 8.0;
                let restore_r = Rect::from_min_size(pos2(ix + inner_w - rw, iy - 2.0), vec2(rw, 20.0));
                let hit = restore_r.intersect(band);
                let mut over = false;
                if hit.is_positive() {
                    let resp = ui.interact(hit, egui::Id::unique("settings-restore"), Sense::click());
                    over = resp.hovered();
                    if over {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if resp.clicked() {
                        self.settings.disabled.clear();
                        for i in 0..self.state.subscribed.len() {
                            self.state.subscribed[i] = true;
                        }
                        state::save_settings(&self.settings);
                        self.spawn_meta_fetch_all();
                        self.state.status = "已恢复全部内置仓库".to_string();
                    }
                }
                painter.text(
                    pos2(restore_r.max.x, restore_r.center().y),
                    Align2::RIGHT_CENTER,
                    caption,
                    text(12.0),
                    if over { p.accent } else { p.muted },
                );
            }

            painter.text(
                pos2(ix, iy + 29.0),
                Align2::LEFT_CENTER,
                "点击右侧 × 可取消订阅（不会卸载已安装的本地应用）。",
                text(11.5),
                p.faint,
            );

            let chip_w = (inner_w - chip_gap) / sub_cols as f32;
            let grid_top = iy + 48.0;

            for (slot, &idx) in active_indices.iter().enumerate() {
                let repo = &list[idx];
                let col = slot % sub_cols;
                let row = slot / sub_cols;
                let cx = ix + col as f32 * (chip_w + chip_gap);
                let cy = grid_top + row as f32 * (chip_h + chip_gap);
                let chip = Rect::from_min_size(pos2(cx, cy), vec2(chip_w, chip_h));

                let x_btn = Rect::from_center_size(
                    pos2(chip.max.x - 16.0, chip.center().y),
                    vec2(20.0, 20.0),
                );
                let hit = x_btn.intersect(band).expand(2.0);
                let mut x_over = false;
                let mut x_clicked = false;
                if hit.is_positive() {
                    let resp = ui.interact(
                        hit,
                        egui::Id::unique(("settings-remove", repo.slug)),
                        Sense::click(),
                    );
                    x_over = resp.hovered();
                    x_clicked = resp.clicked();
                    if x_over {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                }

                painter.rect_filled(chip, radius(7), p.search_fill);
                painter.rect_stroke(
                    chip,
                    radius(7),
                    if x_over {
                        Stroke::new(1.0, p.amber)
                    } else {
                        line(p.border)
                    },
                    StrokeKind::Middle,
                );

                // Colored dot matching the app tile color
                let (r, g, b) = catalog::tile_color(repo.id);
                painter.circle_filled(
                    pos2(chip.min.x + 12.0, chip.center().y),
                    3.5,
                    Color32::from_rgb(r, g, b),
                );

                let name_room = (x_btn.min.x - 6.0 - (chip.min.x + 22.0)).max(24.0);
                let shown = fit_text(ui, repo.name, text(12.5), name_room);
                painter.text(
                    pos2(chip.min.x + 22.0, chip.center().y),
                    Align2::LEFT_CENTER,
                    shown,
                    text(12.5),
                    p.text,
                );

                // Right-aligned × button
                if x_over {
                    painter.rect_filled(
                        x_btn,
                        radius(4),
                        Color32::from_rgba_unmultiplied(p.amber.r(), p.amber.g(), p.amber.b(), 30),
                    );
                }
                let st = Stroke::new(1.4, if x_over { p.amber } else { p.faint });
                let c = x_btn.center();
                painter.line_segment([pos2(c.x - 3.5, c.y - 3.5), pos2(c.x + 3.5, c.y + 3.5)], st);
                painter.line_segment([pos2(c.x + 3.5, c.y - 3.5), pos2(c.x - 3.5, c.y + 3.5)], st);

                if x_clicked {
                    if catalog::is_builtin(repo.slug) {
                        self.toggle_subscription(idx);
                    } else {
                        self.remove_repository(idx);
                    }
                    break;
                }
            }
        }
        ry += r2_h + card_gap;

        ly.max(ry) + 12.0
    }
}

/// Modern macOS-style pill toggle switch.
fn toggle_switch(
    ui: &mut Ui,
    painter: &egui::Painter,
    rect: Rect,
    id: egui::Id,
    on: bool,
    band: Rect,
    p: &Palette,
) -> bool {
    let hit = rect.intersect(band);
    let mut clicked = false;
    if hit.is_positive() {
        let resp = ui.interact(hit, id, Sense::click());
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        clicked = resp.clicked();
    }
    let track_r = rect.height() * 0.5;
    let fill = if on { p.accent } else { p.search_fill };
    painter.rect_filled(rect, track_r, fill);
    if !on {
        painter.rect_stroke(rect, track_r, line(p.border), StrokeKind::Middle);
    }
    let knob_r = track_r - 2.5;
    let knob_x = if on {
        rect.max.x - track_r
    } else {
        rect.min.x + track_r
    };
    painter.circle_filled(
        pos2(knob_x, rect.center().y),
        knob_r,
        if on { p.bg } else { p.muted },
    );
    clicked
}
