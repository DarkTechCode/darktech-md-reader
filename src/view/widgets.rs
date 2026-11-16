//! Плоские UI-примитивы (без скруглений и рамок egui-кнопок).

use eframe::egui::{Color32, Sense, Stroke, StrokeKind, Ui};
use eframe::egui::text::{ByteIndex, Galley, LayoutJob, LayoutSection, TextFormat};

fn single_span_job(text: &str, size: f32, color: Color32, mono: bool) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.sections.push(LayoutSection {
        leading_space: 0.0,
        byte_range: ByteIndex(0)..ByteIndex(text.len()),
        format: TextFormat {
            font_id: eframe::egui::FontId::new(
                size,
                if mono {
                    eframe::egui::FontFamily::Monospace
                } else {
                    eframe::egui::FontFamily::Proportional
                },
            ),
            color,
            ..Default::default()
        },
    });
    job.text.push_str(text);
    job
}

/// Готовый galley одного спана (Proportional), без выделения места в layout.
pub fn plain_galley(ui: &Ui, text: &str, size: f32, color: Color32) -> std::sync::Arc<Galley> {
    ui.painter().layout_job(single_span_job(text, size, color, false))
}

/// Galley с переносом слов по ширине (для оглавления).
pub fn wrapped_galley(
    ui: &Ui,
    text: &str,
    size: f32,
    color: Color32,
    max_width: f32,
) -> std::sync::Arc<Galley> {
    let mut job = single_span_job(text, size, color, false);
    job.wrap = eframe::egui::text::TextWrapping {
        max_width: max_width.max(20.0),
        ..Default::default()
    };
    ui.painter().layout_job(job)
}

/// Текстовая кнопка: меняет цвет при наведении, без фона и скруглений.
pub fn flat_text_button(
    ui: &mut Ui,
    text: &str,
    size: f32,
    color: Color32,
    hover: Color32,
) -> bool {
    let galley = ui.painter().layout_job(single_span_job(text, size, color, false));
    let (rect, resp) =
        ui.allocate_exact_size(galley.size() + eframe::egui::vec2(10.0, 4.0), Sense::click());
    let c = if resp.hovered() { hover } else { color };
    if resp.hovered() {
        ui.ctx()
            .set_cursor_icon(eframe::egui::CursorIcon::PointingHand);
    }
    ui.painter().galley(rect.center() - galley.size() * 0.5, galley, c);
    resp.clicked()
}

/// Кнопка-пилюля с рамкой (внутри блока кода): при наведении подсвечивается.
pub fn flat_pill_button(
    ui: &mut Ui,
    text: &str,
    size: f32,
    pal_color: Color32,
    pal_hover: Color32,
    border: Color32,
    hover_fill: Color32,
) -> bool {
    let galley = ui.painter().layout_job(single_span_job(text, size, pal_color, true));
    let (rect, resp) = ui.allocate_exact_size(
        galley.size() + eframe::egui::vec2(16.0, 6.0),
        Sense::click(),
    );
    let hovered = resp.hovered();
    let fill = if hovered { hover_fill } else { Color32::TRANSPARENT };
    let color = if hovered { pal_hover } else { pal_color };
    let painter = ui.painter().clone();
    painter.rect(rect, eframe::egui::CornerRadius::same(0), fill, Stroke::new(1.0, border), StrokeKind::Middle);
    painter.galley(rect.center() - galley.size() * 0.5, galley, color);
    if hovered {
        ui.ctx()
            .set_cursor_icon(eframe::egui::CursorIcon::PointingHand);
    }
    resp.clicked()
}
