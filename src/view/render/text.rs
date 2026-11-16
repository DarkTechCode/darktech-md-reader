//! Текстовые блоки: декорации из раскладки + выделяемый Label со ссылками.

use std::sync::Arc;

use eframe::egui::{
    self as egui, pos2, vec2, Color32, CornerRadius, FontFamily, FontId, Rect, Stroke, StrokeKind,
    Ui,
};
use eframe::egui::text::{ByteIndex, CCursor, Galley, LayoutJob, LayoutSection, TextFormat};

use crate::layout::measure::{JobSpec, Marker, TextPrep};
use crate::md::blocks::Span;

use super::misc::draw_checkbox;
use super::super::doc_view::DocEvent;
use super::super::themes::Palette;
use super::super::Env;

/// Рисует подготовленный текстовый блок: фон/полосу цитаты, маркер, текст.
pub fn render_text(
    ui: &mut Ui,
    rect: Rect,
    spans: &[Span],
    prep: &TextPrep,
    env: &Env,
) -> Vec<DocEvent> {
    let painter = ui.painter().clone();
    let o = rect.min;

    if let Some(bg) = prep.alert_bg {
        painter.rect(
            bg.translate(o.to_vec2()),
            CornerRadius::same(4),
            env.pal.quote_bg,
            Stroke::NONE,
            StrokeKind::Middle,
        );
    }
    if let Some((bar, color)) = prep.quote_bar {
        painter.rect_filled(
            bar.translate(o.to_vec2()),
            CornerRadius::same(2),
            color,
        );
    }
    if let Some((g, pos, color)) = &prep.alert_label {
        painter.galley(o + *pos, g.clone(), *color);
    }
    match &prep.marker {
        Some(Marker::Text(g, pos, color)) => {
            painter.galley(o + *pos, g.clone(), *color);
        }
        Some(Marker::Checkbox(checked, pos)) => draw_checkbox(&painter, o + *pos, *checked, env),
        None => {}
    }
    if let Some((y, w)) = prep.underline {
        painter.line_segment(
            [pos2(o.x, o.y + y), pos2(o.x + w, o.y + y)],
            Stroke::new(1.0, env.pal.border),
        );
    }

    let label_rect = Rect::from_min_size(o + prep.text_pos, prep.galley.size());
    put_text(
        ui,
        label_rect,
        &prep.galley,
        spans,
        &prep.spec,
        env.pal,
        &prep.char_starts,
    )
}

/// Кладёт выделяемый Label с поддержкой ссылок: hover (цвет+курсор+подсказка)
/// и клик. Возвращает события открытых ссылок.
fn put_text(
    ui: &mut Ui,
    label_rect: Rect,
    galley: &Arc<Galley>,
    spans: &[Span],
    spec: &JobSpec,
    pal: &Palette,
    job_text_char_starts: &[usize],
) -> Vec<DocEvent> {
    let has_links = spans.iter().any(|s| s.link.is_some());
    let mut hover: Option<(String, Rect)> = None;

    if has_links {
        if let Some(p) = ui.input(|i| i.pointer.hover_pos()) {
            // Прямоугольник каждого ссылочного спана — объединение курсоров начала/конца.
            for (i, span) in spans.iter().enumerate() {
                let Some(url) = span.link.as_deref() else { continue };
                let cs = job_text_char_starts[i];
                let ce = cs + span.text.chars().count();
                let r1 = galley.pos_from_cursor(CCursor::new(cs));
                let r2 = galley.pos_from_cursor(CCursor::new(ce));
                let r = r1.union(r2).translate(label_rect.min.to_vec2());
                if r.contains(p) {
                    hover = Some((url.to_string(), r));
                    break;
                }
            }
        }
    }

    let mut events = Vec::new();
    if let Some((url, r)) = &hover {
        // Пересобираем job с подсветкой ссылки и кладём ГОТОВЫЙ galley:
        // Label::new(Arc<Galley>) рендерит его как есть (ui.put создал бы
        // centered_and_justified-ско́п и перекла́дывал job — позиции разъехались
        // бы с хит-тестом). Ширины не меняются — раскладка та же.
        let g = ui.painter().layout_job(spec.job(spans, pal, Some(url)));
        ui.put(label_rect, egui::Label::new(g));
        draw_url_tooltip(ui, url);
        // Курсор ставим ПОСЛЕ Label, иначе перебьётся курсором выделения текста.
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        if ui.input(|i| i.pointer.primary_clicked())
            && r.contains(ui.input(|i| i.pointer.hover_pos()).unwrap())
        {
            events.push(DocEvent::OpenLink(url.clone()));
        }
    } else {
        // Тот же galley, что прошёл хит-тест: выравнивание по левому краю
        // (job.halign = LEFT по умолчанию), без justify.
        ui.put(label_rect, egui::Label::new(galley.clone()));
    }
    events
}

/// Компактная подсказка с URL у курсора.
fn draw_url_tooltip(ui: &Ui, url: &str) {
    let Some(p) = ui.input(|i| i.pointer.hover_pos()) else { return };
    let galley = ui
        .painter()
        .layout_job(plain_job(url, 12.5, ui.style().visuals.strong_text_color()));
    let pad = 7.0;
    let size = galley.size() + vec2(pad * 2.0, pad * 2.0);
    let mut min = p + vec2(12.0, -size.y - 8.0);
    let clip = ui.clip_rect();
    min.x = min.x.min(clip.right() - size.x - 4.0).max(clip.left() + 4.0);
    if min.y < clip.top() + 4.0 {
        min.y = p.y + 16.0;
    }
    let rect = Rect::from_min_size(min, size);
    let painter = ui.painter().clone();
    painter.rect(
        rect,
        CornerRadius::same(4),
        ui.style().visuals.window_fill,
        Stroke::new(1.0, ui.style().visuals.weak_text_color()),
        StrokeKind::Middle,
    );
    painter.galley(
        rect.min + vec2(pad, pad),
        galley,
        ui.style().visuals.strong_text_color(),
    );
}

fn plain_job(text: &str, size: f32, color: Color32) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.sections.push(LayoutSection {
        leading_space: 0.0,
        byte_range: ByteIndex(0)..ByteIndex(text.len()),
        format: TextFormat {
            font_id: FontId::new(size, FontFamily::Monospace),
            color,
            ..Default::default()
        },
    });
    job.text.push_str(text);
    job
}
