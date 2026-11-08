//! The fonts the launcher draws with.
//!
//! Archivo for the wordmark and the app names, Inter for ordinary text and
//! Geist Mono for the versions and the small captions — the same three
//! families the original uses, all published under the SIL Open Font Licence.
//!
//! Characters those faces do not carry — Chinese, for instance — are filled in
//! by the system's own font through egui's system font provider, which is
//! exactly what the original does.

use egui::text::{FontData, FontDefinitions, FontFamily};
use egui::Context;

const ARCHIVO: &[u8] = include_bytes!("../assets/fonts/Archivo-Variable.ttf");
const INTER: &[u8] = include_bytes!("../assets/fonts/Inter-Variable.ttf");
const MONO: &[u8] = include_bytes!("../assets/fonts/GeistMono-Variable.ttf");

/// The family names the rest of the code asks for.
pub const INTER_FAMILY: &str = "inter";
pub const ARCHIVO_FAMILY: &str = "archivo";
pub const MONO_FAMILY: &str = "geist-mono";

/// Install every face and wire up the families.
pub fn install(ctx: &Context) {
    let mut fonts = FontDefinitions::default();

    fonts.font_data.insert(
        "archivo".to_string(),
        std::sync::Arc::new(FontData::from_static(ARCHIVO)),
    );
    fonts.font_data.insert(
        "archivo-bold".to_string(),
        std::sync::Arc::new(FontData::from_static(ARCHIVO).tweak(weight(700.0))),
    );
    fonts.font_data.insert(
        "inter".to_string(),
        std::sync::Arc::new(FontData::from_static(INTER)),
    );
    fonts.font_data.insert(
        "geist-mono".to_string(),
        std::sync::Arc::new(FontData::from_static(MONO)),
    );
    fonts.font_data.insert(
        "geist-mono-bold".to_string(),
        std::sync::Arc::new(FontData::from_static(MONO).tweak(weight(600.0))),
    );

    // The chain for each family. Anything these faces do not carry — Chinese,
    // for instance — is left to the system's own font, which egui asks for
    // when a character is missing.
    let chain = |names: &[&str]| -> Vec<String> { names.iter().map(|s| s.to_string()).collect() };

    fonts.families.insert(
        FontFamily::Proportional,
        chain(&["inter"]),
    );
    fonts.families.insert(
        FontFamily::Monospace,
        chain(&["geist-mono"]),
    );
    fonts.families.insert(
        FontFamily::Name(INTER_FAMILY.into()),
        chain(&["inter"]),
    );
    fonts.families.insert(
        FontFamily::Name(ARCHIVO_FAMILY.into()),
        chain(&["archivo"]),
    );
    fonts.families.insert(
        FontFamily::Name("archivo-bold".into()),
        chain(&["archivo-bold"]),
    );
    fonts.families.insert(
        FontFamily::Name(MONO_FAMILY.into()),
        chain(&["geist-mono"]),
    );
    fonts.families.insert(
        FontFamily::Name("geist-mono-bold".into()),
        chain(&["geist-mono-bold"]),
    );

    ctx.set_fonts(fonts);

    // The Mac's own fonts fill in the characters the three faces above do not
    // carry, which is how the Chinese text gets drawn.
    ctx.add_font_provider(std::sync::Arc::new(
        egui_system_font_provider::SystemFontProvider::new(),
    ));
}

/// A tweak that picks one weight of a variable font.
fn weight(w: f32) -> egui::FontTweak {
    let mut tweak = egui::FontTweak::default();
    tweak.coords = egui::epaint::text::VariationCoords::new([(b"wght", w)]);
    tweak
}
