//! Область документа: скролл + центрированная колонка типографики.
//! Этап 0 — наивный рендер всех блоков; этап 1 заменит на виртуализацию.

use eframe::egui::{self, pos2, Color32, Id, Rect, ScrollArea, Sense, Stroke, Ui, Vec2};

use crate::md::blocks::Doc;

use super::render_block::show_block;
use super::Env;

/// Событие из документа (клик по ссылке и т.п.).
#[derive(Clone, Debug)]
pub enum DocEvent {
    OpenLink(String),
}

const FAB_SIZE: f32 = 44.0;
const FAB_MARGIN: f32 = 20.0;

pub fn show(ui: &mut Ui, doc: &Doc, env: &Env) -> Vec<DocEvent> {
    let mut events = Vec::new();
    let scroll_top_id = ui.scope_id().with("scroll_top");

    let out = ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_viewport(ui, |ui, viewport| {
            // Просьба «наверх» от круглой кнопки: скроллируем к началу контента.
            if ui.ctx().data_mut(|d| {
                let v = d.get_temp::<bool>(scroll_top_id).unwrap_or(false);
                if v {
                    d.insert_temp(scroll_top_id, false);
                }
                v
            }) {
                ui.scroll_to_rect(Rect::ZERO, None);
            }

            ui.add_space(20.0);
            let avail = ui.available_width();
            let col = env.typo.width.min(avail).max(300.0);
            let side = ((avail - col) / 2.0).max(0.0).floor();
            ui.horizontal(|ui| {
                ui.add_space(side);
                ui.vertical(|ui| {
                    ui.set_max_width(col);
                    for (idx, block) in doc.blocks.iter().enumerate() {
                        events.extend(show_block(ui, idx, block, env));
                    }
                });
            });
            ui.add_space(40.0);

            // Прогресс прокрутки: насколько далеко от начала документа.
            let content_h = ui.min_rect().height().max(1.0);
            let scrollable = (content_h - viewport.height()).max(0.0);
            let scrolled = viewport.min.y.min(scrollable);
            scrolled / scrollable.max(1.0)
        });

    let progress = out.inner;
    if progress > 0.001 {
        show_fab(ui, progress, scroll_top_id);
    }
    events
}

/// Круглая кнопка «наверх» с индикатором прогресса чтения по обводке.
fn show_fab(ui: &mut Ui, progress: f32, scroll_top_id: Id) {
    let bounds = ui.max_rect();
    let min = pos2(
        bounds.max.x - FAB_MARGIN - FAB_SIZE,
        bounds.max.y - FAB_MARGIN - FAB_SIZE,
    );
    let rect = Rect::from_min_size(min, Vec2::splat(FAB_SIZE));
    let resp = ui.interact(rect, ui.scope_id().with("fab"), Sense::click());
    let painter = ui.painter().clone();
    let center = rect.center();
    let radius = FAB_SIZE * 0.5;

    let weak = ui.style().visuals.weak_text_color();
    let strong = ui.style().visuals.strong_text_color();
    let panel_fill = ui.style().visuals.panel_fill;

    // Фон и «спокойная» обводка.
    painter.circle(center, radius, panel_fill, Stroke::new(1.0, weak));
    // Обводка-прогресс: ярко-синяя дуга, полный круг в самом низу.
    let accent = Color32::from_rgb(64, 140, 255);
    let a0 = -std::f32::consts::FRAC_PI_2;
    let sweep = std::f32::consts::TAU * progress.clamp(0.0, 1.0);
    let steps = ((progress * 64.0).ceil() as usize).max(2);
    let pts: Vec<egui::Pos2> = (0..=steps)
        .map(|i| {
            let a = a0 + sweep * (i as f32 / steps as f32);
            pos2(center.x + radius * a.cos(), center.y + radius * a.sin())
        })
        .collect();
    painter.add(egui::Shape::line(pts, Stroke::new(3.0, accent)));

    // Стрелка вверх.
    let s = radius * 0.42;
    painter.line_segment(
        [
            pos2(center.x - s, center.y + s * 0.4),
            pos2(center.x, center.y - s * 0.6),
        ],
        Stroke::new(2.5, strong),
    );
    painter.line_segment(
        [
            pos2(center.x, center.y - s * 0.6),
            pos2(center.x + s, center.y + s * 0.4),
        ],
        Stroke::new(2.5, strong),
    );

    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        painter.circle(
            center,
            radius,
            Color32::from_white_alpha(10),
            Stroke::NONE,
        );
    }
    if resp.clicked() {
        ui.ctx().data_mut(|d| d.insert_temp(scroll_top_id, true));
    }
}
