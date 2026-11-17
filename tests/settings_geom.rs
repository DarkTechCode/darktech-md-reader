//! Панель настроек имеет точную ширину — контент обязан в неё влезать:
//! горизонтальный поток egui НЕ переносится, и вылезшие строки (например,
//! ряд кнопок) обрезались бы краем окна («не видно все настройки»).
//! Регрессионный тест: min_rect контента панели не шире exact_size.

use eframe::egui;
use darktech_md::fonts;
use darktech_md::settings::model::SettingsData;
use darktech_md::view::settings_panel;
use darktech_md::view::themes;

#[test]
fn settings_panel_content_fits_exact_width() {
    let ctx = egui::Context::default();
    fonts::install(&ctx);
    // 14 — дефолт UI-шрифта (панель 430), 18 — крупный UI (панель 553).
    for ui_font in [14u8, 18] {
        let mut s = SettingsData::default();
        s.ui_font_size = ui_font;
        let scale = s.ui_font_size as f32 / 14.0;
        let panel_w = (430.0 * scale).clamp(360.0, 640.0);
        let mut input = egui::RawInput::default();
        input.screen_rect = Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(1100.0, 800.0),
        ));
        let mut out = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ui, |ui| {
                    let pal = &themes::DARK;
                    let mut min_rect = egui::Rect::NOTHING;
                    egui::Panel::right("settings_panel")
                        .resizable(false)
                        .exact_size(panel_w)
                        .frame(egui::Frame::NONE.fill(pal.code_bg))
                        .show(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .id_salt("settings_scroll")
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    settings_panel::show(
                                        ui,
                                        &mut s,
                                        pal,
                                        darktech_md::i18n::Lang::Ru,
                                    );
                                });
                            min_rect = ui.min_rect();
                        });
                    assert!(
                        min_rect.width() <= panel_w + 0.5,
                        "ui_font {ui_font}: контент панели {} шире exact_size {panel_w:.0}",
                        min_rect.width()
                    );
                });
        });
        out.textures_delta.clear();
    }
}
