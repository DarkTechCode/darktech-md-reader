//! Блок кода: цельная «карточка» на общем фоне (шапка с языком и «Копировать»
//! сверху, строки кода, кнопка «Копировать» снизу), нумерация строк и
//! подсветка синтаксиса — только для видимого среза строк.

use std::time::Duration;

use eframe::egui::{
    self as egui, pos2, vec2, Color32, CornerRadius, FontFamily, Rect, ScrollArea, Sense, Stroke,
    StrokeKind, Ui, UiBuilder,
};

use eframe::egui::text::{ByteIndex, LayoutJob, LayoutSection, TextFormat};

use crate::highlight;
use crate::i18n::t;
use crate::layout::measure::{
    gap_code_bottom, gap_code_top, plain_job_mono, plain_span, CodePrep, JobSpec, CODE_FOOTER_H,
};

use super::super::doc_view::DocEvent;
use super::super::selection::SelFrame;
use super::super::Env;
use super::text::{place_text, Place};

/// Сколько секунд показывать «Скопировано!» после клика.
const COPIED_SECS: f64 = 3.0;

pub fn render_code(
    ui: &mut Ui,
    rect: Rect,
    vis: Rect,
    idx: usize,
    lang: &Option<String>,
    code: &str,
    prep: &CodePrep,
    env: &Env,
    frame: &mut SelFrame,
) -> (Vec<DocEvent>, Option<f32>) {
    let em = prep.em;
    let pad = prep.pad;
    let gap_top = gap_code_top(em);
    let gap_bot = gap_code_bottom(em);
    let full_w = rect.width();

    // Цельный фон карточки: от верхнего отступа до нижнего, на всю ширину.
    let card = Rect::from_min_max(
        pos2(rect.min.x, rect.min.y + gap_top),
        pos2(rect.max.x, rect.max.y - gap_bot),
    );
    let painter = ui.painter().clone();
    painter.rect(card, CornerRadius::same(0), env.pal.code_bg, Stroke::NONE, StrokeKind::Middle);

    // Шапка: подпись языка слева, «Копировать» справа.
    let hrect = Rect::from_min_size(card.min, vec2(full_w, prep.header_h));
    if let Some(label) = &prep.label {
        let g = painter.layout_job(plain_job_mono(label, em * 0.78, env.pal.muted));
        painter.galley(
            pos2(hrect.min.x + pad, hrect.center().y - g.size().y * 0.5),
            g,
            env.pal.muted,
        );
    }
    draw_copy_button(ui, hrect, ("code_copy_t", idx), code, em, pad, env);

    // Тело кода: горизонтальный скролл для широких строк.
    let body = Rect::from_min_max(
        pos2(card.min.x, card.min.y + prep.header_h),
        pos2(card.max.x, card.max.y - CODE_FOOTER_H - prep.bar_space),
    );
    let lines_area_h = body.height() - pad * 2.0;
    let mut evs = Vec::new();
    ui.scope_builder(UiBuilder::new().max_rect(body), |ui| {
        ScrollArea::horizontal()
            .id_salt(("code_h", idx))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let (crect, _) =
                    ui.allocate_exact_size(vec2(prep.content_w, lines_area_h), Sense::hover());
                let painter = ui.painter();

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

                // Номера строк (видимых): вправо внутри колонки номеров,
                // до кода — 20px (единицы выровнены по одной вертикали).
                if prep.gutter_w > 0.0 {
                    let num_right = crect.min.x + pad + prep.gutter_w - 20.0;
                    for i in a..b {
                        let g = painter.layout_job(plain_job_mono(
                            &(i + 1).to_string(),
                            em * 0.66,
                            env.pal.faint,
                        ));
                        painter.galley(
                            pos2(
                                num_right - g.size().x,
                                text_top + i as f32 * prep.line_h
                                    + (prep.line_h - g.size().y) * 0.5,
                            ),
                            g,
                            env.pal.faint,
                        );
                    }
                }

                if !slice.is_empty() {
                    let text_x = crect.min.x + pad + prep.gutter_w;
                    // Спецификация plain-набора (для place_text; ссылок в коде нет).
                    let spec = JobSpec {
                        size: prep.em * 0.84,
                        family: FontFamily::Name(crate::fonts::MONO.into()),
                        color: env.pal.fg,
                        bold: false,
                        wrap: f32::INFINITY,
                        line_height: prep.line_h,
                        letter_spacing: env.typo.letter_spacing,
                    };
                    // Подсветка синтаксиса (кэш по видимому срезу) или plain.
                    let job = if env.code_highlight {
                        highlighted_job(ui, slice, lang, idx, prep, env)
                            .unwrap_or_else(|| plain_code_job(slice, prep, env))
                    } else {
                        plain_code_job(slice, prep, env)
                    };
                    // Готовый galley: место кладём сами (левый край + выделение).
                    let g = ui.painter().layout_job(job);
                    let label_rect = Rect::from_min_size(
                        pos2(text_x, text_top + a as f32 * prep.line_h),
                        g.size(),
                    );
                    evs.extend(place_text(
                        ui,
                        env.pal,
                        Place {
                            galley: &g,
                            rect: label_rect,
                            spans: &[],
                            spec: &spec,
                            block: idx,
                            sub: 0,
                            base_ch: prep.line_char_starts[a] as usize,
                            total_ch: prep.total_ch,
                        },
                        frame,
                    ));
                }
            });
    });

    // Нижняя полоса: дублирующая «Копировать» справа внутри карточки.
    let frect = Rect::from_min_size(
        pos2(card.min.x, card.max.y - CODE_FOOTER_H - prep.bar_space),
        vec2(full_w, CODE_FOOTER_H),
    );
    draw_copy_button(ui, frect, ("code_copy_b", idx), code, em, pad, env);

    (evs, None)
}

