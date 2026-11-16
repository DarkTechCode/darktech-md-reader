//! Блок кода: шапка (язык + «Копировать»), фон на всю ширину,
//! из строк рисуются только видимые.

use std::time::Duration;

use eframe::egui::{
    self as egui, pos2, vec2, Color32, CornerRadius, FontFamily, Rect, ScrollArea, Sense, Stroke,
    StrokeKind, Ui, UiBuilder,
};

use crate::i18n::t;
use crate::layout::measure::{plain_job_mono, plain_span, CodePrep, JobSpec};

use super::super::doc_view::DocEvent;
use super::super::themes::lighten;
use super::super::Env;

/// Сколько секунд показывать «Скопировано!» после клика.
const COPIED_SECS: f64 = 3.0;

pub fn render_code(
    ui: &mut Ui,
    rect: Rect,
    vis: Rect,
    idx: usize,
    code: &str,
    prep: &CodePrep,
    env: &Env,
) -> (Vec<DocEvent>, Option<f32>) {
    let em = prep.em;
    let gap = crate::layout::measure::gap_code(em);
    let pad = prep.pad;
    let full_w = rect.width();

    // Шапка блока: подпись языка слева, кнопка «копировать» справа — внутри блока.
    let hrect = Rect::from_min_size(rect.min, vec2(full_w, prep.header_h));
    let painter = ui.painter().clone();
    painter.rect(
        hrect,
        CornerRadius::same(0),
        env.pal.code_bg,
        Stroke::NONE,
        StrokeKind::Middle,
    );
    if let Some(label) = &prep.label {
        let g = painter.layout_job(plain_job_mono(label, em * 0.78, env.pal.muted));
        painter.galley(
            pos2(hrect.min.x + pad, hrect.center().y - g.size().y * 0.5),
            g,
            env.pal.muted,
        );
    }
    draw_copy_button(ui, hrect, idx, code, em, pad, env);

    // Область кода: горизонтальный скролл для широких строк.
    let top = rect.min.y + gap + prep.header_h;
    let area_h = (rect.height() - gap * 2.0 - prep.header_h).max(0.0);
    let area = Rect::from_min_size(pos2(rect.min.x, top), vec2(full_w, area_h));
    ui.scope_builder(UiBuilder::new().max_rect(area), |ui| {
        ScrollArea::horizontal()
            .id_salt(("code_h", idx))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let code_h = prep.height - gap * 2.0 - prep.header_h - prep.pad * 2.0
                    - prep.bar_space;
                let content_h = code_h + pad * 2.0;
                let (crect, _) =
                    ui.allocate_exact_size(vec2(prep.content_w, content_h), Sense::hover());
                let painter = ui.painter();
                // Фон тянется шире контента, чтобы не «уезжал» при горизонтальном скролле.
                painter.rect(
                    Rect::from_min_max(
                        pos2(crect.min.x - 100_000.0, crect.min.y),
                        pos2(crect.max.x + 100_000.0, crect.max.y),
                    ),
                    CornerRadius::same(0),
                    env.pal.code_bg,
                    Stroke::NONE,
                    StrokeKind::Middle,
                );

                // Видимое окно строк: пустое, если вьюпорт ушёл ниже кода.
                let n = prep.line_starts.len();
                let text_top = crect.min.y + pad;
                let a = (((vis.min.y - text_top) / prep.line_h).floor().max(0.0) as usize).min(n);
                let b =
                    (((vis.max.y - text_top) / prep.line_h).ceil().max(0.0) as usize).clamp(a, n);
                let end = if b < n {
                    prep.line_starts[b] as usize
                } else {
                    code.len()
                };
                let slice = if a < n {
                    &code[prep.line_starts[a] as usize..end]
                } else {
                    ""
                };
                if !slice.is_empty() {
                    let spec = JobSpec {
                        size: em * 0.84,
                        family: FontFamily::Name(crate::fonts::MONO.into()),
                        color: env.pal.fg,
                        bold: false,
                        wrap: f32::INFINITY,
                        line_height: prep.line_h,
                        letter_spacing: env.typo.letter_spacing,
                    };
                    // Готовый galley — без переразметки Label'ом (левый край).
                    let g = ui.painter().layout_job(spec.job(&[plain_span(slice)], env.pal, None));
                    let label_rect = Rect::from_min_size(
                        pos2(crect.min.x + pad, text_top + a as f32 * prep.line_h),
                        g.size(),
                    );
                    ui.put(label_rect, egui::Label::new(g));
                }
            });
    });
    (Vec::new(), None)
}

/// Кнопка «Копировать» / «Скопировано!» (3 с после клика).
fn draw_copy_button(
    ui: &mut Ui,
    hrect: Rect,
    idx: usize,
    code: &str,
    em: f32,
    pad: f32,
    env: &Env,
) {
    let painter = ui.painter().clone();
    let copied_key = ui.scope_id().with(("code_copied", idx));
    let now = ui.ctx().time();
    let copied_until: f64 = ui
        .ctx()
        .data_mut(|d| d.get_temp(copied_key).unwrap_or(0.0));
    let copied = now < copied_until;
    if copied {
        let left = (copied_until - now).max(0.05);
        ui.ctx().request_repaint_after(Duration::from_secs_f64(left));
    }

    let text = if copied {
        t(env.lang, "copied")
    } else {
        t(env.lang, "copy")
    };
    let btn_g = painter.layout_job(plain_job_mono(text, em * 0.72, env.pal.fg));
    let btn_size = btn_g.size() + vec2(16.0, 6.0);
    let btn_rect = Rect::from_min_size(
        pos2(
            hrect.max.x - pad - btn_size.x,
            hrect.center().y - btn_size.y * 0.5,
        ),
        btn_size,
    );
    let btn_resp = ui.interact(
        btn_rect,
        ui.scope_id().with(("code_copy", idx)),
        Sense::click(),
    );
    let hovered = btn_resp.hovered();
    painter.rect(
        btn_rect,
        CornerRadius::same(0),
        if hovered && !copied {
            lighten(env.pal.code_bg, 0.18)
        } else {
            Color32::TRANSPARENT
        },
        Stroke::new(1.0, env.pal.border),
        StrokeKind::Middle,
    );
    painter.galley(
        btn_rect.center() - btn_g.size() * 0.5,
        btn_g,
        if hovered { env.pal.fg } else { env.pal.muted },
    );
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if btn_resp.clicked() {
        // Только фон: синхронная запись может блокироваться (см. clipboard.rs).
        crate::clipboard::copy_in_background(code.to_string());
        // ВАЖНО: time() ДО data_mut — time() внутри замыкания берёт тот же
        // write-lock Context (parking_lot, не реентерабельный) → само-дедлок.
        let until = now + COPIED_SECS;
        ui.ctx().data_mut(|d| d.insert_temp(copied_key, until));
    }
}
