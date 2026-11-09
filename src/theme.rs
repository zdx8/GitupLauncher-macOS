//! Colours, type and the small vector drawings the launcher needs.
//!
//! The palette was read straight off the reference picture of the original:
//! a near-black page (#121316), hairline borders (#333436) and one accent
//! blue used for versions, badges and links.

use egui::{Color32, CornerRadius, FontId, Stroke};

/// One set of colours, so the same drawing code works in both appearances.
#[derive(Clone, Copy)]
pub struct Palette {
    /// The page behind everything.
    pub bg: Color32,
    /// A slightly raised surface.
    pub surface: Color32,
    /// A line between things.
    pub border: Color32,
    /// The primary text.
    pub text: Color32,
    /// Secondary text.
    pub muted: Color32,
    /// Tertiary text: the "排序" label, the placeholder.
    pub faint: Color32,
    /// The accent used for versions, badges and links.
    pub accent: Color32,
    /// A warm accent, for a version that is out of date.
    pub amber: Color32,
    /// The inside of the search box, darker than the page.
    pub search_fill: Color32,
}

pub const DARK: Palette = Palette {
    bg: Color32::from_rgb(0x12, 0x13, 0x16),
    surface: Color32::from_rgb(0x0B, 0x0C, 0x0E),
    border: Color32::from_rgb(0x33, 0x34, 0x36),
    text: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    muted: Color32::from_rgb(0x98, 0x99, 0x98),
    faint: Color32::from_rgb(0x67, 0x68, 0x69),
    accent: Color32::from_rgb(0x74, 0xAA, 0xFF),
    amber: Color32::from_rgb(0xFF, 0xC1, 0x4D),
    search_fill: Color32::from_rgb(0x06, 0x06, 0x07),
};

pub const LIGHT: Palette = Palette {
    bg: Color32::from_rgb(0xFB, 0xFB, 0xFB),
    surface: Color32::from_rgb(0xF1, 0xF1, 0xF2),
    border: Color32::from_rgb(0xD6, 0xD6, 0xD8),
    text: Color32::from_rgb(0x14, 0x14, 0x16),
    muted: Color32::from_rgb(0x64, 0x64, 0x66),
    faint: Color32::from_rgb(0x8E, 0x8E, 0x90),
    accent: Color32::from_rgb(0x1B, 0x63, 0xD8),
    amber: Color32::from_rgb(0xB0, 0x76, 0x00),
    search_fill: Color32::from_rgb(0xF1, 0xF1, 0xF2),
};

/// The palette that matches the current system appearance.
pub fn palette(dark: bool) -> Palette {
    if dark {
        DARK
    } else {
        LIGHT
    }
}

/// A rounded corner.
pub fn radius(r: u8) -> CornerRadius {
    CornerRadius::same(r)
}

// ---------------------------------------------------------------- the type

/// The wordmark and the app names.
pub fn display(size: f32) -> FontId {
    FontId::new(size, egui::FontFamily::Name("archivo-bold".into()))
}

/// Ordinary text: the descriptions and the dates.
pub fn text(size: f32) -> FontId {
    FontId::new(size, egui::FontFamily::Name("inter".into()))
}

/// Labels, versions and buttons.
pub fn label(size: f32) -> FontId {
    FontId::new(size, egui::FontFamily::Name("geist-mono".into()))
}

// ------------------------------------------------------------ little drawings

/// An arrow pointing up and to the right, for the small link marks.
pub fn arrow_out(painter: &egui::Painter, rect: egui::Rect, color: Color32) {
    let w = rect.width().min(rect.height());
    let c = rect.center();
    let s = Stroke::new((w * 0.16).max(1.0), color);
    painter.line_segment(
        [egui::pos2(c.x - w * 0.16, c.y + w * 0.16), egui::pos2(c.x + w * 0.18, c.y - w * 0.18)],
        s,
    );
    painter.line_segment(
        [egui::pos2(c.x - w * 0.02, c.y - w * 0.18), egui::pos2(c.x + w * 0.18, c.y - w * 0.18)],
        s,
    );
    painter.line_segment(
        [egui::pos2(c.x + w * 0.18, c.y - w * 0.18), egui::pos2(c.x + w * 0.18, c.y + w * 0.02)],
        s,
    );
}

/// A cog, for the settings button.
pub fn gear_icon(painter: &egui::Painter, rect: egui::Rect, color: Color32) {
    let w = rect.width().min(rect.height());
    let c = rect.center();
    let s = Stroke::new((w * 0.09).max(1.0), color);
    let r = w * 0.26;
    painter.circle_stroke(c, r * 0.58, s);
    for i in 0..8 {
        let a = i as f32 * std::f32::consts::TAU / 8.0;
        let (dx, dy) = (a.cos(), a.sin());
        painter.line_segment(
            [
                egui::pos2(c.x + dx * r * 0.80, c.y + dy * r * 0.80),
                egui::pos2(c.x + dx * r * 1.24, c.y + dy * r * 1.24),
            ],
            s,
        );
    }
}

/// A small filled triangle, for the sort dropdown.
pub fn caret_down(painter: &egui::Painter, rect: egui::Rect, color: Color32) {
    let w = rect.width().min(rect.height());
    let c = rect.center();
    // The picture's pointer is about 10 pt wide and 8 pt tall.
    let half = w * 0.45;
    let up = w * 0.34;
    painter.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(c.x - half, c.y - up),
            egui::pos2(c.x + half, c.y - up),
            egui::pos2(c.x, c.y + up * 1.15),
        ],
        color,
        Stroke::NONE,
    ));
}