/// LayoutJob видимого среза кода одним куском (моно, без переноса).
fn plain_code_job(slice: &str, prep: &CodePrep, env: &Env) -> LayoutJob {
    let spec = JobSpec {
        size: prep.em * 0.84,
        family: FontFamily::Name(crate::fonts::MONO.into()),
        color: env.pal.fg,
        bold: false,
        wrap: f32::INFINITY,
        line_height: prep.line_h,
        letter_spacing: env.typo.letter_spacing,
    };
    spec.job(&[plain_span(slice)], env.pal, None)
}

/// LayoutJob из подсвеченных кусков. Результат подсветки кэшируется по
/// (блок, срез, тема) — при остановившемся скролле syntect не вызывается.
fn highlighted_job(
    ui: &mut Ui,
    slice: &str,
    lang: &Option<String>,
    idx: usize,
    prep: &CodePrep,
    env: &Env,
) -> Option<LayoutJob> {
    let lang = lang.as_deref()?;
    let dark = env.pal.bg == crate::view::themes::DARK.bg;
    let cache_key = ui.scope_id().with(("code_hl", idx));
    type Cache = (u64, u64, bool, highlight::Highlighted);
    let ptr = slice.as_ptr() as u64;
    let len = slice.len() as u64;
    let hl: highlight::Highlighted =
        match ui.ctx().data_mut(|d| d.get_temp::<Cache>(cache_key)) {
            Some((p, l, was_dark, ref hl)) if p == ptr && l == len && was_dark == dark => {
                hl.clone()
            }
            _ => {
                let hl = highlight::highlight(slice, lang, dark)?;
                ui.ctx().data_mut(|d| {
                    d.insert_temp(cache_key, (ptr, len, dark, hl.clone()))
                });
                hl
            }
        };

    let mut job = LayoutJob::default();
    let mut byte = 0usize;
    for (text, color) in hl.0 {
        let start = byte;
        byte += text.len();
        job.sections.push(LayoutSection {
            leading_space: 0.0,
            byte_range: ByteIndex(start)..ByteIndex(byte),
            format: TextFormat {
                font_id: eframe::egui::FontId::new(prep.em * 0.84, FontFamily::Name(crate::fonts::MONO.into())),
                color,
                line_height: Some(prep.line_h),
                extra_letter_spacing: env.typo.letter_spacing,
                ..Default::default()
            },
        });
        job.text.push_str(&text);
    }
    Some(job)
}

/// Кнопка «Копировать» / «Скопировано!» (3 с после клика), без рамки-пилюли —
/// строка шапки/подвала карточки кода.
fn draw_copy_button(
    ui: &mut Ui,
    host: Rect,
    salt: (&'static str, usize),
    code: &str,
    em: f32,
    pad: f32,
    env: &Env,
) {
    let painter = ui.painter().clone();
    // Обе кнопки блока делят состояние «Скопировано!» (ключ — индекс блока).
    let copied_key = ui.scope_id().with(("code_copied", salt.1));
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
            host.max.x - pad - btn_size.x,
            host.center().y - btn_size.y * 0.5,
        ),
        btn_size,
    );
    let btn_resp = ui.interact(btn_rect, ui.scope_id().with(salt), Sense::click());
    let hovered = btn_resp.hovered();
    painter.rect(
        btn_rect,
        CornerRadius::same(0),
        if hovered && !copied {
            env.pal.inline_code_bg
        } else {
            Color32::TRANSPARENT
        },
        Stroke::new(1.0, env.pal.border),
        StrokeKind::Middle,
    );
    painter.galley_with_override_text_color(
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
