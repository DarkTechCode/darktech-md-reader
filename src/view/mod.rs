//! Отрисовка документа.

pub mod doc_view;
pub mod minimap;
pub mod nav;
pub mod outline;
pub mod render;
pub mod settings_panel;
pub mod themes;
pub mod widgets;
pub mod welcome;

use crate::i18n::Lang;
use themes::{Palette, Typo};

/// Контекст отрисовки одного кадра.
pub struct Env<'a> {
    pub pal: &'a Palette,
    pub typo: &'a Typo,
    pub lang: Lang,
    /// Растягивать текст на всю ширину окна (игнорирует typo.width).
    pub fit_width: bool,
    /// Выравнивать ячейки таблиц по верхнему краю (иначе по центру).
    pub table_top: bool,
    /// Нумерация строк в блоках кода.
    pub code_line_numbers: bool,
    /// Подсветка синтаксиса в блоках кода.
    pub code_highlight: bool,
}
