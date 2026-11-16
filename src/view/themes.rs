//! Палитры (дух MarkMello) и типографика документа из настроек.

use eframe::egui::{self as egui, Color32, Context, FontFamily, Visuals};
use eframe::egui::epaint::text::TextOptions;
use serde::{Deserialize, Serialize};

use crate::fonts;

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    /// Фон читалки.
    pub bg: Color32,
    /// Основной текст.
    pub fg: Color32,
    /// Приглушённый текст (цитаты, подписи).
    pub muted: Color32,
    /// Ещё тише (хинты, watermark).
    pub faint: Color32,
    /// Линии и рамки.
    pub border: Color32,
    /// Акцент/ссылки.
    pub link: Color32,
    /// Фон блока кода.
    pub code_bg: Color32,
    /// Фон инлайн-кода.
    pub inline_code_bg: Color32,
    /// Полоса цитаты.
    pub quote_bar: Color32,
    /// Фон цитаты.
    pub quote_bg: Color32,
    /// Фон шапки таблицы.
    pub table_head_bg: Color32,
    /// Выделение (поиск/selection).
    pub selection: Color32,
    /// Цвета GFM alerts (полоса + заголовок).
    pub alert_note: Color32,
    pub alert_tip: Color32,
    pub alert_important: Color32,
    pub alert_warning: Color32,
    pub alert_caution: Color32,
    /// Кнопка «наверх»: фон, шеврон, рамка (в тёмной рамки нет).
    pub fab_bg: Color32,
    pub fab_chevron: Color32,
    pub fab_border: Color32,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

pub const DARK: Palette = Palette {
    bg: rgb(27, 27, 30),
    fg: rgb(214, 214, 210),
    muted: rgb(150, 150, 147),
    faint: rgb(104, 104, 102),
    border: rgb(51, 51, 56),
    link: rgb(110, 168, 254),
    code_bg: rgb(34, 34, 38),
    inline_code_bg: rgb(44, 44, 50),
    quote_bar: rgb(90, 90, 96),
    quote_bg: rgb(31, 31, 35),
    table_head_bg: rgb(34, 34, 38),
    selection: rgb(50, 65, 110),
    alert_note: rgb(68, 147, 248),
    alert_tip: rgb(63, 185, 80),
    alert_important: rgb(171, 125, 248),
    alert_warning: rgb(210, 153, 34),
    alert_caution: rgb(248, 81, 73),
    fab_bg: rgb(34, 34, 38),
    fab_chevron: rgb(109, 109, 108),
    fab_border: Color32::TRANSPARENT,
};

pub const LIGHT: Palette = Palette {
    bg: rgb(250, 249, 247),
    fg: rgb(36, 36, 36),
    muted: rgb(107, 107, 107),
    faint: rgb(154, 154, 150),
    border: rgb(226, 224, 220),
    link: rgb(47, 111, 222),
    code_bg: rgb(241, 240, 236),
    inline_code_bg: rgb(232, 231, 226),
    quote_bar: rgb(168, 166, 160),
    quote_bg: rgb(245, 244, 240),
    table_head_bg: rgb(241, 240, 236),
    selection: rgb(190, 205, 245),
    alert_note: rgb(31, 111, 235),
    alert_tip: rgb(26, 127, 55),
    alert_important: rgb(134, 74, 224),
    alert_warning: rgb(174, 119, 19),
    alert_caution: rgb(207, 45, 38),
    fab_bg: rgb(255, 255, 255),
    fab_chevron: rgb(105, 105, 103),
    fab_border: rgb(216, 214, 210),
};

/// Осветление цвета к белому (для hover-состояний).
pub fn lighten(c: Color32, k: f32) -> Color32 {
    let ch = |v: u8| (v as f32 + (255.0 - v as f32) * k).round() as u8;
    Color32::from_rgb(ch(c.r()), ch(c.g()), ch(c.b()))
}

