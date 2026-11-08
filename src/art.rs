//! The marks inside each app's tile.
//!
//! They are drawn from scratch, in the spirit of the engraved heads the
//! original uses — a pale face filling the tile, shaded with fine dark
//! hatching — but none of them copies anyone's artwork. Every mark is built
//! from a few filled shapes and a handful of short lines, so it stays crisp
//! at any size.

use crate::catalog::Glyph;
use egui::{pos2, Color32, Painter, Pos2, Rect, Shape, Stroke};

const INK: Color32 = Color32::from_rgb(0x22, 0x22, 0x24);
const PAPER: Color32 = Color32::from_rgb(0xF4, 0xF3, 0xEF);

/// Where a point inside the unit square lands on the tile.
struct Frame {
    rect: Rect,
    size: f32,
}

impl Frame {
    fn new(rect: Rect) -> Self {
        Self { rect, size: rect.width() }
    }
    /// x and y run 0..1 from the tile's left and top.
    fn p(&self, x: f32, y: f32) -> Pos2 {
        pos2(self.rect.min.x + x * self.size, self.rect.min.y + y * self.size)
    }
    fn poly(&self, painter: &Painter, pts: &[(f32, f32)], color: Color32) {
        painter.add(Shape::convex_polygon(
            pts.iter().map(|&(x, y)| self.p(x, y)).collect(),
            color,
            Stroke::NONE,
        ));
    }
    fn line(&self, painter: &Painter, a: (f32, f32), b: (f32, f32), w: f32, color: Color32) {
        painter.line_segment(
            [self.p(a.0, a.1), self.p(b.0, b.1)],
            Stroke::new(w * self.size, color),
        );
    }
    /// A closed outline around a list of points.
    fn outline(&self, painter: &Painter, pts: &[(f32, f32)], w: f32, color: Color32) {
        let mut v: Vec<Pos2> = pts.iter().map(|&(x, y)| self.p(x, y)).collect();
        v.push(v[0]);
        painter.add(Shape::line(v, Stroke::new(w * self.size, color)));
    }
    fn disc(&self, painter: &Painter, x: f32, y: f32, r: f32, color: Color32) {
        painter.circle_filled(self.p(x, y), r * self.size, color);
    }
    /// A short line of hatching, leaning the way `slant` says.
    fn hatch(&self, painter: &Painter, x: f32, y: f32, len: f32, slant: f32, w: f32, color: Color32) {
        self.line(painter, (x, y), (x + slant * len, y + len), w, color);
    }
}

pub fn draw(painter: &Painter, tile: Rect, glyph: Glyph) {
    let f = Frame::new(tile);
    match glyph {
        Glyph::Bird => raven(&f, painter),
        Glyph::Cat => cat(&f, painter),
        Glyph::Flame => wolf(&f, painter),
        Glyph::Song => songbird(&f, painter),
    }
}

/// A raven's head in profile, facing right: a heavy crown, a long beak and a
/// ruffled neck, shaded with fine lines.
fn raven(f: &Frame, p: &Painter) {
    let head = [
        (0.02, 1.00), (0.02, 0.34), (0.10, 0.13), (0.26, 0.05), (0.46, 0.09),
        (0.60, 0.23), (0.66, 0.36), (0.94, 0.42), (0.66, 0.52), (0.62, 0.64),
        (0.70, 0.78), (0.58, 0.86), (0.60, 1.00),
    ];
    f.poly(p, &head, PAPER);
    f.outline(p, &head, 0.02, INK);
    // The eye, dark with a pale glint.
    f.disc(p, 0.40, 0.27, 0.062, INK);
    f.disc(p, 0.42, 0.25, 0.016, PAPER);
    // The beak, split by a dark line.
    f.line(p, (0.64, 0.36), (0.93, 0.415), 0.018, INK);
    // The brow and the crown.
    f.line(p, (0.10, 0.33), (0.16, 0.14), 0.018, INK);
    f.line(p, (0.16, 0.14), (0.30, 0.10), 0.018, INK);
    // Feathers: fine dark hatching over the cheek and neck.
    for (x, y, l) in [
        (0.18, 0.44, 0.07), (0.27, 0.48, 0.08), (0.15, 0.56, 0.08),
        (0.25, 0.60, 0.09), (0.34, 0.55, 0.07), (0.20, 0.68, 0.09),
        (0.30, 0.72, 0.09), (0.13, 0.78, 0.08), (0.40, 0.42, 0.05),
        (0.43, 0.60, 0.07), (0.36, 0.82, 0.08), (0.24, 0.88, 0.08),
        (0.46, 0.72, 0.07), (0.52, 0.56, 0.05), (0.10, 0.90, 0.08),
        (0.50, 0.90, 0.06), (0.32, 0.96, 0.06),
    ] {
        f.hatch(p, x, y, l, 0.4, 0.013, INK);
    }
    // The ruffled edge of the neck.
    f.line(p, (0.64, 0.62), (0.73, 0.76), 0.016, INK);
    f.line(p, (0.73, 0.76), (0.60, 0.86), 0.016, INK);
}

