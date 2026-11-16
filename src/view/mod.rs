//! Отрисовка документа.

pub mod doc_view;
pub mod render_block;
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
}
