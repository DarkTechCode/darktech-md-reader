//! Таблица: колонки по измеренной раскладке, строки рисуются/измеряются
//! только видимые (высоты копятся в TablePrep).

use eframe::egui::{pos2, vec2, CornerRadius, Rect, ScrollArea, Sense, Stroke, Ui, UiBuilder};

use crate::layout::measure::TablePrep;
use crate::md::blocks::{Align as MdAlign, TableRow};

use super::super::doc_view::DocEvent;
use super::super::Env;

pub fn render_table(
    ui: &mut Ui,
    rect: Rect,
    vis: Rect,
    // id скролла уникален через id блока; idx оставлен для единообразия диспетчера.
    _idx: usize,
    aligns: &[MdAlign],
    rows: &[TableRow],
    prep: &mut TablePrep,
    env: &Env,
) -> (Vec<DocEvent>, Option<f32>) {
    let pad_h = prep.pad_h;
    let pad_v = prep.pad_v;

    let top = rect.min.y + prep.gap_top;
    let content_h = (rect.height() - prep.gap_top - prep.gap_bottom).max(0.0);
    let area = Rect::from_min_size(pos2(rect.min.x, top), vec2(rect.width(), content_h));
    let new_height = ui
        .scope_builder(UiBuilder::new().max_rect(area), |ui| {
            scroll_rows(ui, vis, aligns, rows, prep, pad_h, pad_v, content_h, env)
        })
        .inner;

    (Vec::new(), new_height)
}

#[allow(clippy::too_many_arguments)]
fn scroll_rows(
    ui: &mut Ui,
    vis: Rect,
    aligns: &[MdAlign],
    rows: &[TableRow],
    prep: &mut TablePrep,
    pad_h: f32,
    pad_v: f32,
    content_h: f32,
    env: &Env,
) -> Option<f32> {
    ScrollArea::horizontal()
        .id_salt("table_rows")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let (crect, _) =
                ui.allocate_exact_size(vec2(prep.total_w, content_h), Sense::hover());
            let painter = ui.painter().clone();
            let mut y = crect.min.y;
            let mut changed = false;

            for (ri, row) in rows.iter().enumerate() {
                let mut rh = prep.row_height(ri);
                let row_top = y;
                y += rh;
                // Измеряем и рисуем только строки в видимом окне.
                if row_top + rh <= vis.min.y || row_top >= vis.max.y {
                    continue;
                }

                let mut galleys = Vec::with_capacity(row.cells.len());
                let mut row_h = 0.0f32;
                for (ci, cell) in row.cells.iter().enumerate() {
                    let mut spec = prep.cell.clone();
                    spec.bold = ri == 0;
                    spec.wrap = prep.col_w.get(ci).copied().unwrap_or(300.0) - pad_h * 2.0;
                    let job = spec.job(cell, env.pal, None);
                    let g = ui.painter().layout_job(job);
                    row_h = row_h.max(g.size().y + pad_v * 2.0);
                    galleys.push(g);
                }
                rh = row_h;
                if prep.row_h[ri] == 0.0 || (prep.row_h[ri] - rh).abs() > 0.01 {
                    prep.row_h[ri] = rh;
                    changed = true;
                }

                if ri == 0 {
                    painter.rect_filled(
                        Rect::from_min_max(
                            pos2(crect.min.x, row_top),
                            pos2(crect.min.x + prep.total_w, row_top + rh),
                        ),
                        CornerRadius {
                            nw: 6,
                            ne: 6,
                            sw: 0,
                            se: 0,
                        },
                        env.pal.table_head_bg,
                    );
                }
                let mut x = crect.min.x;
                for (ci, g) in galleys.iter().enumerate() {
                    let cw = prep.col_w.get(ci).copied().unwrap_or(300.0);
                    let align = aligns.get(ci).copied().unwrap_or(MdAlign::None);
                    let tx = match align {
                        MdAlign::Right => x + cw - pad_h - g.size().x,
                        MdAlign::Center => x + (cw - g.size().x) / 2.0,
                        _ => x + pad_h,
                    };
                    let ty = if env.table_top {
                        row_top + pad_v
                    } else {
                        row_top + (rh - g.size().y) / 2.0
                    };
                    let cell_rect = Rect::from_min_size(pos2(tx, ty), g.size());
                    ui.put(cell_rect, eframe::egui::Label::new(g.clone()));
                    x += cw;
                }
                painter.line_segment(
                    [
                        pos2(crect.min.x, row_top + rh),
                        pos2(crect.min.x + prep.total_w, row_top + rh),
                    ],
                    Stroke::new(1.0, env.pal.border),
                );
            }

            if changed {
                prep.recompute_height();
                Some(prep.height)
            } else {
                None
            }
        })
        .inner
}
