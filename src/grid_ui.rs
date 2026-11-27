//! Home grid and list layouts: cards, filtering and scrolling.

use crate::catalog::{self, repos};
use crate::state::{FetchState, JobPhase, Layout as View, SortBy};
use crate::theme::*;
use crate::version;
use crate::widgets::{
    action_button, clamp_two_lines, first_language, fit_text, grid_columns, line, measure,
    primary_button,
};
use crate::{
    Filter, Launcher, Page, CARD_GAP, CARD_H, MARGIN, RIGHT_GUTTER, SECTION_H, TILE, TOOLBAR_H,
    TOPBAR_H,
};
use eframe::egui::{self, pos2, vec2, Align2, Color32, Rect, Sense, Stroke, StrokeKind, Ui};

impl Launcher {
    /// One app's tile, wherever it is drawn.
    pub(crate) fn draw_tile(&self, painter: &egui::Painter, tile: Rect, idx: usize, corner: u8) {
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

    /// The line under an app's name: the repository's own description
    /// (Chinese when it has one), and the built-in text when it has none.
    pub(crate) fn summary_text(&self, idx: usize) -> String {
        let repo = repos()[idx];
        match self.descriptions.get(repo.slug) {
            Some(text) if !text.is_empty() => first_language(text).to_string(),
            _ => repo.summary.to_string(),
        }
    }

    /// The repositories that pass the search box, in the chosen order.
    pub(crate) fn visible(&self) -> Vec<usize> {
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
    pub(crate) fn button_label(&self, idx: usize) -> String {
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

    pub(crate) fn body(&mut self, ui: &mut Ui, rect: Rect, p: &Palette) {
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
    pub(crate) fn grid(&mut self, ui: &mut Ui, band: Rect, apps: &[usize], p: &Palette) {
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
    pub(crate) fn grid_height(&self, apps: &[usize], cols: usize) -> f32 {
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
    pub(crate) fn card(&mut self, ui: &mut Ui, card: Rect, idx: usize, band: Rect, p: &Palette) {
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

    /// Total height of the redesigned list view (header + grouped rows).
    pub(crate) fn list_height(&self, apps: &[usize]) -> f32 {
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
    pub(crate) fn list(&mut self, ui: &mut Ui, band: Rect, apps: &[usize], p: &Palette) {
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
}
