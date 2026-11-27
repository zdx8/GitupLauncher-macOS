//! Top bar, toolbar and footer chrome around the main content.

use crate::catalog::repos;
use crate::self_update::SelfUpdateState;
use crate::state::{Layout as View, SortBy};
use crate::theme::*;
use crate::version;
use crate::widgets::{line, measure, primary_button};
use crate::{
    Filter, Launcher, Page, FOOTER_H, MARGIN, TOPBAR_H, WORDMARK_X,
};
use eframe::egui::{self, pos2, vec2, Align2, Color32, Rect, Sense, Stroke, StrokeKind, TextEdit, Ui};

impl Launcher {
    pub(crate) fn top_bar(&mut self, ui: &mut Ui, rect: Rect, p: &Palette) {
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
                SelfUpdateState::Ready { version, .. } => Some((
                    format!("V{} 已就绪 · 重启更新", version.to_uppercase()),
                    Color32::from_rgb(0x3D, 0xC9, 0x7A),
                    true,
                    2,
                )),
                SelfUpdateState::Available(rel) => Some((
                    format!("发现新版 V{} · 更新", version::clean_tag(&rel.tag).to_uppercase()),
                    p.amber,
                    false,
                    1,
                )),
                SelfUpdateState::Downloading { .. } => {
                    let pct = self.self_update.progress_pct().unwrap_or(0);
                    Some((format!("启动器更新中 {pct}%"), p.accent, false, 0))
                }
                SelfUpdateState::Staging => {
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
            SelfUpdateState::Available(_) | SelfUpdateState::Ready { .. }
        ) {
            ui.painter().circle_filled(
                pos2(gear_rect.max.x - 2.0, gear_rect.min.y + 3.0),
                3.5,
                p.amber,
            );
        }
    }

    pub(crate) fn toolbar(&mut self, ui: &mut Ui, rect: Rect, p: &Palette) {
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

    pub(crate) fn footer(&mut self, ui: &mut Ui, rect: Rect, p: &Palette) {
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
