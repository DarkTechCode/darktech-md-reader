//! Текстовые блоки: декорации из раскладки + текст с собственным выделением
//! (подсветка красится в галлей за глифами) и ссылками (hover/клик/подсказка).

use std::sync::Arc;

use eframe::egui::{
    self as egui, pos2, vec2, Color32, CornerRadius, FontFamily, FontId, Rect, Stroke, StrokeKind,
    Ui,
};
use eframe::egui::text::{ByteIndex, CCursor, CCursorRange, Galley, LayoutJob, LayoutSection, TextFormat};

use crate::layout::measure::{char_starts, JobSpec, Marker, TextPrep};
use crate::md::blocks::Span;

use super::misc::draw_checkbox;
use super::super::doc_view::DocEvent;
use super::super::find::Hit;
use super::super::selection::{Region, SelFrame};
use super::super::themes::Palette;
use super::super::Env;

/// Параметры place_text: что кладём и где, с координатами выделения.
pub struct Place<'a> {
    /// Готовый галлей (без подсветки ссылки — при hover переберётся по spec).
    pub galley: &'a Arc<Galley>,
    /// Экранный прямоугольник галлея.
    pub rect: Rect,
    /// Спаны текста (ссылки, выделение слов); пусто для кода.
    pub spans: &'a [Span],
    /// Спецификация job (пересборка с подсветкой ссылки).
    pub spec: &'a JobSpec,
    /// Исходник документа (тексты спанов — диапазоны в нём).
    pub source: &'a str,
    /// Координаты выделения: блок / под-область / база видимого среза.
    pub block: usize,
    pub sub: usize,
    pub base_ch: usize,
    /// Полная длина текста под-области, символов.
    pub total_ch: usize,
}

/// Рисует подготовленный текстовый блок: фон/полосу цитаты, маркер, текст.
pub fn render_text(
    ui: &mut Ui,
    rect: Rect,
    block: usize,
    spans: &[Span],
    prep: &TextPrep,
    env: &Env,
    frame: &mut SelFrame,
    source: &str,
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
        // От начала ТЕКСТА заголовка (при центрировании он смещён).
        let x0 = o.x + prep.text_pos.x;
        painter.line_segment(
            [pos2(x0, o.y + y), pos2(x0 + w, o.y + y)],
            Stroke::new(1.0, env.pal.border),
        );
    }

    let label_rect = Rect::from_min_size(o + prep.text_pos, prep.galley.size());
    place_text(
        ui,
        env.pal,
        Place {
            galley: &prep.galley,
            rect: label_rect,
            spans,
            spec: &prep.spec,
            source,
            block,
            sub: 0,
            base_ch: 0,
            total_ch: prep.total_ch,
        },
        frame,
    )
}

