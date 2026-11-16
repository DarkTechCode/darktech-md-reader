//! Миникарта-скелет справа: документ в виде полосок (заголовки — плотнее и
//! ярче, код — монотонные строки, картинки — рамки). Не текст в миниатюре:
//! блоки квантуются в пиксельные ряды миникарты (O(её высоты)) и кэш
//! пересчитывается только при изменении раскладки. Прямоугольник вьюпорта,
//! клик/драг — навигация.

use eframe::egui::{pos2, vec2, Color32, Rect, Sense, Stroke, StrokeKind, Ui};

use crate::layout::LayoutState;
use crate::md::blocks::{BlockKind, Doc};
use crate::view::doc_view::{BOTTOM_PAD, TOP_PAD};
use crate::view::nav;
use crate::view::themes::Palette;

/// Тип полоски скелета.
#[derive(Clone, Copy, PartialEq)]
enum StripKind {
    Head(u8),
    Text,
    Code,
    Quote,
    Table,
    Image,
    Rule,
}

impl StripKind {
    fn of(block: &BlockKind) -> Self {
        match block {
            BlockKind::Heading { level, .. } => StripKind::Head(*level),
            BlockKind::Code { .. } => StripKind::Code,
            BlockKind::Quote { .. } => StripKind::Quote,
            BlockKind::Table { .. } => StripKind::Table,
            BlockKind::Image { .. } => StripKind::Image,
            BlockKind::Rule => StripKind::Rule,
            BlockKind::Paragraph { .. } | BlockKind::ListItem { .. } => StripKind::Text,
        }
    }

    /// Приоритет при слиянии блоков в один ряд: заголовок побеждает.
    fn rank(self) -> u8 {
        match self {
            StripKind::Head(_) => 6,
            StripKind::Image => 5,
            StripKind::Table => 4,
            StripKind::Code => 3,
            StripKind::Quote => 2,
            StripKind::Text => 1,
            StripKind::Rule => 0,
        }
    }
}

/// Один пиксельный ряд миникарты.
#[derive(Clone)]
struct Strip {
    row: u32,
    kind: StripKind,
    /// Доля ширины (для «строк текста» — квантованная), 0..1.
    w: f32,
}

/// Кэш полосок: пересчёт только при изменении раскладки/размера/темы.
#[derive(Clone)]
struct Cache {
    key: (u64, u32, usize, bool),
    strips: Vec<Strip>,
}

/// Внутренние поля скелета.
const INSET: f32 = 8.0;
/// Миникарта листает документ чуть живее обычного скролла.
const WHEEL_BOOST: f32 = 1.5;

