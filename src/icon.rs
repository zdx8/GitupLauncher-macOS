//! Turning an app's icon into pixels for a texture.
//!
//! sips hands us a PNG; this makes it into straight RGBA bytes that egui
//! can wrap in a `ColorImage`, whatever colour layout the PNG happened to
//! use.

/// Decode a PNG into (width, height, straight RGBA bytes).
pub fn decode_png_rgba(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    // sips writes ordinary 8-bit PNGs, so the decoder's defaults are
    // fine; the colour layout it reports is normalised below.
    let decoder = png::Decoder::new(bytes);
    let mut reader = decoder
        .read_info()
        .map_err(|e| format!("图标无法解码: {e}"))?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|e| format!("{e}"))?;
    let (w, h) = (info.width, info.height);
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf
            .chunks_exact(3)
            .flat_map(|c| [c[0], c[1], c[2], 255])
            .collect(),
        png::ColorType::Grayscale => buf.into_iter().flat_map(|g| [g, g, g, 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf
            .chunks_exact(2)
            .flat_map(|c| [c[0], c[0], c[0], c[1]])
            .collect(),
        other => return Err(format!("不支持的图标颜色布局: {other:?}")),
    };
    Ok((w, h, rgba))
}

/// The part of a decoded icon that actually carries artwork, as texture
/// coordinates (u0, v0, u1, v1). A Mac icon is a rounded square floating
/// inside transparent padding; stretching just this part makes the
/// artwork fill the whole tile instead of sitting small in the middle.
pub fn artwork_uv(width: u32, height: u32, rgba: &[u8]) -> (f32, f32, f32, f32) {
    let (mut x0, mut y0, mut x1, mut y1) = (width, height, 0u32, 0u32);
    let mut seen = false;
    for y in 0..height {
        for x in 0..width {
            let alpha = rgba[(y * width + x) as usize * 4 + 3];
            if alpha > 16 {
                seen = true;
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x + 1);
                y1 = y1.max(y + 1);
            }
        }
    }
    if !seen {
        return (0.0, 0.0, 1.0, 1.0);
    }
    let as_uv = |v: u32, m: u32| v as f32 / m as f32;
    (
        as_uv(x0, width),
        as_uv(y0, height),
        as_uv(x1, width),
        as_uv(y1, height),
    )
}
