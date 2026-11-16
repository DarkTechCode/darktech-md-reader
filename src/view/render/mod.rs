//! Отрисовка блоков по подготовленной раскладке (crate::layout::measure).
//! Каждый блок рисуется в своём прямоугольнике со стабильным id —
//! поэтому автогенерация id egui не «плывёт» при изменении видимого окна.

pub mod code;
pub mod misc;
pub mod table;
pub mod text;

use eframe::egui::{Rect, Ui, UiBuilder};

use crate::layout::measure::Prepared;
use crate::md::blocks::BlockKind;

use super::doc_view::DocEvent;
use super::Env;

/// Рисует блок `idx` в `rect` (экранные координаты). `vis` — видимый
/// прямоугольник скролла (тоже экранный), нужен блокам с внутренней
/// виртуализацией (код/таблицы). Возвращает события и новую высоту блока,
/// если она уточнилась при отрисовке.
#[allow(clippy::type_complexity)]
pub fn render_block(
    ui: &mut Ui,
    rect: Rect,
    vis: Rect,
    tab_key: u64,
    idx: usize,
    block: &BlockKind,
    prep: &mut Prepared,
    env: &Env,
) -> (Vec<DocEvent>, Option<f32>) {
    let id = ui.scope_id().with(("blk", tab_key, idx));
    ui.scope_builder(UiBuilder::new().max_rect(rect).scope_id(id), |ui| {
        match (block, prep) {
            (BlockKind::Heading { spans, .. }, Prepared::Text(p)) => {
                (text::render_text(ui, rect, spans, p, env), None)
            }
            (BlockKind::Paragraph { spans }, Prepared::Text(p)) => {
                (text::render_text(ui, rect, spans, p, env), None)
            }
            (BlockKind::Quote { spans, .. }, Prepared::Text(p)) => {
                (text::render_text(ui, rect, spans, p, env), None)
            }
            (BlockKind::ListItem { spans, .. }, Prepared::Text(p)) => {
                (text::render_text(ui, rect, spans, p, env), None)
            }
            (BlockKind::Code { lang, code }, Prepared::Code(p)) => {
                code::render_code(ui, rect, vis, idx, lang, code, p, env)
            }
            (BlockKind::Table { aligns, rows }, Prepared::Table(p)) => {
                table::render_table(ui, rect, vis, idx, aligns, rows, p, env)
            }
            (BlockKind::Rule, Prepared::Rule) => {
                misc::render_rule(ui, rect, env);
                (Vec::new(), None)
            }
            (BlockKind::Image { .. }, Prepared::Image(p)) => {
                misc::render_image(ui, rect, p, env);
                (Vec::new(), None)
            }
            // Блок и кэш всегда согласованы (создаются парой).
            _ => (Vec::new(), None),
        }
    })
    .inner
}
