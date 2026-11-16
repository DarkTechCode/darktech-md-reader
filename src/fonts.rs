//! Встроенные шрифты (все — вариативные, с кириллицей, лицензии OFL в assets/fonts).
//! Bold реализуется координатой `wght` вариативного шрифта, курсив egui синтезирует сам.

use eframe::egui::{text::FontData, Context, FontFamily, FontInsert, FontPriority, InsertFontFamily};

/// Семейство основного текста документа (Noto Serif).
pub const SERIF: &str = "Serif";
/// Семейство sans (Inter); оно же — интерфейсное (Proportional).
pub const SANS: &str = "Sans";
/// Семейство mono (JetBrains Mono); оно же — код в UI (Monospace).
pub const MONO: &str = "Mono";

/// Устанавливает встроенные шрифты в контекст egui (вызывается один раз при старте).
pub fn install(ctx: &Context) {
    let hi = |family: FontFamily| InsertFontFamily {
        family,
        priority: FontPriority::Highest,
    };

    let fonts = [
        (
            "Inter",
            include_bytes!("../assets/fonts/InterVariable.ttf") as &'static [u8],
            vec![
                hi(FontFamily::Proportional),
                hi(FontFamily::Name(SANS.into())),
            ],
        ),
        (
            "NotoSerif",
            include_bytes!("../assets/fonts/NotoSerif-VF.ttf"),
            vec![hi(FontFamily::Name(SERIF.into()))],
        ),
        (
            "JetBrainsMono",
            include_bytes!("../assets/fonts/JetBrainsMono-VF.ttf"),
            vec![
                hi(FontFamily::Monospace),
                hi(FontFamily::Name(MONO.into())),
            ],
        ),
    ];

    for (name, bytes, families) in fonts {
        ctx.add_font(FontInsert::new(
            name,
            FontData::from_static(bytes),
            families,
        ));
    }
}
