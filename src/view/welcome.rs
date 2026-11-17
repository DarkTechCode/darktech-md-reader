//! Экран приветствия: показывается, когда нет открытых файлов. Кнопка
//! «Открыть файл…» и список последних файлов (сессия в settings.json).

use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui::{
    self, pos2, vec2, Align, Color32, CornerRadius, FontFamily, Layout, Sense, Stroke,
    StrokeKind, Ui,
};

use crate::fonts;
use crate::i18n::{t, Lang};
use crate::view::themes::{lighten, Palette};
use crate::view::widgets::plain_galley;

/// Действие на welcome-экране.
pub enum Action {
    /// «Открыть файл…» — диалог выбора файла.
    Dialog,
    /// Клик по файлу из «Последних».
    Recent(PathBuf),
}

/// `em` — размер UI-шрифта из настроек (масштабирует весь экран).
pub fn show(ui: &mut Ui, lang: Lang, pal: &Palette, em: f32, recent: &[PathBuf]) -> Option<Action> {
    let mut action = None;
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
            action = Some(Action::Dialog);
        }

        // Последние файлы: плоские строки-кнопки (имя + каталог приглушённо).
        if !recent.is_empty() {
            ui.add_space(34.0);
            let hg = plain_galley(ui, t(lang, "welcome_recent"), em * 0.95, pal.muted);
            let (hr, _) = ui.allocate_exact_size(vec2(hg.size().x, hg.size().y + 6.0), Sense::hover());
            ui.painter().galley(
                pos2(hr.min.x, hr.center().y - hg.size().y * 0.5),
                hg,
                pal.muted,
            );

            ui.add_space(6.0);
            let rows: Vec<(Arc<egui::Galley>, Arc<egui::Galley>)> = recent
                .iter()
                .map(|p| {
                    let name = p
                        .file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| p.to_string_lossy().into_owned());
                    let dir = p.parent().map(|d| d.display().to_string()).unwrap_or_default();
                    (
                        plain_galley(ui, &name, em, pal.fg),
                        plain_galley(ui, &dir, em * 0.85, pal.faint),
                    )
                })
                .collect();
            let text_w = rows
                .iter()
                .map(|(n, d)| n.size().x + if d.is_empty() { 0.0 } else { 12.0 + d.size().x })
                .fold(0.0_f32, f32::max);
            let block_w = (text_w + em * 2.0).min(640.0);
            for (i, (name, dir)) in rows.iter().enumerate() {
                let h = (em * 1.9).max(30.0);
                let (rect, resp) = ui.allocate_exact_size(vec2(block_w, h), Sense::click());
                let painter = ui.painter().clone();
                let hovered = resp.hovered();
                if hovered {
                    painter.rect_filled(rect, CornerRadius::same(0), lighten(pal.bg, 0.06));
                }
                let cy = rect.center().y;
                let mut x = rect.min.x + em;
                let max_x = rect.max.x - em;
                ui.painter().galley(
                    pos2(x, cy - name.size().y * 0.5),
                    name.clone(),
                    pal.fg,
                );
                x += name.size().x + 12.0;
                // Каталог рисуем, только если влезает целиком.
                if !dir.is_empty() && x + dir.size().x <= max_x {
                    ui.painter().galley(
                        pos2(x, cy - dir.size().y * 0.5),
                        dir.clone(),
                        pal.faint,
                    );
                }
                if hovered {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if resp.clicked() && action.is_none() {
                    action = Some(Action::Recent(recent[i].clone()));
                }
            }
        }
    });
    action
}
