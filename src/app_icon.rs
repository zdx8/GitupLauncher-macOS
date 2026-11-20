//! App icon procedural renderer and PNG writer using the `png` crate.

use eframe::egui::{self, Color32, Vec2};

/// Draws the launcher's icon: a white rounded plate with the head of a
/// bird in profile on it — white, black-outlined, hatched like an
/// engraving, with a black eye and a white glint. Every edge is smoothed
/// with 4x4 supersampling.
pub fn render_icon(size: usize) -> egui::ColorImage {
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
    let head: [(f32, f32); 13] = [
        (0.130, 0.905),
        (0.118, 0.330),
        (0.190, 0.140),
        (0.320, 0.058),
        (0.480, 0.072),
        (0.600, 0.162),
        (0.655, 0.278),
        (0.905, 0.408),
        (0.658, 0.472),
        (0.612, 0.560),
        (0.700, 0.678),
        (0.636, 0.772),
        (0.652, 0.905),
    ];
    let shrink = |p: (f32, f32)| -> (f32, f32) {
        (0.5 + (p.0 - 0.5) * 0.80, 0.5 + (p.1 - 0.5) * 0.80)
    };
    let head = head.map(shrink);
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
    let plate_sd = |x: f32, y: f32| -> f32 {
        let (hw, hh, r) = (0.455, 0.455, 0.225);
        let qx = (x - 0.5).abs() - (hw - r);
        let qy = (y - 0.5).abs() - (hh - r);
        let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
        outside + qx.max(qy).min(0.0) - r
    };
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

/// Writes an `egui::ColorImage` as a compressed RGBA PNG atomically.
pub fn write_png(path: &std::path::Path, image: &egui::ColorImage) {
    let (w, h) = (image.width() as u32, image.height() as u32);
    let mut raw = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let c = image[(x as usize, y as usize)];
            let (r, g, b, a) = (c.r(), c.g(), c.b(), c.a());
            let un = |v: u8| {
                if a == 0 {
                    0
                } else {
                    ((v as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8
                }
            };
            raw.extend_from_slice(&[un(r), un(g), un(b), a]);
        }
    }
    let mut png_bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png_bytes, w, h);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let Ok(mut writer) = encoder.write_header() else {
            return;
        };
        if writer.write_image_data(&raw).is_err() {
            return;
        }
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let temp = path.with_extension("png.part");
    if std::fs::write(&temp, &png_bytes).is_ok() {
        let _ = std::fs::rename(&temp, path);
        return;
    }
    let _ = std::fs::write(path, png_bytes);
}
