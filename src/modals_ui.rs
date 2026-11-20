//! Pop-up menus, sort dropdown, version picker, and subscription importer.

use crate::catalog::repos;
use crate::state::SortBy;
use crate::theme::*;
use crate::{action_button, line, measure, primary_button, sort_option_label, Launcher, MARGIN};
use eframe::egui::{self, pos2, vec2, Align2, Color32, Rect, Sense, Stroke, StrokeKind, TextEdit, Ui};

impl Launcher {
    /// The small menu beside a card's button, drawn on top of the cards.
    pub(crate) fn draw_menu(
        &mut self,
        ui: &mut Ui,
        anchor: Rect,
        idx: usize,
        body: Rect,
        p: &Palette,
    ) {
        let painter = ui.painter();
        let app = &repos()[idx];
        let installed = self.state.is_installed(idx);
        let mut items: Vec<(&'static str, &'static str)> = Vec::new();
        items.push(("GitHub 仓库", "repo"));
        items.push(("更新说明", "notes"));
        items.push(("所有版本", "all"));
        items.push(("选择版本", "versions"));
        items.push(("", "sep"));
        if installed {
            items.push(("卸载软件", "uninstall"));
        }
        items.push(("删除订阅", "unsub"));

        let w = 168.0;
        let row_h = 34.0;
        let sep_h = 15.0;
        let h: f32 = items
            .iter()
            .map(|(label, _)| if label.is_empty() { sep_h } else { row_h })
            .sum();
        let below = anchor.max.y + 4.0 + h;
        let top = if below > body.max.y {
            anchor.min.y - 4.0 - h
        } else {
            anchor.max.y + 4.0
        };
        let left = if anchor.min.x + w > body.max.x - MARGIN {
            anchor.max.x - w
        } else {
            anchor.min.x
        };
        let rect = Rect::from_min_max(pos2(left, top), pos2(left + w, top + h));
        if let Some(pointer) = ui.input(|i| i.pointer.hover_pos()) {
            if !rect.contains(pointer) && !anchor.expand(8.0).contains(pointer) {
                self.menu_for = None;
                return;
            }
        }
        painter.rect_filled(rect, radius(6), p.surface);
        painter.rect_stroke(rect, radius(6), line(p.border), StrokeKind::Middle);

        let mut y = rect.min.y;
        for (i, (label, action)) in items.iter().enumerate() {
            if label.is_empty() {
                painter.hline(
                    (rect.min.x + 10.0)..=(rect.max.x - 10.0),
                    y + sep_h * 0.5,
                    line(p.border),
                );
                y += sep_h;
                continue;
            }
            let row = Rect::from_min_max(pos2(rect.min.x, y), pos2(rect.max.x, y + row_h));
            let resp = ui.interact(row, egui::Id::unique(("menu", app.id, i)), Sense::click());
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                painter.rect_filled(row.shrink(2.0), radius(4), p.border);
            }
            if resp.clicked() {
                match *action {
                    "versions" => self.open_versions(idx),
                    "notes" => self.open_notes(idx),
                    "repo" => {
                        let url = format!("https://github.com/{}", app.slug);
                        let _ = std::process::Command::new("open").arg(url).spawn();
                    }
                    "all" => {
                        let url = format!("https://github.com/{}/releases", app.slug);
                        let _ = std::process::Command::new("open").arg(url).spawn();
                    }
                    "uninstall" => self.uninstall(idx),
                    "unsub" => self.delete_subscription(idx),
                    _ => {}
                }
                if action != &"versions" {
                    self.menu_for = None;
                }
            }
            painter.text(
                pos2(row.min.x + 14.0, row.center().y),
                Align2::LEFT_CENTER,
                *label,
                text(13.5),
                p.text,
            );
            y += row_h;
        }
    }

    /// The importer: a single address by default, or a whole list once
    /// 批量添加 is chosen.
    pub(crate) fn draw_batch(&mut self, ui: &mut Ui, full: Rect, p: &Palette) {
        let Some(batch) = self.batch.as_mut() else {
            return;
        };
        let scrim = ui.interact(full, egui::Id::unique("batch-scrim"), Sense::click());
        ui.painter()
            .rect_filled(full, 0, Color32::from_black_alpha(150));

        let expanded = batch.expanded;
        let height = if expanded {
            460.0
        } else if batch.report.is_some() {
            300.0
        } else {
            240.0
        };
        let rect = Rect::from_center_size(full.center(), vec2(680.0, height));
        let painter = ui.painter().clone();
        let painter = &painter;

        for (spread, alpha) in [(10.0f32, 18u8), (5.0, 26)] {
            painter.rect_filled(
                rect.translate(vec2(0.0, 6.0)).expand(spread),
                radius(18),
                Color32::from_black_alpha(alpha),
            );
        }
        painter.rect_filled(rect, radius(12), p.bg);
        painter.rect_stroke(rect, radius(12), line(p.border), StrokeKind::Middle);

        painter.text(
            pos2(rect.min.x + 30.0, rect.min.y + 40.0),
            Align2::LEFT_CENTER,
            "添加订阅",
            display(23.0),
            p.text,
        );
        let caption_x = rect.min.x + 30.0 + measure(ui, "添加订阅", display(23.0)) + 14.0;
        painter.text(
            pos2(caption_x, rect.min.y + 40.0),
            Align2::LEFT_CENTER,
            if expanded {
                "每行一个链接或 owner/repo；空行与以 # 开头的行会跳过。"
            } else {
                "粘贴一个链接或 owner/repo，回车即可添加。"
            },
            text(12.5),
            p.faint,
        );
        let close = Rect::from_center_size(
            pos2(rect.max.x - 44.0, rect.min.y + 40.0),
            vec2(26.0, 26.0),
        );
        if expanded {
            let count = batch
                .text
                .lines()
                .filter(|line| {
                    let line = line.trim();
                    !line.is_empty() && !line.starts_with('#')
                })
                .count();
            painter.text(
                pos2(close.min.x - 14.0, rect.min.y + 40.0),
                Align2::RIGHT_CENTER,
                format!("{count} 个地址"),
                label(11.5),
                p.faint,
            );
        }
        let close_hit = ui.interact(close, egui::Id::unique("batch-close"), Sense::click());
        if close_hit.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            painter.rect_filled(close, radius(4), p.surface);
        }
        let cross = Stroke::new(1.5, if close_hit.hovered() { p.text } else { p.muted });
        let centre = close.center();
        painter.line_segment(
            [
                pos2(centre.x - 5.0, centre.y - 5.0),
                pos2(centre.x + 5.0, centre.y + 5.0),
            ],
            cross,
        );
        painter.line_segment(
            [
                pos2(centre.x + 5.0, centre.y - 5.0),
                pos2(centre.x - 5.0, centre.y + 5.0),
            ],
            cross,
        );
        if close_hit.clicked() {
            self.batch = None;
            return;
        }

        let box_rect = Rect::from_min_size(
            pos2(rect.min.x + 30.0, rect.min.y + 76.0),
            vec2(rect.width() - 60.0, if expanded { 196.0 } else { 46.0 }),
        );
        let field_id = if expanded {
            egui::Id::unique("batch-field")
        } else {
            egui::Id::unique("batch-single")
        };
        let focused = ui.memory(|m| m.has_focus(field_id));
        painter.rect_filled(box_rect, radius(8), p.search_fill);
        painter.rect_stroke(
            box_rect,
            radius(8),
            if focused {
                Stroke::new(1.5, p.accent)
            } else {
                line(p.border)
            },
            StrokeKind::Middle,
        );

        let inner = if expanded {
            Rect::from_min_max(
                pos2(box_rect.min.x + 12.0, box_rect.min.y + 10.0),
                pos2(box_rect.max.x - 12.0, box_rect.max.y - 10.0),
            )
        } else {
            Rect::from_min_max(
                pos2(box_rect.min.x + 12.0, box_rect.center().y - 10.0),
                pos2(box_rect.max.x - 12.0, box_rect.center().y + 10.0),
            )
        };
        let empty = if expanded {
            batch.text.is_empty()
        } else {
            batch.single.is_empty()
        };
        if empty {
            painter.text(
                pos2(inner.min.x + 2.0, inner.center().y),
                Align2::LEFT_CENTER,
                if expanded {
                    "https://github.com/owner/repo（一行一个）"
                } else {
                    "https://github.com/owner/repo"
                },
                text(12.0),
                p.faint,
            );
        }
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(inner)
                .id_salt(if expanded { "batch-text" } else { "batch-one" }),
        );
        if batch.focus {
            batch.focus = false;
            ui.ctx().memory_mut(|m| m.request_focus(field_id));
        }
        let edit = if expanded {
            TextEdit::multiline(&mut batch.text)
                .id(field_id)
                .text_color(p.text)
                .frame(egui::Frame::NONE)
                .desired_width(inner.width())
        } else {
            TextEdit::singleline(&mut batch.single)
                .id(field_id)
                .text_color(p.text)
                .frame(egui::Frame::NONE)
                .desired_width(inner.width())
        };
        child.add_sized(inner.size(), edit);

        let link_y = box_rect.max.y + 24.0;
        let caption = if expanded {
            "－ 收起批量输入"
        } else {
            "＋ 批量添加"
        };
        let link = Rect::from_min_size(
            pos2(rect.min.x + 30.0, link_y - 11.0),
            vec2(measure(ui, caption, text(12.5)) + 8.0, 22.0),
        );
        let link_hit = ui.interact(link, egui::Id::unique("batch-toggle"), Sense::click());
        if link_hit.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        painter.text(
            pos2(link.min.x, link.center().y),
            Align2::LEFT_CENTER,
            caption,
            text(12.5),
            if link_hit.hovered() { p.accent } else { p.muted },
        );

        if let Some(report) = &batch.report {
            let colour = if report.contains("无法识别") {
                p.amber
            } else {
                p.muted
            };
            let mut lines: Vec<&str> = report.lines().collect();
            if lines.len() > 4 {
                lines.truncate(3);
                lines.push("…");
            }
            let galley = painter.layout(
                lines.join("\n"),
                text(12.0),
                colour,
                rect.width() - 60.0,
            );
            painter.galley(pos2(rect.min.x + 30.0, link_y + 16.0), galley, colour);
        }

        let import = Rect::from_min_size(
            pos2(rect.max.x - 30.0 - 104.0, rect.max.y - 56.0),
            vec2(104.0, 38.0),
        );
        let entered = !expanded
            && focused
            && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let clicked = primary_button(
            ui,
            painter,
            import,
            egui::Id::unique("batch-import"),
            "添加",
            13.0,
            full,
            p,
        );

        if link_hit.clicked() {
            batch.expanded = !expanded;
            if batch.expanded {
                if batch.text.trim().is_empty() {
                    batch.text = batch.single.clone();
                }
            } else {
                batch.single = batch
                    .text
                    .lines()
                    .map(str::trim)
                    .find(|line| !line.is_empty() && !line.starts_with('#'))
                    .unwrap_or_default()
                    .to_string();
            }
            batch.focus = true;
            batch.report = None;
            return;
        }

        if clicked || entered {
            let text = if expanded {
                batch.text.clone()
            } else {
                batch.single.clone()
            };
            let (added, known, bad) = self.import_repositories(&text);
            if added + known > 0 || bad == 0 {
                self.batch = None;
            }
            return;
        }

        if scrim.clicked() {
            let inside = ui
                .input(|i| i.pointer.interact_pos())
                .is_some_and(|at| rect.contains(at));
            if !inside {
                self.batch = None;
            }
        }
    }

    /// The version chooser: a panel in the middle of the window with the
    /// release list, and a button that installs whichever one is chosen.
    pub(crate) fn draw_picker(&mut self, ui: &mut Ui, full: Rect, p: &Palette) {
        let Some(picker) = self.picker.as_mut() else {
            return;
        };
        let idx = picker.idx;
        let name = repos()[idx].name;
        ui.interact(full, egui::Id::unique("picker-scrim"), Sense::click());
        ui.painter()
            .rect_filled(full, 0, Color32::from_black_alpha(140));

        let w = 560.0;
        let panel_h = 268.0;
        let rect = Rect::from_center_size(full.center(), vec2(w, panel_h));
        let painter = ui.painter().clone();
        let painter = &painter;
        painter.rect_filled(rect, radius(8), p.bg);
        painter.rect_stroke(rect, radius(8), line(p.border), StrokeKind::Middle);

        painter.text(
            pos2(rect.min.x + 28.0, rect.min.y + 34.0),
            Align2::LEFT_CENTER,
            format!("{name} · 选择版本"),
            display(22.0),
            p.text,
        );

        painter.text(
            pos2(rect.min.x + 28.0, rect.min.y + 74.0),
            Align2::LEFT_CENTER,
            "版本",
            text(13.0),
            p.muted,
        );

        let field = Rect::from_min_size(
            pos2(rect.min.x + 28.0, rect.min.y + 90.0),
            vec2(w - 56.0, 40.0),
        );
        painter.rect_filled(field, radius(6), p.search_fill);
        painter.rect_stroke(field, radius(6), line(p.border), StrokeKind::Middle);
        let caption = match picker.list.get(picker.chosen) {
            Some(rel) => {
                let tag = crate::version::clean_tag(&rel.tag).to_uppercase();
                if picker.chosen == 0 {
                    format!("V{tag}（最新发布）")
                } else {
                    format!("V{tag}")
                }
            }
            None if picker.loading => "正在获取版本…".to_string(),
            None => "没有可选版本".to_string(),
        };
        painter.text(
            pos2(field.min.x + 14.0, field.center().y),
            Align2::LEFT_CENTER,
            &caption,
            text(14.0),
            p.text,
        );
        caret_down(
            painter,
            Rect::from_center_size(
                pos2(field.max.x - 20.0, field.center().y),
                vec2(12.0, 12.0),
            ),
            p.muted,
        );
        let resp = ui.interact(field, egui::Id::unique("picker-field"), Sense::click());
        if resp.clicked() && !picker.list.is_empty() {
            picker.open = !picker.open;
        }
        if picker.open {
            let rows = picker.list.len().min(6);
            let list_h = rows as f32 * 36.0;
            let list = Rect::from_min_size(
                pos2(field.min.x, field.max.y + 2.0),
                vec2(field.width(), list_h),
            );
            painter.rect_filled(list, radius(6), p.surface);
            painter.rect_stroke(list, radius(6), line(p.border), StrokeKind::Middle);
            for (i, rel) in picker.list.iter().take(rows).enumerate() {
                let row = Rect::from_min_size(
                    pos2(list.min.x, list.min.y + i as f32 * 36.0),
                    vec2(list.width(), 36.0),
                );
                let hovered =
                    row.contains(ui.input(|i| i.pointer.hover_pos().unwrap_or_default()));
                if hovered {
                    painter.rect_filled(row, 0, p.border);
                }
                if i == picker.chosen {
                    painter.rect_filled(
                        Rect::from_min_size(row.min, vec2(3.0, row.height())),
                        0,
                        p.accent,
                    );
                }
                let tag = crate::version::clean_tag(&rel.tag).to_uppercase();
                let label = if i == 0 {
                    format!("V{tag}（最新发布）")
                } else {
                    format!("V{tag}")
                };
                painter.text(
                    pos2(row.min.x + 14.0, row.center().y),
                    Align2::LEFT_CENTER,
                    label,
                    text(13.5),
                    if rel.asset.is_some() { p.text } else { p.faint },
                );
                let hit = ui.interact(row, egui::Id::unique(("picker-row", i)), Sense::click());
                if hit.clicked() {
                    picker.chosen = i;
                    picker.open = false;
                }
            }
        }

        let hint_y = rect.min.y + 152.0;
        painter.text(
            pos2(rect.min.x + 28.0, hint_y),
            Align2::LEFT_CENTER,
            "旧版可能无法打开你已保存的文件；安装会替换对应安装目录下的同名应用。",
            text(12.5),
            p.faint,
        );
        if let Some(err) = &picker.error {
            painter.text(
                pos2(rect.min.x + 28.0, hint_y + 22.0),
                Align2::LEFT_CENTER,
                err,
                text(12.5),
                p.amber,
            );
        }

        let close = Rect::from_min_size(
            pos2(rect.min.x + 28.0, rect.max.y - 60.0),
            vec2(150.0, 40.0),
        );
        if action_button(
            ui,
            painter,
            close,
            egui::Id::unique("picker-close"),
            "关闭",
            13.5,
            full,
            p,
        ) {
            self.picker = None;
            return;
        }
        let can_install = picker
            .list
            .get(picker.chosen)
            .map(|r| r.asset.is_some())
            .unwrap_or(false);
        let install_label = match picker.list.get(picker.chosen) {
            Some(rel) => format!(
                "安装版本 V{}",
                crate::version::clean_tag(&rel.tag).to_uppercase()
            ),
            None => "安装版本".to_string(),
        };
        let install = Rect::from_min_max(
            pos2(rect.max.x - 28.0 - 220.0, rect.max.y - 60.0),
            pos2(rect.max.x - 28.0, rect.max.y - 20.0),
        );
        if can_install {
            if primary_button(
                ui,
                painter,
                install,
                egui::Id::unique("picker-install"),
                &install_label,
                13.5,
                full,
                p,
            ) {
                let rel = picker.list[picker.chosen].clone();
                if let Some(asset) = rel.asset.clone() {
                    self.picker = None;
                    self.start_download(idx, asset);
                }
            }
        } else {
            let faded = Rect::from_min_size(install.min, install.size());
            painter.rect_stroke(faded, radius(5), line(p.border), StrokeKind::Middle);
            painter.text(
                faded.center(),
                Align2::CENTER_CENTER,
                &install_label,
                text(13.5),
                p.faint,
            );
        }
    }

    /// The sort dropdown, drawn on top of the cards.
    pub(crate) fn draw_sort_dropdown(&mut self, ui: &mut Ui, sort_rect: Rect, p: &Palette) {
        let painter = ui.painter();
        let mut chosen = self.state.sort;
        let list_rect = Rect::from_min_max(
            pos2(sort_rect.min.x, sort_rect.max.y + 3.0),
            pos2(sort_rect.max.x, sort_rect.max.y + 3.0 + 56.0),
        );
        if let Some(pointer) = ui.input(|i| i.pointer.hover_pos()) {
            if ui.input(|i| i.pointer.primary_clicked())
                && !list_rect.contains(pointer)
                && !sort_rect.contains(pointer)
            {
                self.sort_open = false;
                return;
            }
        }
        painter.rect_filled(list_rect, radius(6), p.surface);
        painter.rect_stroke(list_rect, radius(6), line(p.border), StrokeKind::Middle);
        for (i, option) in [SortBy::Name, SortBy::Updated].iter().enumerate() {
            let row = Rect::from_min_max(
                pos2(list_rect.min.x, list_rect.min.y + i as f32 * 28.0),
                pos2(list_rect.max.x, list_rect.min.y + (i as f32 + 1.0) * 28.0),
            );
            let resp = ui.interact(row, ui.scope_id().with(("sort", i)), Sense::click());
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                painter.rect_filled(row.shrink(2.0), radius(4), p.border);
            }
            if resp.clicked() {
                chosen = *option;
                self.sort_open = false;
            }
            painter.text(
                pos2(row.min.x + 10.0, row.center().y),
                Align2::LEFT_CENTER,
                sort_option_label(*option),
                text(12.5),
                if chosen == *option { p.accent } else { p.text },
            );
        }
        self.state.sort = chosen;
    }
}
