//! Область документа: скролл + центрированная колонка типографики.
//! Этап 0 — наивный рендер всех блоков; этап 1 заменит на виртуализацию.

use eframe::egui::{self, pos2, Color32, Id, Rect, ScrollArea, Sense, Stroke, Ui, Vec2};

use crate::md::blocks::Doc;

use super::render_block::show_block;
use super::themes::lighten;
use super::Env;

/// Событие из документа (клик по ссылке и т.п.).
#[derive(Clone, Debug)]
pub enum DocEvent {
    OpenLink(String),
}

const FAB_SIZE: f32 = 44.0;
const FAB_MARGIN_BOTTOM: f32 = 20.0;
/// Справа — с учётом ширины скроллбара, чтобы зазор был как снизу.
const FAB_MARGIN_RIGHT: f32 = FAB_MARGIN_BOTTOM + 26.0;
const FAB_BG: Color32 = Color32::from_rgb(0x22, 0x22, 0x26);
const FAB_CHEVRON: Color32 = Color32::from_rgb(0x6D, 0x6D, 0x6C);
const FAB_ACCENT: Color32 = Color32::from_rgb(64, 140, 255);

pub fn show(ui: &mut Ui, doc: &Doc, env: &Env) -> Vec<DocEvent> {
    let mut events = Vec::new();
    let scroll_top_id = ui.scope_id().with("scroll_top");

    let out = ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_viewport(ui, |ui, viewport| {
            // Просьба «наверх»: скроллируем к курсору = началу контента
            // (scroll_to_rect ждёт экранные координаты, поэтому не подходит).
            if ui.ctx().data_mut(|d| {
                let v = d.get_temp::<bool>(scroll_top_id).unwrap_or(false);
                if v {
                    d.insert_temp(scroll_top_id, false);
                }
                v
            }) {
                ui.scroll_to_cursor(Some(egui::Align::Min));
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