/// Four squares, for the grid view.
pub fn grid_icon(painter: &egui::Painter, rect: egui::Rect, color: Color32) {
    let w = rect.width().min(rect.height());
    let c = rect.center();
    // Four squares filling about three quarters of the box.
    let s = w * 0.30;
    let g = w * 0.10;
    let step = s + g;
    for ix in 0..2 {
        for iy in 0..2 {
            let x = c.x - step * 0.5 + ix as f32 * step;
            let y = c.y - step * 0.5 + iy as f32 * step;
            painter.rect_filled(
                egui::Rect::from_center_size(egui::pos2(x, y), egui::vec2(s, s)),
                1,
                color,
            );
        }
    }
}

/// Three short bars, for the list view.
pub fn list_icon(painter: &egui::Painter, rect: egui::Rect, color: Color32) {
    let w = rect.width().min(rect.height());
    let c = rect.center();
    // Three bars, as wide as the mark and heavy enough to read small.
    let bar_w = w * 0.68;
    let bar_h = (w * 0.13).max(1.6);
    let gap = w * 0.17;
    for i in 0..3 {
        let y = c.y - gap + i as f32 * gap;
        painter.rect_filled(
            egui::Rect::from_min_size(
                egui::pos2(c.x - bar_w * 0.5, y - bar_h * 0.5),
                egui::vec2(bar_w, bar_h),
            ),
            1,
            color,
        );
    }
}


/// The little marks in the footer: a globe and seven simple social shapes,
/// drawn in the spirit of the original's row without copying its logos.


/// The wordmark's first letter: a solid "G" whose crossbar runs on into a
/// short second leg, like the branch of a repository. Drawn from scratch
/// rather than copied from anyone's lettering.
pub fn draw_g_mark(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let w = rect.width();
    let h = rect.height();
    let t = w * 0.26;
    let mid_y = rect.center().y;
    let r = rect.right();
    let l = rect.left();
    let top = rect.top();
    let bottom = rect.bottom();
    // Top edge, bottom edge and left edge: an open square bracket.
    painter.rect_filled(
        egui::Rect::from_min_max(rect.min, egui::pos2(r - t * 0.3, top + t)),
        0,
        color,
    );
    painter.rect_filled(
        egui::Rect::from_min_max(egui::pos2(l, bottom - t), egui::pos2(r - t * 0.3, bottom)),
        0,
        color,
    );
    painter.rect_filled(
        egui::Rect::from_min_max(rect.min, egui::pos2(l + t, bottom)),
        0,
        color,
    );
    // The right edge, with the little gap that keeps the bowl open.
    painter.rect_filled(
        egui::Rect::from_min_max(egui::pos2(r - t, top), egui::pos2(r, mid_y - h * 0.02)),
        0,
        color,
    );
    painter.rect_filled(
        egui::Rect::from_min_max(egui::pos2(r - t, mid_y + h * 0.16), egui::pos2(r, bottom)),
        0,
        color,
    );
    // The crossbar, running in from the right.
    painter.rect_filled(
        egui::Rect::from_min_max(
            egui::pos2(rect.center().x, mid_y),
            egui::pos2(r, mid_y + h * 0.18),
        ),
        0,
        color,
    );
    // The branch: a short leg dropping from the crossbar.
    painter.rect_filled(
        egui::Rect::from_min_max(
            egui::pos2(r - t * 1.7, mid_y + h * 0.18),
            egui::pos2(r - t * 0.7, bottom),
        ),
        0,
        color,
    );
}


/// A circular arrow, for the check-updates button at the far left of the
/// toolbar: a ring open at one side and an arrowhead running into the gap.
pub fn refresh_icon(painter: &egui::Painter, rect: egui::Rect, color: Color32) {
    let w = rect.width().min(rect.height());
    let c = rect.center();
    let s = Stroke::new((w * 0.13).max(1.2), color);
    let r = w * 0.34;
    let tau = std::f32::consts::TAU;
    // The ring, as an arc running most of the way round.
    let mut pts = Vec::new();
    for i in 0..=24 {
        let a = (50.0 + 280.0 * i as f32 / 24.0) * tau / 360.0;
        pts.push(egui::pos2(c.x + a.cos() * r, c.y + a.sin() * r));
    }
    painter.add(egui::Shape::line(pts, s));
    // The arrowhead where the arc begins, pointing along it.
    let a = 50.0 * tau / 360.0;
    let (dx, dy) = (a.cos(), a.sin());
    let e = egui::pos2(c.x + dx * r, c.y + dy * r);
    let (tx, ty) = (-dy, dx);
    let tip = egui::pos2(e.x - tx * w * 0.12, e.y - ty * w * 0.12);
    let b1 = egui::pos2(e.x + tx * w * 0.02 - dx * w * 0.10, e.y + ty * w * 0.02 - dy * w * 0.10);
    let b2 = egui::pos2(e.x + tx * w * 0.02 + dx * w * 0.10, e.y + ty * w * 0.02 + dy * w * 0.10);
    painter.add(egui::Shape::convex_polygon(vec![tip, b1, b2], color, Stroke::NONE));
}
