//! Per-app detail page and its navigation chrome.

use crate::catalog::repos;
use crate::github;
use crate::state::{FetchState, JobPhase};
use crate::theme::*;
use crate::version;
use crate::widgets::{action_button, line, measure, primary_button};
use crate::{Launcher, Page, DETAIL_MARGIN, RIGHT_GUTTER};
use eframe::egui::{self, pos2, vec2, Align2, Rect, Sense, Stroke, StrokeKind, Ui};

impl Launcher {
    /// Leave whatever pop-ups are open and show an app's page.
    pub(crate) fn open_page(&mut self, idx: usize) {
        self.page = Page::Detail(idx);
        self.detail_scroll = 0.0;
        self.detail_expanded = false;
        self.menu_for = None;
        self.sort_open = false;
    }

    /// The detail page's navigation cluster: back, forward and close.
    pub(crate) fn draw_detail_nav(&mut self, ui: &mut Ui, rect: Rect, mid: f32, idx: usize, p: &Palette) {
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
    pub(crate) fn draw_nav_cell(&mut self, ui: &mut Ui, which: usize, x: f32, mid: f32, p: &Palette, size: f32) {
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
    pub(crate) fn step_page(&mut self, from: usize, forward: bool) {
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
    pub(crate) fn draw_close_button(&mut self, ui: &mut Ui, rect: Rect, mid: f32, p: &Palette) {
        let nav_end = rect.max.x - 56.0;
        self.draw_nav_cell(ui, 2, nav_end - 28.0, mid, p, 28.0);
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

    /// One app's own page: what it is, what it does, and what its newest
    /// release changed. Everything sits in one scrolling column, laid out
    /// once to measure and again to draw.
    pub(crate) fn detail(&mut self, ui: &mut Ui, band: Rect, idx: usize, p: &Palette) {
        let content_h = self.detail_pass(ui, band, idx, p, false);
        self.detail_scroll = self.apply_wheel(ui, band, content_h, self.detail_scroll);
        self.detail_pass(ui, band, idx, p, true);
        self.detail_scroll =
            self.scrollbar(ui, band, content_h, self.detail_scroll, "detail", p);
    }

    /// The page's layout, run twice: once with `draw` off to learn how
    /// tall it is, then again to paint with the scroll applied.
    pub(crate) fn detail_pass(
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
}