/// Кладёт текст: регистрирует область выделения, красит подсветку прямо в
/// галлей (за глифами), обрабатывает ссылки (hover, «чистый» клик) — общий
/// путь для абзацев/заголовков/цитат/списков, ячеек таблиц и кода.
pub fn place_text(ui: &mut Ui, pal: &Palette, p: Place<'_>, f: &mut SelFrame) -> Vec<DocEvent> {
    // Область видимого текста: нужен трекингу выделения (драг под курсором —
    // сразу, чтобы подсветка не отставала на кадр).
    let region = Region {
        block: p.block,
        sub: p.sub,
        galley: p.galley.clone(),
        rect: p.rect,
        base_ch: p.base_ch,
        total_ch: p.total_ch,
    };
    f.regions.push(region.clone());
    if f.sel.is_dragging()
        && let Some(pos) = f.pointer
        && p.rect.contains(pos)
    {
        let raw = region.hit(pos);
        let unit = region.around(raw.ch, f.sel.granularity());
        f.sel.drag_focus(raw, unit);
    }

    // Начало координат галлея на экране (rect — визуальные границы): у
    // halign=Center строки лежат в [-w/2, +w/2] вокруг нуля, красим от
    // центра строк, как Label egui (center_top).
    let origin = p.rect.min - p.galley.rect.min.to_vec2();

    // Ссылка под курсором: прямоугольник каждого ссылочного спана —
    // объединение курсоров начала/конца.
    let mut hover: Option<(String, Rect)> = None;
    if p.spans.iter().any(|s| s.link.is_some())
        && let Some(pos) = f.pointer
    {
        let starts = char_starts(p.spans, p.source);
        for (i, span) in p.spans.iter().enumerate() {
            let Some(url) = span.link.as_deref() else { continue };
            let cs = starts[i];
            let ce = cs + span.text.get(p.source).chars().count();
            let r = p
                .galley
                .pos_from_cursor(CCursor::new(cs))
                .union(p.galley.pos_from_cursor(CCursor::new(ce)))
                .translate(origin.to_vec2());
            if r.contains(pos) {
                hover = Some((url.to_string(), r));
                break;
            }
        }
    }

    // Галлей кадра: с подсветкой ссылки под курсором (та же раскладка) —
    // и с подсветкой выделения, вклеенной за глифами.
    let mut galley = match &hover {
        Some((url, _)) => {
            ui.painter()
                .layout_job(p.spec.job(p.spans, pal, Some(url), p.source))
        }
        None => p.galley.clone(),
    };
    if !f.find_hits.is_empty() {
        paint_find_hits(
            ui,
            &mut galley,
            p.block,
            p.sub,
            p.base_ch,
            f.find_hits,
            f.find_current,
            pal,
        );
    }
    if let Some(range) = f.sel.region_range(p.block, p.sub) {
        let a = galley.clamp_cursor(&CCursor::new(range.start.saturating_sub(p.base_ch)));
        let b = galley.clamp_cursor(&CCursor::new(range.end.saturating_sub(p.base_ch)));
        if a.index < b.index {
            egui::text_selection::visuals::paint_text_selection(
                &mut galley,
                ui.visuals(),
                &CCursorRange::two(a, b),
                None,
            );
        }
    }
    let painter = ui.painter().clone();
    painter.galley(origin, galley, pal.fg);

    let mut events = Vec::new();
    if let Some((url, r)) = &hover {
        f.link_hover = true;
        draw_url_tooltip(ui, url);
        // Курсор — после отрисовки текста, иначе перебьётся выделением.
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        // Ссылка открывается отпусканием без движения (драг выделяет текст).
        let released = ui.input(|i| i.pointer.primary_released());
        if released && f.pointer.is_some_and(|pos| r.contains(pos)) && f.sel.link_click_ok() {
            events.push(DocEvent::OpenLink(url.clone()));
        }
    }
    events
}

/// Подсветка попаданий поиска в галлей под-области: та же механика, что у
/// выделения (paint_text_selection красит строки за глифами и перекрашивает
/// текст), но с цветами поиска; текущее попадание — ярче. Список уже отфильтрован
/// по видимым блокам; попадания отсортированы — находим свою под-область.
fn paint_find_hits(
    ui: &Ui,
    galley: &mut Arc<Galley>,
    block: usize,
    sub: usize,
    base_ch: usize,
    hits: &[Hit],
    current: Option<Hit>,
    pal: &Palette,
) {
    let mut vis_plain = ui.visuals().clone();
    vis_plain.selection.bg_fill = pal.find_hit;
    vis_plain.selection.stroke.color = pal.find_text;
    let mut vis_cur = ui.visuals().clone();
    vis_cur.selection.bg_fill = pal.find_cur;
    vis_cur.selection.stroke.color = pal.find_text;

    let mut i = hits.partition_point(|h| (h.block, h.sub) < (block, sub));
    while i < hits.len() && (hits[i].block, hits[i].sub) == (block, sub) {
        let h = hits[i];
        let a = galley.clamp_cursor(&CCursor::new(h.start.saturating_sub(base_ch)));
        let b = galley.clamp_cursor(&CCursor::new(h.end.saturating_sub(base_ch)));
        if a.index < b.index {
            let vis = if Some(h) == current {
                &vis_cur
            } else {
                &vis_plain
            };
            egui::text_selection::visuals::paint_text_selection(
                galley,
                vis,
                &CCursorRange::two(a, b),
                None,
            );
        }
        i += 1;
    }
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
