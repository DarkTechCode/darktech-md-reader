//! Экран приветствия: показывается, когда нет открытых файлов.

use eframe::egui::{self, Align, FontFamily, Layout, Ui};

use crate::fonts;
use crate::i18n::{t, Lang};

/// Возвращает true, если пользователь нажал «Открыть файл…».
pub fn show(ui: &mut Ui, lang: Lang) -> bool {
    let mut open_clicked = false;
    ui.with_layout(Layout::top_down(Align::Center), |ui| {
        ui.add_space(ui.available_height() * 0.22);
        ui.label(
            egui::RichText::new(t(lang, "welcome_title"))
                .family(FontFamily::Name(fonts::SERIF.into()))
                .size(42.0)
                .strong(),
        );
        ui.add_space(10.0);
        ui.label(egui::RichText::new(t(lang, "welcome_hint")).weak());
        ui.add_space(26.0);
        if ui
            .button(t(lang, "open_file"))
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            open_clicked = true;
        }
    });
    open_clicked
}
