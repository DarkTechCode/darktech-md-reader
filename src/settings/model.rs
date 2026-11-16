//! Модель настроек (структура settings.json) и нормализация значений —
//! диапазоны и шаги из плана §4.9 (набор опций MarkMello).

use serde::{Deserialize, Serialize};

use crate::i18n::{self, Lang};
use crate::view::themes::{FontChoice, Typo};

// --- Диапазоны опций (план §4.9). ---
pub const FONT_MIN: u8 = 14;
pub const FONT_MAX: u8 = 24;
pub const FONT_DEFAULT: u8 = 18;
pub const UI_FONT_MIN: u8 = 11;
pub const UI_FONT_MAX: u8 = 18;
pub const UI_FONT_DEFAULT: u8 = 14;
pub const LINE_HEIGHT_MIN: f32 = 1.40;
pub const LINE_HEIGHT_MAX: f32 = 2.00;
pub const LINE_HEIGHT_STEP: f32 = 0.05;
pub const LINE_HEIGHT_DEFAULT: f32 = 1.70;
pub const LETTER_MIN: f32 = 0.0;
pub const LETTER_MAX: f32 = 2.0;
pub const LETTER_STEP: f32 = 0.1;
pub const WIDTH_MIN: u16 = 640;
pub const WIDTH_MAX: u16 = 1280;
pub const WIDTH_STEP: u16 = 20;
/// Пресеты ширины колонки (Ctrl+1..3): узкая/средняя/широкая.
pub const WIDTH_PRESETS: [u16; 3] = [640, 820, 1080];

/// Тема интерфейса.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    Light,
    Dark,
    System,
}

/// Язык интерфейса.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LangMode {
    Ru,
    En,
    System,
}

/// Все настройки приложения. Дефолты — план §4.9; шрифт по умолчанию Sans —
/// просьба владельца (осознанное отклонение от Serif в плане).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SettingsData {
    pub theme: ThemeMode,
    pub language: LangMode,
    pub font: FontChoice,
    /// Размер шрифта документа, px.
    pub font_size: u8,
    /// Межстрочный интервал (множитель).
    pub line_height: f32,
    /// Ширина текстовой колонки, px.
    pub text_width: u16,
    /// Межбуквенный интервал, px.
    pub letter_spacing: f32,
    /// Размер шрифта интерфейса, px.
    pub ui_font_size: u8,
    /// Растягивать текст на всю ширину окна (тумблер в статус-баре).
    pub fit_width: bool,
    /// Выравнивать содержимое ячеек таблиц по верхнему краю (иначе по центру).
    pub table_top: bool,
    /// Нумерация строк в блоках кода.
    pub code_line_numbers: bool,
    /// Подсветка синтаксиса в блоках кода.
    pub code_highlight: bool,
}

impl Default for SettingsData {
    fn default() -> Self {
        Self {
            theme: ThemeMode::System,
            language: LangMode::System,
            font: FontChoice::Sans,
            font_size: FONT_DEFAULT,
            line_height: LINE_HEIGHT_DEFAULT,
            text_width: WIDTH_PRESETS[1],
            letter_spacing: 0.0,
            ui_font_size: UI_FONT_DEFAULT,
            fit_width: false,
            table_top: false,
            code_line_numbers: false,
            code_highlight: true,
        }
    }
}

impl SettingsData {
    /// Подтягивает значения в допустимые диапазоны и к шагам слайдеров.
    pub fn normalize(&mut self) {
        self.font_size = self.font_size.clamp(FONT_MIN, FONT_MAX);
        self.ui_font_size = self.ui_font_size.clamp(UI_FONT_MIN, UI_FONT_MAX);
        self.line_height = snap(
            self.line_height.clamp(LINE_HEIGHT_MIN, LINE_HEIGHT_MAX),
            LINE_HEIGHT_STEP,
            LINE_HEIGHT_MIN,
            LINE_HEIGHT_MAX,
        );
        self.letter_spacing = snap(
            self.letter_spacing.clamp(LETTER_MIN, LETTER_MAX),
            LETTER_STEP,
            LETTER_MIN,
            LETTER_MAX,
        );
        let w = self.text_width.clamp(WIDTH_MIN, WIDTH_MAX) as f32 / WIDTH_STEP as f32;
        self.text_width = (w.round() as u16).saturating_mul(WIDTH_STEP);
    }