/// Применяет палитры к egui. ВАЖНО: пишем в ОБА слота тем (Dark и Light) —
/// активный слот выбирается ThemePreference/системой и может переключиться
/// среди кадра; если палитра лежит только в активном, новый слот отдаст
/// дефолты egui (микс стилей).
pub fn apply(ctx: &Context) {
    // text_options ФИКСИРУЕМ одни на оба слота: у Visuals::dark()/light()
    // разные color_transfer_function — если значение меняется при смене
    // темы, egui сбрасывает ВЕСЬ атлас глифов (Fonts::set_options →
    // GlyphAtlas::new). Закэшированные нами Arc<Galley> при этом остаются
    // со старыми UV → «кракозябры»/пропадающий текст + фриз на
    // ре-растеризации. Берём текущие text_options активного слота.
    let text_options = ctx.global_style().visuals.text_options;
    ctx.set_visuals_of(egui::Theme::Dark, visuals(&DARK, true, text_options));
    ctx.set_visuals_of(egui::Theme::Light, visuals(&LIGHT, false, text_options));

    // Широкий «виндоузовый» скроллбар: сплошной, всегда видимый, легко схватить.
    ctx.all_styles_mut(|style| {
        style.spacing.scroll = egui::style::ScrollStyle {
            floating: false,
            bar_width: 20.0,
            handle_min_length: 32.0,
            bar_inner_margin: 1.0,
            bar_outer_margin: 0.0,
            dormant_background_opacity: 1.0,
            dormant_handle_opacity: 1.0,
            ..egui::style::ScrollStyle::solid()
        };
    });
}

/// Визуальный стиль egui из палитры (панели, выделение, ссылки, скроллбар).
fn visuals(pal: &Palette, dark: bool, text_options: TextOptions) -> Visuals {
    let mut vis = if dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    vis.panel_fill = pal.bg;
    vis.window_fill = pal.code_bg;
    vis.extreme_bg_color = pal.code_bg;
    vis.code_bg_color = pal.inline_code_bg;
    vis.hyperlink_color = pal.link;
    vis.selection.bg_fill = pal.selection;
    vis.widgets.noninteractive.bg_fill = pal.bg;
    vis.widgets.noninteractive.fg_stroke.color = pal.fg;
    // Цвет ручки скроллбара (egui берёт его из widgets.*.bg_fill).
    vis.widgets.inactive.bg_fill = lighten(pal.code_bg, 0.14);
    vis.widgets.hovered.bg_fill = lighten(pal.code_bg, 0.26);
    vis.widgets.active.bg_fill = lighten(pal.code_bg, 0.32);
    // Плоский стиль: без скруглений.
    vis.widgets.inactive.corner_radius = egui::CornerRadius::same(0);
    vis.widgets.hovered.corner_radius = egui::CornerRadius::same(0);
    vis.widgets.active.corner_radius = egui::CornerRadius::same(0);
    vis.text_options = text_options;
    vis
}

/// Выбор шрифта документа (в settings.json — "serif"/"sans"/"mono").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FontChoice {
    Serif,
    Sans,
    Mono,
}

/// Типографика документа (все опции из панели настроек чтения).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Typo {
    pub font: FontChoice,
    /// Размер шрифта, px.
    pub size: f32,
    /// Межстрочный интервал (множитель).
    pub line_height: f32,
    /// Ширина колонки текста, px.
    pub width: f32,
    /// Межбуквенный интервал, px.
    pub letter_spacing: f32,
}

impl Default for Typo {
    fn default() -> Self {
        Self {
            font: FontChoice::Sans,
            size: 18.0,
            line_height: 1.7,
            width: 820.0,
            letter_spacing: 0.0,
        }
    }
}

impl Typo {
    pub fn family(&self) -> FontFamily {
        match self.font {
            FontChoice::Serif => FontFamily::Name(fonts::SERIF.into()),
            FontChoice::Sans => FontFamily::Name(fonts::SANS.into()),
            FontChoice::Mono => FontFamily::Name(fonts::MONO.into()),
        }
    }

    /// Высота строки в px.
    pub fn row_height(&self) -> f32 {
        self.size * self.line_height
    }
}

/// Масштаб размера шрифта для уровня заголовка.
pub fn heading_ratio(level: u8) -> f32 {
    match level {
        1 => 1.75,
        2 => 1.45,
        3 => 1.25,
        4 => 1.1,
        5 => 1.0,
        _ => 0.92,
    }
}
