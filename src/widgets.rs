//! Shared drawing helpers and small utilities used by the UI modules.

use crate::state;
use crate::state::SortBy;
use crate::theme::*;
use crate::{CARD_GAP, CARD_MIN_W};
use eframe::egui::{self, Align2, Color32, Rect, Sense, Stroke, StrokeKind, Ui};

/// The Chinese half of a description that carries two languages, e.g.
/// "中文简介 / An English summary". Anything else is left whole.
pub(crate) fn first_language(text: &str) -> &str {
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
pub(crate) fn grid_columns(avail_w: f32) -> usize {
    ((avail_w + CARD_GAP) / (CARD_MIN_W + CARD_GAP))
        .floor()
        .max(2.0) as usize
}

/// Clamps wrapped text to at most two lines, appending an ellipsis when truncated.
pub(crate) fn clamp_two_lines(ui: &Ui, text_in: &str, font: egui::FontId, wrap_w: f32) -> String {
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
pub(crate) fn human_speed(bytes_per_sec: f32) -> String {
    if bytes_per_sec >= 1_048_576.0 {
        format!("{:.1} MB/s", bytes_per_sec / 1_048_576.0)
    } else if bytes_per_sec >= 1024.0 {
        format!("{:.0} KB/s", bytes_per_sec / 1024.0)
    } else {
        format!("{:.0} B/s", bytes_per_sec)
    }
}

/// A file size, for the progress the footer shows.
pub(crate) fn human_size(bytes: u64) -> String {
    let m = bytes as f32 / 1_048_576.0;
    if m >= 1.0 {
        format!("{m:.1} MB")
    } else {
        format!("{:.0} KB", bytes as f32 / 1024.0)
    }
}

/// The Mac's clipboard as text, read straight from `pbpaste` so pasting
/// works even if a keyboard shortcut never reaches the field.
pub(crate) fn clipboard_text() -> Option<String> {
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
pub(crate) fn set_movable_by_background(movable: bool) -> bool {
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
pub(crate) fn set_movable_by_background(_movable: bool) -> bool {
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