pub fn show(ui: &mut Ui, doc: &Doc, layout: &LayoutState, pal: &Palette, tab: u64, dark: bool) {
    let bounds = ui.max_rect();
    let area = Rect::from_min_size(
        pos2(bounds.min.x + INSET, bounds.min.y + INSET),
        vec2(
            (bounds.width() - INSET * 2.0).max(0.0),
            (bounds.height() - INSET * 2.0).max(0.0),
        ),
    );
    let rows = area.height().floor().max(0.0) as u32;
    let content_h = TOP_PAD + layout.total_height() + BOTTOM_PAD;
    if rows == 0 || content_h <= 0.0 || doc.blocks.is_empty() {
        return;
    }
    // Раскладка ещё не построена (первый кадр вкладки) или стриминг принёс
    // новые блоки — дождёмся ensure в doc_view, иначе высоты не готовы.
    if layout.blocks_len() < doc.blocks.len() {
        return;
    }

    // Полоски: по пиксельному ряду на блок (слияние по приоритету).
    let cache_key = (layout.version(), rows, doc.blocks.len(), dark);
    let cache_id = nav::key(tab, "mm_cache");
    let mut cache = ui
        .ctx()
        .data(|d| d.get_temp::<Cache>(cache_id))
        .unwrap_or(Cache {
            key: (0, 0, 0, false),
            strips: Vec::new(),
        });
    if cache.key != cache_key {
        cache.strips = build_strips(doc, layout, rows, content_h);
        cache.key = cache_key;
        ui.ctx()
            .data_mut(|d| d.insert_temp(cache_id, cache.clone()));
    }
    let strips = &cache.strips;

    let painter = ui.painter().clone();
    let scale = area.height() / content_h;
    let sx = area.min.x;
    let full_w = area.width();

    // Полоски скелета. Заголовки рисуются вторым проходом — их 2px высота
    // перекрывает соседний ряд и должна остаться поверх.
    let draw = |s: &Strip| {
        let y = area.min.y + s.row as f32;
        match s.kind {
            StripKind::Head(level) => {
                let color = if level <= 2 {
                    alpha(pal.link, if dark { 225 } else { 215 })
                } else {
                    alpha(pal.fg, if dark { 205 } else { 185 })
                };
                painter.rect_filled(
                    Rect::from_min_size(pos2(sx, y), vec2(full_w, 2.0)),
                    eframe::egui::CornerRadius::same(0),
                    color,
                );
            }
            StripKind::Text => {
                painter.rect_filled(
                    Rect::from_min_size(pos2(sx, y), vec2(full_w * s.w, 1.0)),
                    eframe::egui::CornerRadius::same(0),
                    alpha(pal.fg, if dark { 56 } else { 70 }),
                );
            }
            StripKind::Code => {
                painter.rect_filled(
                    Rect::from_min_size(pos2(sx, y), vec2(full_w, 1.0)),
                    eframe::egui::CornerRadius::same(0),
                    alpha(pal.fg, if dark { 40 } else { 52 }),
                );
            }
            StripKind::Quote => {
                painter.rect_filled(
                    Rect::from_min_size(pos2(sx, y), vec2(full_w * 0.7, 1.0)),
                    eframe::egui::CornerRadius::same(0),
                    alpha(pal.quote_bar, if dark { 120 } else { 140 }),
                );
            }
            StripKind::Table => {
                painter.rect_filled(
                    Rect::from_min_size(pos2(sx, y), vec2(full_w, 1.0)),
                    eframe::egui::CornerRadius::same(0),
                    alpha(pal.fg, if dark { 70 } else { 84 }),
                );
            }
            StripKind::Image => {
                // Картинка — рамка, а не заливка.
                painter.rect(
                    Rect::from_min_size(pos2(sx, y), vec2(full_w, 2.0)),
                    eframe::egui::CornerRadius::same(0),
                    Color32::TRANSPARENT,
                    Stroke::new(1.0, alpha(pal.muted, if dark { 150 } else { 165 })),
                    StrokeKind::Middle,
                );
            }
            StripKind::Rule => {
                painter.rect_filled(
                    Rect::from_min_size(
                        pos2(sx + full_w * 0.3, y),
                        vec2(full_w * 0.4, 1.0),
                    ),
                    eframe::egui::CornerRadius::same(0),
                    alpha(pal.fg, if dark { 70 } else { 84 }),
                );
            }
        }
    };
    for s in strips.iter().filter(|s| !matches!(s.kind, StripKind::Head(_))) {
        draw(s);
    }
    for s in strips.iter().filter(|s| matches!(s.kind, StripKind::Head(_))) {
        draw(s);
    }

    // Прямоугольник вьюпорта.
    let Some(m) = nav::metrics(ui.ctx(), tab) else {
        return;
    };
    let vy = area.min.y + m.offset * scale;
    let vh = (m.viewport_h * scale).max(6.0).min(area.height());
    let vp = Rect::from_min_size(pos2(area.min.x, vy), vec2(full_w, vh));
    painter.rect(
        vp,
        eframe::egui::CornerRadius::same(0),
        alpha(pal.selection, if dark { 40 } else { 55 }),
        Stroke::new(1.0, alpha(pal.muted, if dark { 130 } else { 150 })),
        StrokeKind::Middle,
    );

    // Клик/драг — центрировать документ на выбранном месте.
    let resp = ui.interact(area, ui.scope_id().with(("minimap", tab)), Sense::click_and_drag());
    let dragging = resp.dragged() || resp.clicked();
    if dragging
        && let Some(p) = resp.interact_pointer_pos()
    {
        let frac = ((p.y - area.min.y) / area.height()).clamp(0.0, 1.0);
        let target = frac * content_h - m.viewport_h * 0.5;
        let max = (content_h - m.viewport_h).max(0.0);
        nav::stop_autoscroll(ui.ctx(), tab);
        nav::request_scroll(ui.ctx(), tab, nav::ScrollRequest::Y(target.clamp(0.0, max)));
    }
    // Колесо над миникартой прокручивает документ — та же скорость, что у
    // обычного скролла (units → points, как это делает egui), и чуть живее.
    if resp.hovered() {
        let line_speed = ui
            .ctx()
            .options(|o| o.input_options.line_scroll_speed);
        let wheel: f32 = ui.input(|i| {
            i.events
                .iter()
                .map(|e| match e {
                    eframe::egui::Event::MouseWheel { delta, unit, .. } => match unit {
                        eframe::egui::MouseWheelUnit::Point => delta.y,
                        eframe::egui::MouseWheelUnit::Line => delta.y * line_speed,
                        eframe::egui::MouseWheelUnit::Page => delta.y * m.viewport_h,
                    },
                    _ => 0.0,
                })
                .sum()
        });
        if wheel != 0.0 {
            let max = (content_h - m.viewport_h).max(0.0);
            let target = (m.offset - wheel * WHEEL_BOOST).clamp(0.0, max);
            nav::stop_autoscroll(ui.ctx(), tab);
            nav::request_scroll(ui.ctx(), tab, nav::ScrollRequest::Y(target));
        }
    }
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(eframe::egui::CursorIcon::PointingHand);
    }
}