/// A lynx-like cat's face, front on: pointed ears, a broad muzzle and long
/// whiskers.
fn cat(f: &Frame, p: &Painter) {
    let face = [
        (0.10, 0.12), (0.30, 0.30), (0.70, 0.30), (0.90, 0.12), (0.96, 0.60),
        (0.78, 0.86), (0.50, 1.00), (0.22, 0.86), (0.04, 0.60),
    ];
    f.poly(p, &face, PAPER);
    f.outline(p, &face, 0.022, INK);
    // The inner ears.
    f.poly(p, &[(0.16, 0.22), (0.18, 0.40), (0.28, 0.30)], INK);
    f.poly(p, &[(0.84, 0.22), (0.82, 0.40), (0.72, 0.30)], INK);
    // Eyes, dark and narrowed.
    f.poly(p, &[(0.24, 0.58), (0.38, 0.55), (0.36, 0.63), (0.25, 0.65)], INK);
    f.poly(p, &[(0.76, 0.58), (0.62, 0.55), (0.64, 0.63), (0.75, 0.65)], INK);
    // The nose and the split lip.
    f.poly(p, &[(0.50, 0.68), (0.45, 0.74), (0.55, 0.74)], INK);
    f.line(p, (0.50, 0.74), (0.50, 0.83), 0.014, INK);
    // Fur: short strokes on the cheeks and brow.
    for (x, y, l) in [
        (0.20, 0.44, 0.05), (0.28, 0.36, 0.05), (0.36, 0.44, 0.05),
        (0.70, 0.42, 0.05), (0.62, 0.36, 0.05), (0.78, 0.46, 0.05),
        (0.22, 0.72, 0.06), (0.28, 0.78, 0.06), (0.34, 0.72, 0.05),
        (0.72, 0.72, 0.06), (0.66, 0.78, 0.06), (0.60, 0.72, 0.05),
        (0.44, 0.88, 0.05), (0.54, 0.88, 0.05), (0.50, 0.44, 0.04),
    ] {
        f.hatch(p, x, y, l, 0.4, 0.012, INK);
    }
    // Whiskers, running off both sides.
    for (y, d) in [(0.68, -0.04), (0.75, 0.0), (0.82, 0.04)] {
        f.line(p, (0.42, y), (0.02, y + d), 0.011, INK);
        f.line(p, (0.58, y), (0.98, y + d), 0.011, INK);
    }
}

