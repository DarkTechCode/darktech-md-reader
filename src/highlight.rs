//! Подсветка синтаксиса в блоках кода (syntect). Загружается лениво
//! (прогрев фоновым потоком при старте), красится только видимый срез строк.
//! Цвета — из готовых тем syntect (тёмная/светлая), результата хватает как есть.

use std::sync::OnceLock;

use syntect::easy::HighlightLines;
use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet};

use eframe::egui::Color32;

/// Готовые куски текста с цветами (для сборки LayoutJob в render/code).
#[derive(Clone)]
pub struct Highlighted(pub Vec<(String, Color32)>);

static SYNTAXES: OnceLock<SyntaxSet> = OnceLock::new();
static DARK: OnceLock<Theme> = OnceLock::new();
static LIGHT: OnceLock<Theme> = OnceLock::new();

/// Прогрев: грузим наборы в фоновом потоке при старте (однократный ~50 мс).
pub fn warm_up() {
    std::thread::spawn(|| {
        let _ = syntax_set();
        let _ = dark_theme();
        let _ = light_theme();
    });
}

fn syntax_set() -> &'static SyntaxSet {
    SYNTAXES.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn dark_theme() -> &'static Theme {
    DARK.get_or_init(|| {
        let ts = ThemeSet::load_defaults();
        ts.themes["base16-ocean.dark"].clone()
    })
}

fn light_theme() -> &'static Theme {
    LIGHT.get_or_init(|| {
        let ts = ThemeSet::load_defaults();
        ts.themes["InspiredGitHub"].clone()
    })
}

/// Синтаксис по языку fence (rust, js, json, bash…) или None.
fn syntax_for(lang: &str) -> Option<&'static SyntaxReference> {
    let ss = syntax_set();
    let l = lang.trim().to_ascii_lowercase();
    if l.is_empty() {
        return None;
    }
    ss.find_syntax_by_token(&l)
        .or_else(|| ss.find_syntax_by_extension(&l))
}

/// Красит срез кода (целые строки). None — язык неизвестен или подсветка
/// не подгрузилась. Итерации по строкам с переносом состояния (многострочные
/// комментарии/строки внутри среза подсвечиваются корректно).
pub fn highlight(slice: &str, lang: &str, dark: bool) -> Option<Highlighted> {
    let syntax = syntax_for(lang)?;
    let theme = if dark { dark_theme() } else { light_theme() };
    let ss = syntax_set();
    let mut hl = HighlightLines::new(syntax, theme);
    let mut out: Vec<(String, Color32)> = Vec::new();
    for line in slice.split_inclusive('\n') {
        let regions = hl.highlight_line(line, ss).ok()?;
        for (style, text) in regions {
            let color = Color32::from_rgb(
                style.foreground.r,
                style.foreground.g,
                style.foreground.b,
            );
            // Склеиваем соседние куски одного цвета — меньше секций в job.
            match out.last_mut() {
                Some((prev, prev_color)) if *prev_color == color => prev.push_str(text),
                _ => out.push((text.to_string(), color)),
            }
        }
    }
    Some(Highlighted(out))
}
