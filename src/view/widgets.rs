//! Плоские UI-примитивы (без скруглений и рамок egui-кнопок).

use eframe::egui::{Color32, Sense, Ui};

/// Текстовая кнопка: меняет цвет при наведении, без фона и скруглений.
pub fn flat_text_button(
    ui: &mut Ui,
    text: &str,
    size: f32,
    color: Color32,
    hover: Color32,
) -> bool {
    let mut job = eframe::egui::text::LayoutJob::default();
    job.sections.push(eframe::egui::text::LayoutSection {
        leading_space: 0.0,
        byte_range: eframe::egui::text::ByteIndex(0)
            ..eframe::egui::text::ByteIndex(text.len()),
        format: eframe::egui::text::TextFormat {
            font_id: eframe::egui::FontId::new(size, eframe::egui::FontFamily::Proportional),
            color,
            ..Default::default()
        },
    });
    job.text.push_str(text);
    let galley = ui.painter().layout_job(job);
    let (rect, resp) = ui.allocate_exact_size(galley.size() + eframe::egui::vec2(10.0, 4.0), Sense::click());
    let c = if resp.hovered() { hover } else { color };
    if resp.hovered() {
        ui.ctx().set_cursor_icon(eframe::egui::CursorIcon::PointingHand);
    }
    ui.painter()
        .galley(rect.center() - galley.size() * 0.5, galley, c);
    resp.clicked()
}
