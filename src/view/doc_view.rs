//! Область документа: виртуальный скролл. Рендерится только видимое окно
//! блоков ± overscan; высоты — измеренные и оценённые (префикс-суммы в
//! LayoutState), скроллбар уточняется по мере измерения.

use eframe::egui::{
    self, pos2, scroll_area, Color32, Id, Rect, ScrollArea, Sense, Stroke, Ui, Vec2,
};

use crate::layout::{measure, LayoutKey, LayoutState};
use crate::md::blocks::Doc;

use super::render;
use super::themes::{self, lighten};
use super::Env;

/// Событие из документа (клик по ссылке и т.п.).
#[derive(Clone, Debug)]
pub enum DocEvent {
    OpenLink(String),
}

/// Запас рендера сверху/снизу от вьюпорта, px.
const OVERSCAN: f32 = 600.0;
const TOP_PAD: f32 = 20.0;
const BOTTOM_PAD: f32 = 40.0;

const FAB_SIZE: f32 = 44.0;
const FAB_MARGIN_BOTTOM: f32 = 20.0;
/// Справа — с учётом ширины скроллбара, чтобы зазор был как снизу.
const FAB_MARGIN_RIGHT: f32 = FAB_MARGIN_BOTTOM + 26.0;
const FAB_BG: Color32 = Color32::from_rgb(0x22, 0x22, 0x26);
const FAB_CHEVRON: Color32 = Color32::from_rgb(0x6D, 0x6D, 0x6C);
const FAB_ACCENT: Color32 = Color32::from_rgb(64, 140, 255);

pub fn show(
    ui: &mut Ui,
    doc: &Doc,
    layout: &mut LayoutState,
    env: &Env,
    tab_key: u64,
) -> Vec<DocEvent> {
    let mut events = Vec::new();
    // Тот же id, что у ScrollArea (make_persistent_id = scope_id.with(salt)).
    let scroll_id = ui.scope_id().with(("doc_scroll", tab_key));
    let fab_id = ui.scope_id().with(("fab_top", tab_key));

    // Принудительный оффсет этого кадра: просьба «наверх» или компенсация
    // «прилипания» (высоты выше вьюпорта изменились — держим контент на месте).
    let mut forced: Option<f32> = None;
    let to_top = ui.ctx().data_mut(|d| {
        let v = d.get_temp::<bool>(fab_id).unwrap_or(false);
        if v {
            d.insert_temp(fab_id, false);
        }
        v
    });
    if to_top {
        forced = Some(0.0);
    } else if let Some(delta) = layout.take_pending_scroll() {
        let cur = scroll_area::State::load(ui.ctx(), scroll_id)
            .map(|s| s.clamped_offset().y)
            .unwrap_or(0.0);
        forced = Some((cur + delta).max(0.0));
    }

    let mut sa = ScrollArea::vertical()
        .id_salt(("doc_scroll", tab_key))
        .auto_shrink([false, false]);
    if let Some(off) = forced {
        sa = sa.vertical_scroll_offset(off);
    }

    let out = sa.show_viewport(ui, |ui, viewport| {
        let avail = ui.available_width();
        let col = env.typo.width.min(avail).max(300.0);
        layout.ensure(
            doc,
            LayoutKey {
                typo: *env.typo,
                width: col,
                dark: env.pal.bg == themes::DARK.bg,
            },
        );

        // origin = экранная позиция контента (0,0); viewport — координаты контента.
        let origin = ui.cursor().min;
        ui.set_min_height(TOP_PAD + layout.total_height() + BOTTOM_PAD);

        let side = ((avail - col) / 2.0).max(0.0).floor();
        let x0 = origin.x + side;
        let vis = viewport.translate(origin.to_vec2());
        let i0 = layout
            .visible_range(viewport.min.y - OVERSCAN, viewport.max.y + OVERSCAN)
            .start;

        let mut y = origin.y + TOP_PAD + layout.offset(i0);
        for i in layout.visible_range(viewport.min.y - OVERSCAN, viewport.max.y + OVERSCAN) {
            let block = &doc.blocks[i];
            if layout.prepared(i).is_none() {
                let (p, h) = measure::measure_block(ui, env, col, block);
                layout.set_measured(i, h);
                layout.put_prepared(i, p);
            }
            let h = layout.height(i);
            let rect = Rect::from_min_size(pos2(x0, y), Vec2::new(col, h));
            let prep = layout.prepared_mut(i).expect("prepared только что создан");
            let (ev, new_h) = render::render_block(ui, rect, vis, tab_key, i, block, prep, env);
            if let Some(h) = new_h {
                layout.set_measured(i, h);
            }
            events.extend(ev);
            y += layout.height(i);
        }

        if layout.finish_frame(i0) {
            ui.ctx().request_repaint();
        }
    });

    // Прогресс прокрутки: насколько далеко от начала документа.
    let offset = out.state.clamped_offset().y;
    let scrollable = (out.content_size.y - out.inner_rect.height()).max(1.0);
    let progress = (offset / scrollable).clamp(0.0, 1.0);
    if progress > 0.001 {
        show_fab(ui, progress, fab_id);
    }
    events
}

/// Круглая кнопка «наверх» с индикатором прогресса чтения по обводке.
fn show_fab(ui: &mut Ui, progress: f32, scroll_top_id: Id) {
    let bounds = ui.max_rect();
    let min = pos2(
        bounds.max.x - FAB_MARGIN_RIGHT - FAB_SIZE,
        bounds.max.y - FAB_MARGIN_BOTTOM - FAB_SIZE,
    );
    let rect = Rect::from_min_size(min, Vec2::splat(FAB_SIZE));
    let resp = ui.interact(rect, ui.scope_id().with("fab"), Sense::click());
    let painter = ui.painter().clone();
    let center = rect.center();
    let radius = FAB_SIZE * 0.5;

    // Тёмный фон, при наведении — чуть светлее (шеврон остаётся читаемым).
    let hovered = resp.hovered();
    let bg = if hovered {
        lighten(FAB_BG, 0.15)
    } else {
        FAB_BG
    };
    let chevron = if hovered {
        lighten(FAB_CHEVRON, 0.35)
    } else {
        FAB_CHEVRON
    };
    painter.circle(center, radius, bg, Stroke::NONE);

    // Обводка-прогресс: ярко-синяя дуга, полный круг в самом низу.
    let a0 = -std::f32::consts::FRAC_PI_2;
    let sweep = std::f32::consts::TAU * progress.clamp(0.0, 1.0);
    let steps = ((progress * 64.0).ceil() as usize).max(2);
    let pts: Vec<egui::Pos2> = (0..=steps)
        .map(|i| {
            let a = a0 + sweep * (i as f32 / steps as f32);
            pos2(center.x + radius * a.cos(), center.y + radius * a.sin())
        })
        .collect();
    painter.add(egui::Shape::line(pts, Stroke::new(3.0, FAB_ACCENT)));

    // Шеврон вверх.
    let s = radius * 0.42;
    painter.line_segment(
        [
            pos2(center.x - s, center.y + s * 0.4),
            pos2(center.x, center.y - s * 0.6),
        ],
        Stroke::new(2.5, chevron),
    );
    painter.line_segment(
        [
            pos2(center.x, center.y - s * 0.6),
            pos2(center.x + s, center.y + s * 0.4),
        ],
        Stroke::new(2.5, chevron),
    );

    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if resp.clicked() {
        ui.ctx().data_mut(|d| d.insert_temp(scroll_top_id, true));
    }
}