/// A wolf's head in profile, facing right, with a pointed ear and a long
/// muzzle — the mark for the image editor.
fn wolf(f: &Frame, p: &Painter) {
    let head = [
        (0.04, 0.96), (0.04, 0.30), (0.16, 0.04), (0.30, 0.30), (0.46, 0.24),
        (0.62, 0.30), (0.94, 0.44), (0.66, 0.54), (0.60, 0.66), (0.66, 0.74),
        (0.56, 0.80), (0.58, 0.96),
    ];
    f.poly(p, &head, PAPER);
    f.outline(p, &head, 0.022, INK);
    // The ear, folded dark.
    f.poly(p, &[(0.15, 0.26), (0.17, 0.08), (0.26, 0.29)], INK);
    // A dark eye.
    f.poly(p, &[(0.40, 0.40), (0.48, 0.38), (0.48, 0.44), (0.40, 0.45)], INK);
    // The nose at the end of the muzzle.
    f.disc(p, 0.89, 0.45, 0.035, INK);
    // The mouth line and the cheek.
    f.line(p, (0.66, 0.56), (0.88, 0.48), 0.014, INK);
    f.line(p, (0.50, 0.44), (0.62, 0.52), 0.012, INK);
    // The fur, etched in fine dark lines.
    for (x, y, l) in [
        (0.20, 0.42, 0.08), (0.28, 0.48, 0.09), (0.36, 0.54, 0.09),
        (0.24, 0.60, 0.09), (0.32, 0.66, 0.09), (0.22, 0.74, 0.09),
        (0.42, 0.64, 0.08), (0.38, 0.78, 0.09), (0.48, 0.72, 0.07),
        (0.28, 0.86, 0.08), (0.44, 0.86, 0.07), (0.14, 0.50, 0.07),
        (0.14, 0.82, 0.07), (0.52, 0.88, 0.06), (0.10, 0.36, 0.05),
    ] {
        f.hatch(p, x, y, l, 0.35, 0.013, INK);
    }
}

/// A small songbird in profile with an open beak, for the audio app.
fn songbird(f: &Frame, p: &Painter) {
    // The body: a rounded head flowing into a chest.
    let body = [
        (0.14, 0.40), (0.26, 0.22), (0.44, 0.18), (0.60, 0.28), (0.66, 0.44),
        (0.72, 0.56), (0.70, 0.78), (0.62, 1.00), (0.10, 1.00), (0.08, 0.62),
    ];
    f.poly(p, &body, PAPER);
    f.outline(p, &body, 0.022, INK);
    // The crest.
    f.poly(p, &[(0.28, 0.24), (0.38, 0.04), (0.46, 0.22)], INK);
    // The open beak.
    f.poly(p, &[(0.66, 0.40), (0.98, 0.34), (0.68, 0.48)], PAPER);
    f.poly(p, &[(0.66, 0.52), (0.98, 0.48), (0.68, 0.60)], PAPER);
    f.outline(p, &[(0.66, 0.40), (0.98, 0.34), (0.68, 0.48)], 0.014, INK);
    f.outline(p, &[(0.66, 0.52), (0.98, 0.48), (0.68, 0.60)], 0.014, INK);
    // The eye.
    f.disc(p, 0.50, 0.40, 0.060, INK);
    f.disc(p, 0.52, 0.38, 0.016, PAPER);
    // Feathers across the breast and wing.
    for (x, y, l) in [
        (0.20, 0.66, 0.07), (0.30, 0.72, 0.07), (0.40, 0.68, 0.06),
        (0.24, 0.80, 0.07), (0.36, 0.84, 0.07), (0.46, 0.76, 0.06),
        (0.18, 0.92, 0.07), (0.32, 0.94, 0.07), (0.46, 0.90, 0.06),
        (0.16, 0.52, 0.06), (0.26, 0.56, 0.06), (0.36, 0.54, 0.05),
        (0.54, 0.62, 0.05), (0.56, 0.78, 0.06), (0.54, 0.94, 0.05),
    ] {
        f.hatch(p, x, y, l, 0.4, 0.012, INK);
    }
    // Three notes rising beside the beak.
    for (i, (x, y, r)) in [(0.80, 0.18, 0.05), (0.90, 0.08, 0.04), (0.98, 0.00, 0.032)]
        .into_iter()
        .enumerate()
    {
        f.disc(p, x, y, r, INK);
        f.line(p, (x + r * 0.9, y), (x + r * 0.9, y - 0.12 - i as f32 * 0.01), 0.016, INK);
    }
}