    /// Типографика документа из настроек (для Env и LayoutKey).
    pub fn typo(&self) -> Typo {
        Typo {
            font: self.font,
            size: self.font_size as f32,
            line_height: self.line_height,
            width: self.text_width as f32,
            letter_spacing: self.letter_spacing,
        }
    }

    /// Эффективный язык интерфейса (System → язык ОС).
    pub fn lang(&self) -> Lang {
        match self.language {
            LangMode::Ru => Lang::Ru,
            LangMode::En => Lang::En,
            LangMode::System => i18n::system_lang(),
        }
    }
}

/// Округление к сетке шага: min + round((v-min)/step)*step.
fn snap(v: f32, step: f32, min: f32, max: f32) -> f32 {
    (((v - min) / step).round() * step + min).clamp(min, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_plan() {
        let d = SettingsData::default();
        assert_eq!(d.theme, ThemeMode::System);
        assert_eq!(d.language, LangMode::System);
        assert_eq!(d.font, FontChoice::Sans); // отклонение по просьбе владельца
        assert_eq!(d.font_size, 18);
        assert_eq!(d.line_height, 1.7);
        assert_eq!(d.text_width, 820);
        assert_eq!(d.letter_spacing, 0.0);
        assert_eq!(d.ui_font_size, 14);
        assert!(!d.fit_width);
        assert!(!d.table_top);
        assert!(!d.code_line_numbers);
        assert!(d.code_highlight);
    }

    #[test]
    fn normalize_clamps_and_steps() {
        let mut d = SettingsData {
            font_size: 99,
            line_height: 3.0,
            text_width: 5000,
            letter_spacing: 9.0,
            ui_font_size: 0,
            ..Default::default()
        };
        d.normalize();
        assert_eq!(d.font_size, 24);
        assert!((d.line_height - 2.0).abs() < 1e-6);
        assert_eq!(d.text_width, 1280);
        assert!((d.letter_spacing - 2.0).abs() < 1e-6);
        assert_eq!(d.ui_font_size, 11);

        let mut d = SettingsData {
            line_height: 1.52,
            ..Default::default()
        };
        d.normalize();
        assert!((d.line_height - 1.50).abs() < 1e-6);

        let mut d = SettingsData {
            text_width: 833,
            ..Default::default()
        };
        d.normalize();
        assert_eq!(d.text_width, 840);
    }

    #[test]
    fn json_camelcase_roundtrip() {
        let d = SettingsData::default();
        let js = serde_json::to_string(&d).unwrap();
        assert!(js.contains("\"theme\":\"system\""));
        assert!(js.contains("\"language\":\"system\""));
        assert!(js.contains("\"font\":\"sans\""));
        assert!(js.contains("\"fontSize\":18"));
        assert!(js.contains("\"lineHeight\":1.7"));
        assert!(js.contains("\"textWidth\":820"));
        assert!(js.contains("\"letterSpacing\":0.0"));
        assert!(js.contains("\"uiFontSize\":14"));
        assert!(js.contains("\"fitWidth\":false"));
        assert!(js.contains("\"tableTop\":false"));
        assert!(js.contains("\"codeLineNumbers\":false"));
        assert!(js.contains("\"codeHighlight\":true"));
        assert_eq!(serde_json::from_str::<SettingsData>(&js).unwrap(), d);
    }

    #[test]
    fn partial_json_gets_defaults_for_missing() {
        let d: SettingsData = serde_json::from_str(r#"{"fontSize":20,"font":"mono"}"#).unwrap();
        assert_eq!(d.font_size, 20);
        assert_eq!(d.font, FontChoice::Mono);
        assert_eq!(d.theme, ThemeMode::System);
        assert_eq!(d.text_width, 820);
    }

    #[test]
    fn typo_maps_all_fields() {
        let d = SettingsData {
            font: FontChoice::Serif,
            font_size: 22,
            line_height: 1.9,
            text_width: 640,
            letter_spacing: 1.5,
            ..Default::default()
        };
        let typo = d.typo();
        assert_eq!(typo.font, FontChoice::Serif);
        assert_eq!(typo.size, 22.0);
        assert!((typo.line_height - 1.9).abs() < 1e-6);
        assert_eq!(typo.width, 640.0);
        assert!((typo.letter_spacing - 1.5).abs() < 1e-6);
    }
}
