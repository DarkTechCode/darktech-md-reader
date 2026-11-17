//! Модель настроек (структура settings.json) и нормализация значений —
//! диапазоны и шаги из плана §4.9 (набор опций MarkMello).

use std::path::Path;

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
pub const TOC_LEVEL_MIN: u8 = 1;
pub const TOC_LEVEL_MAX: u8 = 6;
/// Ширины панелей навигации: дефолты и диапазоны растяжения (px).
pub const TOC_WIDTH_DEFAULT: f32 = 260.0;
pub const TOC_WIDTH_RANGE: (f32, f32) = (180.0, 560.0);
pub const MM_WIDTH_DEFAULT: f32 = 152.0;
pub const MM_WIDTH_RANGE: (f32, f32) = (64.0, 400.0);
/// Сколько последних файлов помним на welcome-экране.
pub const RECENT_MAX: usize = 10;
/// Максимум вкладок в сохраняемой сессии.
pub const SESSION_TABS_MAX: usize = 32;

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

/// Режим панели (оглавление/миникарта): Авто — показывать, когда полезна.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PanelMode {
    Auto,
    On,
    Off,
}

/// Сторона панели оглавления.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Left,
    Right,
}

/// Сессия приложения (часть settings.json): вкладки с позициями чтения,
/// последние файлы (welcome-экран) и геометрия окна. Восстанавливается при
/// запуске без аргументов CLI.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SessionData {
    /// Открытые вкладки (файлы, которые ещё существуют, открываются снова).
    pub tabs: Vec<TabSession>,
    /// Индекс активной вкладки.
    pub active: usize,
    /// Последние открытые файлы: путь — вниз списком welcome-экрана.
    pub recent: Vec<String>,
    /// Позиция окна, поинты (координаты монитора egui). None — не сохранена.
    pub win_pos: Option<[i32; 2]>,
    /// Нормальный (не развёрнутый) размер окна, поинты.
    pub win_size: Option<[f32; 2]>,
    /// Окно развёрнуто.
    pub win_maximized: bool,
}

/// Позиция чтения одной вкладки сессии: оффсет скролла + якорь первого
/// видимого блока. Индекс блока устаревает при изменении файла — сверька
/// по текстовому префиксу `anchor`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TabSession {
    /// Путь к файлу (display-строка; нелатинские пути поддерживаются).
    pub path: String,
    /// Оффсет скролла, px (координаты контента).
    pub y: f32,
    /// Первый видимый блок (якорь восстановления).
    pub block: usize,
    /// Верх якорного блока в координатах скролла на момент сохранения, px.
    pub block_top: f32,
    /// Префикс текста якорного блока (свежий снимок каждого кадра).
    pub anchor: Option<String>,
}

impl SessionData {
    /// Запомнить открытый файл: в начало списка, без дублей, с потолком.
    pub fn push_recent(&mut self, path: &Path) {
        let key = path_key(path);
        self.recent
            .retain(|r| path_key(Path::new(r)) != key);
        self.recent.insert(0, path.display().to_string());
        self.recent.truncate(RECENT_MAX);
    }
}

/// Ключ дедупликации путей (Windows: без учёта регистра).
#[cfg(windows)]
fn path_key(p: &Path) -> String {
    p.to_string_lossy().to_lowercase()
}

