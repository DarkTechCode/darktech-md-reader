//! Разделитель, картинка-плейсхолдер, чекбокс списка.

use eframe::egui::{
    pos2, vec2, Color32, CornerRadius, Painter, Pos2, Rect, Stroke, StrokeKind, Ui, Vec2,
};

use crate::layout::measure::ImagePrep;

use super::super::Env;

/// Горизонтальная линия `---`.
pub fn render_rule(ui: &mut Ui, rect: Rect, env: &Env) {
    let mid = rect.min.y + 0.8 * env.typo.size + 0.5;
    ui.painter().line_segment(
        [pos2(rect.min.x, mid), pos2(rect.max.x, mid)],
        Stroke::new(1.0, env.pal.border),
    );
}

/// Плейсхолдер локальной картинки (загрузка — этап 2–3 по плану).
pub fn render_image(ui: &mut Ui, rect: Rect, prep: &ImagePrep, env: &Env) {
    let em = env.typo.size;
    let box_top = rect.min.y + 0.4 * em;
    let r = Rect::from_min_size(pos2(rect.min.x, box_top), vec2(prep.width, prep.height));
    let painter = ui.painter().clone();
    painter.rect(
        r,
        CornerRadius::same(6),
        env.pal.quote_bg,
        Stroke::new(1.0, env.pal.border),
        StrokeKind::Middle,
    );
    draw_image_icon(&painter, pos2(r.min.x + 18.0, r.center().y), env);
    painter.galley(
        pos2(r.min.x + 58.0, r.center().y - prep.label.size().y * 0.5),
        prep.label.clone(),
        env.pal.muted,
    );
}

/// Чекбокс task list.
pub fn draw_checkbox(painter: &Painter, min: Pos2, checked: bool, env: &Env) {
    let size = 15.0;
    let rect = Rect::from_min_size(min, Vec2::splat(size));
    let radius = CornerRadius::same(3);
    if checked {
        painter.rect_filled(rect, radius, env.pal.link);
        let c = rect.center();
        let s = size * 0.28;
        painter.line_segment(
            [pos2(c.x - s, c.y + s * 0.2), pos2(c.x - s * 0.25, c.y + s * 0.8)],
            Stroke::new(2.0, Color32::WHITE),
        );
        painter.line_segment(
            [pos2(c.x - s * 0.25, c.y + s * 0.8), pos2(c.x + s, c.y - s * 0.6)],
            Stroke::new(2.0, Color32::WHITE),
        );
    } else {
        painter.rect(
            rect,
            radius,
            Color32::TRANSPARENT,
            Stroke::new(1.5, env.pal.muted),
            StrokeKind::Middle,
        );
    }
}

fn draw_image_icon(painter: &Painter, center: Pos2, env: &Env) {
    let r = 13.0;
    let rect = Rect::from_center_size(center, Vec2::splat(r * 2.0));
    painter.rect(
        rect,
        CornerRadius::same(4),
        Color32::TRANSPARENT,
        Stroke::new(1.5, env.pal.muted),
        StrokeKind::Middle,
    );
    painter.circle_filled(pos2(center.x - r * 0.4, center.y - r * 0.35), 2.2, env.pal.muted);
    painter.line_segment(
        [
            pos2(center.x - r * 0.7, center.y + r * 0.6),
            pos2(center.x - r * 0.1, center.y - r * 0.2),
        ],
        Stroke::new(1.5, env.pal.muted),
    );
    painter.line_segment(
        [
            pos2(center.x - r * 0.1, center.y - r * 0.2),
            pos2(center.x + r * 0.75, center.y + r * 0.6),
        ],
        Stroke::new(1.5, env.pal.muted),
    );
}
