//! Оглавление: список заголовков из `Doc::headings` с переносом длинных
//! строк. Виртуализация как в документе: высоты строк (с переносами)
//! измеряются лениво при появлении в окне, префикс-суммы + бинарный поиск —
//! длинные документы не тормозят. Клик — скролл к разделу; текущий раздел
//! подсвечен по позиции скролла; список держит его в зоне видимости.

use eframe::egui::{vec2, CornerRadius, Rect, ScrollArea, Sense, Ui};

use crate::layout::LayoutState;
use crate::md::blocks::Doc;
use crate::view::doc_view::TOP_PAD;
use crate::view::nav;
use crate::view::themes::Palette;
use crate::view::widgets::wrapped_galley;

/// Отступ слева (плюс отступ уровня).
const PAD_X: f32 = 10.0;
/// Отступ текста от правой границы панели.
const PAD_RIGHT: f32 = 10.0;
/// Вертикальные поля строки сверху/снизу.
const ROW_VPAD: f32 = 4.0;

/// Состояние оглавления вкладки: высоты строк под текущую ширину панели.
/// При смене ширины/набора заголовков сбрасывается в одну строку на пункт
/// и перемеряется по мере прокрутки.
#[derive(Default)]
pub struct TocState {
    width: f32,
    /// Высота строки каждого пункта (перенос учтён).
    heights: Vec<f32>,
    /// offsets[i] — верх строки i; len = пункты+1.
    offsets: Vec<f32>,
}

impl TocState {
    /// Сброс под новую ширину/число пунктов (одна строка на пункт).
    fn reset(&mut self, width: f32, n: usize, base_row_h: f32) {
        if self.width == width && self.heights.len() == n {
            return;
        }
        self.width = width;
        self.heights.clear();
        self.offsets.clear();
        self.heights.resize(n, base_row_h);
        self.rebuild();
    }

    fn rebuild(&mut self) {
        let n = self.heights.len();
        self.offsets.clear();
        self.offsets.reserve(n + 1);
        self.offsets.push(0.0);
        let mut acc = 0.0;
        for &h in &self.heights {
            acc += h;
            self.offsets.push(acc);
        }
    }

    fn set_height(&mut self, i: usize, h: f32) {
        if (self.heights[i] - h).abs() > 0.5 {
            self.heights[i] = h;
            self.rebuild();
        }
    }

    /// Диапазон строк, пересекающих [top, bottom).
    fn visible_range(&self, top: f32, bottom: f32) -> std::ops::Range<usize> {
        let n = self.heights.len();
        if n == 0 || bottom <= 0.0 {
            return 0..0;
        }
        let i0 = self.offsets.partition_point(|&o| o <= top).saturating_sub(1);
        let i1 = self.offsets.partition_point(|&o| o < bottom).min(n);
        i0..i1.max(i0)
    }
}

pub fn show(
    ui: &mut Ui,
    doc: &Doc,
    layout: &LayoutState,
    pal: &Palette,
    tab: u64,
    max_level: u8,
    em: f32,
    state: &mut TocState,
) {
    // Заголовки в пределах выбранного уровня (порядок документа).
    let items: Vec<(usize, u8, &str)> = doc
        .headings
        .iter()
        .filter(|h| h.level <= max_level)
        .map(|h| (h.block_idx, h.level, h.text.as_str()))
        .collect();
    if items.is_empty() {
        return;
    }

    let base_row_h = em * 1.55 + ROW_VPAD * 2.0;
    let indent = em * 0.85;

    // Текущий раздел: последний заголовок, чей верх дошёл до верха окна
    // (+TOP_PAD: в самом начале документа первый раздел уже активен).
    let cur: Option<usize> = nav::metrics(ui.ctx(), tab).and_then(|m| {
        let threshold = m.offset + TOP_PAD + 1.0;
        let mut cur = None;
        for (row, &(blk, _, _)) in items.iter().enumerate() {
            if TOP_PAD + layout.offset(blk) <= threshold {
                cur = Some(row);
            } else {
                break;
            }
        }
        cur
    });

    ScrollArea::vertical()
        .id_salt(("toc_scroll", tab))
        .auto_shrink([false, false])
        .show_viewport(ui, |ui, viewport| {
            // Экранная позиция контента (0,0): панель может быть и справа, и
            // проскроллена — внутри show_viewport painter/interact работают в
            // ЭКРАННЫХ координатах (у левой панели при offset 0 контентные
            // совпадают с экранными — потому баг не ловился; справа строки
            // красились за клипом панели → «пустая колонка»).
            let origin = ui.cursor().min;
            let w = ui.max_rect().width();
            state.reset(w, items.len(), base_row_h);
            ui.set_min_height(state.offsets.last().copied().unwrap_or(0.0));

            for i in state.visible_range(viewport.min.y - base_row_h, viewport.max.y + base_row_h)
            {
                let &(blk, level, text) = &items[i];
                let y = state.offsets[i];
                let x = PAD_X + (level as f32 - 1.0) * indent;
                let wrap_w = (w - x - PAD_RIGHT).max(30.0);
                let g = wrapped_galley(ui, text, em, pal.muted, wrap_w);
                state.set_height(i, g.size().y + ROW_VPAD * 2.0);

                let row_h = state.heights[i];
                let rect = Rect::from_min_size(origin + vec2(0.0, y), vec2(w, row_h));
                let resp = ui.interact(rect, ui.scope_id().with(("toc_item", i)), Sense::click());
                let painter = ui.painter().clone();

                let is_cur = cur == Some(i);
                let hovered = resp.hovered();
                let color = if is_cur {
                    pal.link
                } else if hovered {
                    pal.fg
                } else {
                    pal.muted
                };
                // Плашка текущего раздела: полоса 2px у левого края.
                if is_cur {
                    painter.rect_filled(
                        Rect::from_min_size(origin + vec2(0.0, y + 2.0), vec2(2.0, row_h - 4.0)),
                        CornerRadius::same(0),
                        pal.link,
                    );
                }
                painter.galley_with_override_text_color(origin + vec2(x, y + ROW_VPAD), g, color);
                if hovered {
                    ui.ctx().set_cursor_icon(eframe::egui::CursorIcon::PointingHand);
                }
                if resp.clicked() {
                    nav::request_scroll(ui.ctx(), tab, nav::ScrollRequest::Block(blk));
                }
            }

            // Держать текущий раздел в зоне видимости (если пользователь сам
            // не скроллит список в этот момент).
            if let Some(c) = cur
                && c < state.heights.len()
            {
                let row = Rect::from_min_size(
                    origin + vec2(0.0, state.offsets[c]),
                    vec2(w, state.heights[c]),
                );
                let visible = row.min.y >= viewport.min.y && row.max.y <= viewport.max.y;
                let dragging = ui.input(|i| i.pointer.any_down())
                    && ui
                        .input(|i| i.pointer.interact_pos())
                        .is_some_and(|p| ui.clip_rect().contains(p));
                if !visible && !dragging {
                    ui.scroll_to_rect(row, None);
                }
            }
        });
}