#[cfg(not(windows))]
fn path_key(p: &Path) -> String {
    p.to_string_lossy().into_owned()
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
    /// Горизонтальный скролл в широких таблицах (выкл — сжатие в ширину
    /// колонки текста с переносом ячеек по словам).
    pub table_scroll: bool,
    /// Нумерация строк в блоках кода.
    pub code_line_numbers: bool,
    /// Подсветка синтаксиса в блоках кода.
    pub code_highlight: bool,
    /// Оглавление: показывать (Авто — при достаточном числе заголовков).
    pub toc_mode: PanelMode,
    /// Сторона панели оглавления.
    pub toc_side: Side,
    /// Максимальный уровень заголовков в оглавлении, H1–H6.
    pub toc_max_level: u8,
    /// Миникарта-скелет (Авто — только для длинных документов).
    pub minimap_mode: PanelMode,
    /// Ширина панели оглавления, px (растягивается за границу).
    pub toc_width: f32,
    /// Ширина миникарты, px.
    pub minimap_width: f32,
    /// Сессия: вкладки с позициями, последние файлы, геометрия окна.
    pub session: SessionData,
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
            table_scroll: true,
            code_line_numbers: false,
            code_highlight: true,
            toc_mode: PanelMode::Auto,
            toc_side: Side::Left,
            toc_max_level: TOC_LEVEL_MAX,
            minimap_mode: PanelMode::Auto,
            toc_width: TOC_WIDTH_DEFAULT,
            minimap_width: MM_WIDTH_DEFAULT,
            session: SessionData::default(),
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
        self.toc_max_level = self.toc_max_level.clamp(TOC_LEVEL_MIN, TOC_LEVEL_MAX);
        self.toc_width = self
            .toc_width
            .clamp(TOC_WIDTH_RANGE.0, TOC_WIDTH_RANGE.1);
        self.minimap_width = self
            .minimap_width
            .clamp(MM_WIDTH_RANGE.0, MM_WIDTH_RANGE.1);
        // Сессия: потолки списков и разумные диапазоны позиции.
        self.session.tabs.truncate(SESSION_TABS_MAX);
        self.session.active = self
            .session
            .active
            .min(self.session.tabs.len().saturating_sub(1));
        for t in &mut self.session.tabs {
            t.y = t.y.max(0.0);
        }
        self.session.recent.truncate(RECENT_MAX);
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
        assert!(d.table_scroll);
        assert!(!d.code_line_numbers);
        assert!(d.code_highlight);
        assert_eq!(d.toc_mode, PanelMode::Auto);
        assert_eq!(d.toc_side, Side::Left);
        assert_eq!(d.toc_max_level, 6);
        assert_eq!(d.minimap_mode, PanelMode::Auto);
    }

    #[test]
    fn normalize_clamps_and_steps() {
        let mut d = SettingsData {
            font_size: 99,
            line_height: 3.0,
            text_width: 5000,
            letter_spacing: 9.0,
            ui_font_size: 0,
            toc_max_level: 9,
            ..Default::default()
        };
        d.normalize();
        assert_eq!(d.font_size, 24);
        assert!((d.line_height - 2.0).abs() < 1e-6);
        assert_eq!(d.text_width, 1280);
        assert!((d.letter_spacing - 2.0).abs() < 1e-6);
        assert_eq!(d.ui_font_size, 11);
        assert_eq!(d.toc_max_level, 6);

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
        assert!(js.contains("\"tableScroll\":true"));
        assert!(js.contains("\"codeLineNumbers\":false"));
        assert!(js.contains("\"codeHighlight\":true"));
        assert!(js.contains("\"tocMode\":\"auto\""));
        assert!(js.contains("\"tocSide\":\"left\""));
        assert!(js.contains("\"tocMaxLevel\":6"));
        assert!(js.contains("\"minimapMode\":\"auto\""));
        assert!(js.contains("\"tocWidth\":260.0"));
        assert!(js.contains("\"minimapWidth\":152.0"));
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

    #[test]
    fn session_json_roundtrip() {
        let d = SettingsData {
            session: SessionData {
                tabs: vec![TabSession {
                    path: "D:/док/заметки.md".into(),
                    y: 4310.5,
                    block: 87,
                    block_top: 4290.0,
                    anchor: Some("Якорь видимого блока".into()),
                }],
                active: 0,
                recent: vec!["D:/док/заметки.md".into(), "C:/temp/demo.md".into()],
                win_pos: Some([-8, 31]),
                win_size: Some([1100.0, 760.0]),
                win_maximized: true,
            },
            ..Default::default()
        };
        let js = serde_json::to_string(&d).unwrap();
        assert!(js.contains(r#""tabs":[{"path":"D:/док/заметки.md","y":4310.5,"block":87,"blockTop":4290.0,"anchor":"Якорь видимого блока"}]"#));
        assert!(js.contains(r#""recent":["D:/док/заметки.md","C:/temp/demo.md"]"#));
        assert!(js.contains(r#""winPos":[-8,31]"#));
        assert!(js.contains(r#""winSize":[1100.0,760.0]"#));
        assert!(js.contains(r#""winMaximized":true"#));
        assert_eq!(serde_json::from_str::<SettingsData>(&js).unwrap(), d);
    }

    #[test]
    fn session_defaults_when_missing_and_normalize_caps() {
        let d: SettingsData = serde_json::from_str(r#"{"fontSize":20}"#).unwrap();
        assert!(d.session.tabs.is_empty());
        assert!(d.session.recent.is_empty());
        assert!(!d.session.win_maximized);

        let mut d = SettingsData {
            session: SessionData {
                tabs: (0..40)
                    .map(|i| TabSession {
                        path: format!("f{i}.md"),
                        y: -5.0,
                        ..Default::default()
                    })
                    .collect(),
                active: 39,
                recent: (0..20).map(|i| format!("r{i}.md")).collect(),
                ..Default::default()
            },
            ..Default::default()
        };
        d.normalize();
        assert_eq!(d.session.tabs.len(), SESSION_TABS_MAX);
        assert_eq!(d.session.active, SESSION_TABS_MAX - 1);
        assert!(d.session.tabs.iter().all(|t| t.y >= 0.0));
        assert_eq!(d.session.recent.len(), RECENT_MAX);
    }

    #[test]
    fn push_recent_dedup_and_cap() {
        let mut s = SessionData::default();
        for i in 0..RECENT_MAX + 2 {
            s.push_recent(Path::new(&format!("C:/temp/{i}.md")));
        }
        assert_eq!(s.recent.len(), RECENT_MAX);
        assert_eq!(s.recent[0], "C:/temp/11.md");

        // Повторное открытие поднимает наверх без дублей (регистр не важен).
        s.push_recent(Path::new("c:/TEMP/0.md"));
        assert_eq!(s.recent.len(), RECENT_MAX);
        assert_eq!(s.recent[0], "c:/TEMP/0.md");
        assert_eq!(s.recent.iter().filter(|r| *r == "C:/temp/0.md").count(), 0);
    }
}