/// Полоски по одному на пиксельный ряд: блоки квантуются в ряды, в один ряд
/// попадает сильнейший тип (заголовок > картинка > таблица > код > …).
fn build_strips(doc: &Doc, layout: &LayoutState, rows: u32, content_h: f32) -> Vec<Strip> {
    let scale = rows as f32 / content_h;
    // (тип, доля ширины) на ряд; None — ряд пока не покрыт.
    let mut grid: Vec<Option<(StripKind, f32)>> = vec![None; rows as usize];
    for (i, block) in doc.blocks.iter().enumerate() {
        let y0 = (TOP_PAD + layout.offset(i)) * scale;
        let y1 = y0 + layout.height(i) * scale;
        let r0 = (y0.floor().max(0.0) as u32).min(rows);
        let mut r1 = (y1.ceil().max(0.0) as u32).min(rows);
        if r1 <= r0 {
            r1 = r0 + 1; // субпиксельный блок занимает хотя бы свой ряд
        }
        let kind = StripKind::of(block);
        let w = width_frac(i);
        for r in r0..r1 {
            let cell = &mut grid[r as usize];
            *cell = Some(match cell.take() {
                Some((k0, w0)) => {
                    if kind.rank() > k0.rank() {
                        (kind, w)
                    } else {
                        (k0, w0)
                    }
                }
                None => (kind, w),
            });
        }
    }
    grid.into_iter()
        .enumerate()
        .filter_map(|(row, cell)| {
            cell.map(|(kind, w)| Strip {
                row: row as u32,
                kind,
                w,
            })
        })
        .collect()
}

/// Детерминированная «длина строки текста»: хэш индекса блока, 6 градаций.
fn width_frac(i: usize) -> f32 {
    let mut h = (i as u64).wrapping_mul(0x9E3779B97F4A7C15);
    h ^= h >> 29;
    h = h.wrapping_mul(0xBF58476D1CE4E5B9);
    let step = (h % 6) as f32;
    0.58 + step * 0.065
}

fn alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}
