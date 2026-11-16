//! Экран приветствия: показывается, когда нет открытых файлов.

use eframe::egui::{self, vec2, Align, Color32, CornerRadius, FontFamily, Layout, Sense, Stroke, StrokeKind, Ui};

use crate::fonts;
use crate::i18n::{t, Lang};
use crate::view::themes::{lighten, Palette};
use crate::view::widgets::plain_galley;

/// Возвращает true, если пользователь нажал «Открыть файл…».
/// `em` — размер UI-шрифта из настроек (масштабирует весь экран).
pub fn show(ui: &mut Ui, lang: Lang, pal: &Palette, em: f32) -> bool {
    let mut open_clicked = false;
    ui.with_layout(Layout::top_down(Align::Center), |ui| {
        ui.add_space(ui.available_height() * 0.22);
        ui.label(
            egui::RichText::new(t(lang, "welcome_title"))
                .family(FontFamily::Name(fonts::SERIF.into()))
                .size(em * 3.0)
                .strong(),
        );
        ui.add_space(10.0);
        ui.label(egui::RichText::new(t(lang, "welcome_hint")).weak().size(em));
        ui.add_space(26.0);

        // Плоская кнопка «Открыть файл…»: рамка, при наведении — заливка.
        let g = plain_galley(ui, t(lang, "open_file"), em, pal.fg);
        let size = g.size() + vec2(em * 2.4, em * 1.2);
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
        let painter = ui.painter().clone();
        let fill = if resp.hovered() {
            lighten(pal.bg, 0.07)
        } else {
            Color32::TRANSPARENT
        };
        let border = if resp.hovered() {
            pal.muted
        } else {
            pal.border
        };
        painter.rect(rect, CornerRadius::same(0), fill, Stroke::new(1.0, border), StrokeKind::Middle);
        painter.galley(rect.center() - g.size() * 0.5, g, pal.fg);
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if resp.clicked() {
            open_clicked = true;
        }
    });
    open_clicked
}
