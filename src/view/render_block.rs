//! Отрисовка одного блока документа. Текст — через selectable-Label
//! (выделение мышью и Ctrl+C), всё остальное — painter.

use std::sync::Arc;

use eframe::egui::{
    self as egui, pos2, vec2, Color32, CornerRadius, FontFamily, FontId, Rect,
    Sense, Stroke, StrokeKind, Ui, Vec2,
};
use eframe::egui::epaint::text::VariationCoords;
use eframe::egui::text::{
    ByteIndex, CCursor, Galley, LayoutJob, LayoutSection, TextFormat, TextWrapping,
};

use crate::fonts;
use crate::i18n::t;
use crate::md::blocks::{Alert, Align as MdAlign, BlockKind, ListMarker, Span};

use super::doc_view::DocEvent;
use super::themes::{lighten, Palette};
use super::Env;

/// Параметры LayoutJob для набора спанов.
struct JobSpec {
    size: f32,
    family: FontFamily,
    color: Color32,
    bold: bool,
    wrap: f32,
    line_height: f32,
    letter_spacing: f32,
}

impl JobSpec {
    fn base(env: &Env) -> Self {
        let typo = env.typo;
        Self {
            size: typo.size,
            family: typo.family(),
            color: env.pal.fg,
            bold: false,
            wrap: f32::INFINITY,
            line_height: typo.row_height(),
            letter_spacing: typo.letter_spacing,
        }
    }

    /// Собирает job из спанов (секции 1:1 со спанами, для хит-теста ссылок).
    /// `hover_link` — URL ссылки под курсором (подсветить её).
    fn job(&self, spans: &[Span], pal: &Palette, hover_link: Option<&str>) -> LayoutJob {
        let mut job = LayoutJob {
            wrap: TextWrapping {
                max_width: self.wrap,
                ..Default::default()
            },
            ..Default::default()
        };
        for span in spans {
            let mut fmt = TextFormat {
                font_id: FontId::new(self.size, self.family.clone()),
                color: self.color,
                line_height: Some(self.line_height),
                extra_letter_spacing: self.letter_spacing,
                ..Default::default()
            };
            if self.bold || span.fmt.bold {
                fmt.coords = VariationCoords::new([(b"wght", 700.0)]);
            }
            if span.fmt.italic {
                fmt.italics = true;
            }
            if span.fmt.strike {
                fmt.strikethrough = Stroke::new(1.0, fmt.color);
            }
            if let Some(url) = span.link.as_deref() {
                fmt.color = pal.link;
                if Some(url) == hover_link {
                    fmt.color = lighten(pal.link, 0.35);
                    fmt.underline = Stroke::new(1.0, fmt.color);
                }
            }
            if span.fmt.html {
                fmt.font_id.family = FontFamily::Name(fonts::MONO.into());
                fmt.font_id.size = self.size * 0.88;
                fmt.color = pal.muted;
            } else if span.fmt.code {
                fmt.font_id.family = FontFamily::Name(fonts::MONO.into());
                fmt.font_id.size = self.size * 0.88;
                fmt.color = pal.fg;
                fmt.background = pal.inline_code_bg;
            }
            let start = job.text.len();
            job.text.push_str(&span.text);
            job.sections.push(LayoutSection {
                leading_space: 0.0,
                byte_range: ByteIndex(start)..ByteIndex(job.text.len()),
                format: fmt,
            });
        }
        job
    }

    /// Раскладка job через galley-кэш egui (дёшево на повторных кадрах).
    fn galley(&self, spans: &[Span], pal: &Palette, ui: &mut Ui) -> Arc<Galley> {
        ui.painter().layout_job(self.job(spans, pal, None))
    }
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
        // Пересобираем job с подсветкой ссылки (расладка та же — ширины не меняются).
        ui.put(
            label_rect,
            egui::Label::new(Arc::new(spec.job(spans, pal, Some(url)))),
        );
        draw_url_tooltip(ui, url);
        // Курсор ставим ПОСЛЕ Label, иначе перебьётся курсором выделения текста.
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        if ui.input(|i| i.pointer.primary_clicked())
            && r.contains(ui.input(|i| i.pointer.hover_pos()).unwrap())
        {
            events.push(DocEvent::OpenLink(url.clone()));
        }
    } else {
        ui.put(
            label_rect,
            egui::Label::new(Arc::new(spec.job(spans, pal, None))),
        );
    }
    events
}

/// Компактная подсказка с URL у курсора.
fn draw_url_tooltip(ui: &Ui, url: &str) {
    let Some(p) = ui.input(|i| i.pointer.hover_pos()) else { return };
    let galley = ui.painter().layout_job(plain_job(url, 12.5, ui.style().visuals.strong_text_color()));
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
    painter.galley(rect.min + vec2(pad, pad), galley, ui.style().visuals.strong_text_color());
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

/// Смещения (в символах) начала каждого спана в тексте job.
fn char_starts(spans: &[Span]) -> Vec<usize> {
    let mut out = Vec::with_capacity(spans.len());
    let mut acc = 0;
    for span in spans {
        out.push(acc);
        acc += span.text.chars().count();
    }
    out
}

fn plain_span(text: &str) -> Span {
    Span {
        text: text.into(),
        src: None,
        fmt: Default::default(),
        link: None,
    }
}

/// Рисует блок; `idx` — индекс блока (для уникальных id вложенных скроллов).
pub fn show_block(ui: &mut Ui, idx: usize, block: &BlockKind, env: &Env) -> Vec<DocEvent> {
    match block {
        BlockKind::Heading {
            level, spans, ..
        } => show_heading(ui, *level, spans, env),
        BlockKind::Paragraph { spans } => show_paragraph(ui, spans, env),
        BlockKind::Quote {
            depth,
            alert,
            spans,
        } => show_quote(ui, *depth, *alert, spans, env),
        BlockKind::ListItem {
            indent,
            marker,
            task,
            cont,
            loose,
            quote_depth,
            spans,
        } => show_list_item(
            ui, *indent, marker, *task, *cont, *loose, *quote_depth, spans, env,
        ),
        BlockKind::Code { lang, code } => show_code(ui, idx, lang, code, env),
        BlockKind::Table { aligns, rows } => show_table(ui, idx, aligns, rows, env),
        BlockKind::Rule => {
            show_rule(ui, env);
            Vec::new()
        }
        BlockKind::Image { url, alt } => {
            show_image(ui, url, alt, env);
            Vec::new()
        }
    }
}

fn show_heading(ui: &mut Ui, level: u8, spans: &[Span], env: &Env) -> Vec<DocEvent> {
    let em = env.typo.size;
    let before = match level {
        1 => 1.0,
        2 => 0.9,
        3 => 0.75,
        _ => 0.65,
    };
    ui.add_space(before * em);
    let spec = JobSpec {
        size: em * super::themes::heading_ratio(level),
        wrap: ui.available_width(),
        bold: true,
        ..JobSpec::base(env)
    };
    let galley = spec.galley(spans, env.pal, ui);
    let (rect, _) = ui.allocate_exact_size(galley.size(), Sense::hover());
    let events = put_text(
        ui,
        rect,
        &galley,
        spans,
        &spec,
        env.pal,
        &char_starts(spans),
    );
    if level <= 2 {
        let y = rect.max.y + 0.4 * em;
        ui.painter().line_segment(
            [pos2(rect.min.x, y), pos2(rect.max.x, y)],
            Stroke::new(1.0, env.pal.border),
        );
        ui.add_space(0.6 * em);
    } else {
        ui.add_space(0.35 * em);
    }
    events
}

fn show_paragraph(ui: &mut Ui, spans: &[Span], env: &Env) -> Vec<DocEvent> {
    ui.add_space(0.45 * env.typo.size);
    let spec = JobSpec {
        wrap: ui.available_width(),
        ..JobSpec::base(env)
    };
    let galley = spec.galley(spans, env.pal, ui);
    let (rect, _) = ui.allocate_exact_size(galley.size(), Sense::hover());
    put_text(ui, rect, &galley, spans, &spec, env.pal, &char_starts(spans))
}

fn alert_colors(alert: Option<Alert>, pal: &Palette) -> (Color32, &'static str) {
    match alert {
        Some(Alert::Note) => (pal.alert_note, "alert_note"),
        Some(Alert::Tip) => (pal.alert_tip, "alert_tip"),
        Some(Alert::Important) => (pal.alert_important, "alert_important"),
        Some(Alert::Warning) => (pal.alert_warning, "alert_warning"),
        Some(Alert::Caution) => (pal.alert_caution, "alert_caution"),
        None => (pal.quote_bar, ""),
    }
}

fn show_quote(ui: &mut Ui, depth: u8, alert: Option<Alert>, spans: &[Span], env: &Env) -> Vec<DocEvent> {
    let em = env.typo.size;
    let (color, label_key) = alert_colors(alert, env.pal);
    let indent = 14.0 * depth as f32;
    ui.add_space(if alert.is_some() { 0.55 } else { 0.3 } * em);

    let text_w = ui.available_width() - indent - 12.0;
    let spec = JobSpec {
        wrap: text_w,
        color: if alert.is_some() { env.pal.fg } else { env.pal.muted },
        ..JobSpec::base(env)
    };
    let galley = spec.galley(spans, env.pal, ui);
    let pad_v = 0.35 * em;
    let mut total_h = galley.size().y + pad_v * 2.0;

    let label_galley = alert.map(|_| {
        let label_spec = JobSpec {
            size: em * 0.82,
            wrap: text_w,
            color,
            bold: true,
            family: FontFamily::Proportional,
            ..JobSpec::base(env)
        };
        let g = label_spec
            .galley(&[plain_span(t(env.lang, label_key))], env.pal, ui);
        total_h += g.size().y + 0.25 * em;
        g
    });

    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), total_h), Sense::hover());
    let painter = ui.painter().clone();
    if alert.is_some() {
        painter.rect(
            rect,
            CornerRadius::same(4),
            env.pal.quote_bg,
            Stroke::NONE,
            StrokeKind::Middle,
        );
    }
    painter.rect_filled(
        Rect::from_min_max(
            pos2(rect.min.x + indent, rect.min.y),
            pos2(rect.min.x + indent + 3.0, rect.max.y),
        ),
        CornerRadius::same(2),
        color,
    );
    let mut y = rect.min.y + pad_v;
    let x = rect.min.x + indent + 12.0;
    if let Some(label) = label_galley {
        let h = label.size().y;
        painter.galley(pos2(x, y), label, color);
        y += h + 0.25 * em;
    }
    let label_rect = Rect::from_min_size(pos2(x, y), galley.size());
    put_text(
        ui,
        label_rect,
        &galley,
        spans,
        &spec,
        env.pal,
        &char_starts(spans),
    )
}

#[allow(clippy::too_many_arguments)]
fn show_list_item(
    ui: &mut Ui,
    indent: u8,
    marker: &ListMarker,
    task: Option<bool>,
    cont: bool,
    loose: bool,
    quote_depth: u8,
    spans: &[Span],
    env: &Env,
) -> Vec<DocEvent> {
    let em = env.typo.size;
    let row_h = env.typo.row_height();
    let indent_px = 24.0 * (indent as f32 - 1.0) + 14.0 * quote_depth as f32;
    let gap = if loose { 0.5 } else { 0.22 } * em;
    ui.add_space(gap);

    // Ширина зоны маркера — единая, чтобы текст пунктов выравнивался.
    let marker_text = match (marker, task) {
        (_, Some(_)) => String::new(),
        (ListMarker::Bullet, _) => "•".into(),
        (ListMarker::Ordered(n), _) => format!("{n}."),
    };
    let marker_w = if task.is_some() {
        20.0
    } else {
        let spec = JobSpec {
            size: em,
            wrap: f32::INFINITY,
            color: env.pal.muted,
            ..JobSpec::base(env)
        };
        spec.galley(&[plain_span(&marker_text)], env.pal, ui)
            .size()
            .x
            .max(20.0)
            + 6.0
    };

    let text_x = indent_px + marker_w;
    let spec = JobSpec {
        wrap: ui.available_width() - text_x,
        ..JobSpec::base(env)
    };
    let galley = spec.galley(spans, env.pal, ui);
    let h = galley.size().y.max(row_h);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());

    let painter = ui.painter().clone();
    let first_line_cy = rect.min.y + row_h * 0.5;
    if let Some(checked) = task {
        draw_checkbox(
            &painter,
            pos2(rect.min.x + indent_px, first_line_cy - 7.5),
            checked,
            env,
        );
    } else if !cont {
        let spec = JobSpec {
            size: em,
            wrap: f32::INFINITY,
            color: env.pal.muted,
            ..JobSpec::base(env)
        };
        let m = spec.galley(&[plain_span(&marker_text)], env.pal, ui);
        painter.galley(
            pos2(
                rect.min.x + indent_px + marker_w - m.size().x - 6.0,
                first_line_cy - m.size().y * 0.5,
            ),
            m,
            env.pal.muted,
        );
    }
    let label_rect = Rect::from_min_size(pos2(rect.min.x + text_x, rect.min.y), galley.size());
    put_text(
        ui,
        label_rect,
        &galley,
        spans,
        &spec,
        env.pal,
        &char_starts(spans),
    )
}

fn draw_checkbox(
    painter: &eframe::egui::Painter,
    min: eframe::egui::Pos2,
    checked: bool,
    env: &Env,
) {
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

fn show_code(ui: &mut Ui, idx: usize, lang: &Option<String>, code: &str, env: &Env) -> Vec<DocEvent> {
    let em = env.typo.size;
    ui.add_space(0.6 * em);

    let is_diagram = lang.as_deref() == Some("mermaid");
    let label = match lang {
        Some(l) if is_diagram => format!("{l} · {}", t(env.lang, "diagram_label")),
        Some(l) => l.clone(),
        None => String::new(),
    };

    let spec = JobSpec {
        size: em * 0.84,
        family: FontFamily::Name(fonts::MONO.into()),
        wrap: f32::INFINITY,
        line_height: em * 0.84 * 1.55,
        ..JobSpec::base(env)
    };
    let galley = spec.galley(&[plain_span(code)], env.pal, ui);

    let pad = 12.0;
    let full_w = ui.available_width();
    let header_h = 32.0;
    let code_h = galley.size().y + pad * 2.0;

    // Шапка блока: подпись языка слева, кнопка «копировать» справа — внутри блока.
    let (hrect, _) = ui.allocate_exact_size(vec2(full_w, header_h), Sense::hover());
    let painter = ui.painter().clone();
    painter.rect(
        hrect,
        egui::CornerRadius::same(0),
        env.pal.code_bg,
        Stroke::NONE,
        StrokeKind::Middle,
    );
    if !label.is_empty() {
        let g = painter.layout_job(plain_job_mono(&label, em * 0.78, env.pal.muted));
        painter.galley(
            pos2(hrect.min.x + pad, hrect.center().y - g.size().y * 0.5),
            g,
            env.pal.muted,
        );
    }
    {
        // Кнопка в правом верхнем углу блока — абсолютное позиционирование.
        let btn_g = painter.layout_job(plain_job_mono(t(env.lang, "copy"), em * 0.72, env.pal.fg));
        let btn_size = btn_g.size() + vec2(16.0, 6.0);
        let btn_rect = Rect::from_min_size(
            pos2(hrect.max.x - pad - btn_size.x, hrect.center().y - btn_size.y * 0.5),
            btn_size,
        );
        let btn_resp = ui.interact(
            btn_rect,
            ui.scope_id().with(("code_copy", idx)),
            Sense::click(),
        );
        let hovered = btn_resp.hovered();
        let painter2 = ui.painter().clone();
        painter2.rect(
            btn_rect,
            egui::CornerRadius::same(0),
            if hovered {
                lighten(env.pal.code_bg, 0.5)
            } else {
                Color32::TRANSPARENT
            },
            Stroke::new(1.0, env.pal.border),
            StrokeKind::Middle,
        );
        painter2.galley(
            btn_rect.center() - btn_g.size() * 0.5,
            btn_g,
            if hovered { env.pal.fg } else { env.pal.muted },
        );
        if hovered {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if btn_resp.clicked() {
            ui.ctx().copy_text(code.to_string());
        }
    }

    // Код: фон тянется шире контента, чтобы не «уезжал» при горизонтальном скролле.
    scroll_h(ui, ("code", idx), full_w, |ui| {
        let content_w = galley.size().x + pad * 2.0;
        let (rect, _) = ui.allocate_exact_size(vec2(content_w, code_h), Sense::hover());
        let painter = ui.painter();
        painter.rect(
            Rect::from_min_max(
                pos2(rect.min.x - 100_000.0, rect.min.y),
                pos2(rect.max.x + 100_000.0, rect.max.y),
            ),
            egui::CornerRadius::same(0),
            env.pal.code_bg,
            Stroke::NONE,
            StrokeKind::Middle,
        );
        let label_rect = Rect::from_min_size(pos2(rect.min.x + pad, rect.min.y + pad), galley.size());
        ui.put(
            label_rect,
            egui::Label::new(Arc::new(spec.job(&[plain_span(code)], env.pal, None))),
        );
    });
    ui.add_space(0.6 * em);
    Vec::new()
}

fn plain_job_mono(text: &str, size: f32, color: Color32) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.sections.push(LayoutSection {
        leading_space: 0.0,
        byte_range: ByteIndex(0)..ByteIndex(text.len()),
        format: TextFormat {
            font_id: FontId::new(size, FontFamily::Name(fonts::MONO.into())),
            color,
            ..Default::default()
        },
    });
    job.text.push_str(text);
    job
}

fn scroll_h(ui: &mut Ui, id: impl std::hash::Hash + std::fmt::Debug, max_w: f32, content: impl FnOnce(&mut Ui)) {
    eframe::egui::ScrollArea::horizontal()
        .id_salt(id)
        .max_width(max_w)
        .auto_shrink([false, false])
        .show(ui, |ui| content(ui));
}

fn show_table(
    ui: &mut Ui,
    idx: usize,
    aligns: &[MdAlign],
    rows: &[crate::md::blocks::TableRow],
    env: &Env,
) -> Vec<DocEvent> {
    let em = env.typo.size;
    ui.add_space(0.5 * em);
    let pad_h = 10.0;
    let pad_v = 3.0;
    let avail = ui.available_width();

    // Естественные ширины колонок.
    let mut col_w = vec![0.0f32; aligns.len()];
    let mut jobs: Vec<Vec<LayoutJob>> = Vec::with_capacity(rows.len());
    for (ri, row) in rows.iter().enumerate() {
        let mut row_jobs = Vec::with_capacity(row.cells.len());
        for (ci, cell) in row.cells.iter().enumerate() {
            let spec = JobSpec {
                wrap: f32::INFINITY,
                bold: ri == 0,
                size: em * 0.94,
                line_height: em * 1.4,
                ..JobSpec::base(env)
            };
            let job = spec.job(cell, env.pal, None);
            let w = ui.painter().layout_job(job.clone()).size().x;
            if ci < col_w.len() {
                col_w[ci] = col_w[ci].max(w);
            }
            row_jobs.push(job);
        }
        jobs.push(row_jobs);
    }
    for w in &mut col_w {
        *w = (*w + pad_h * 2.0).max(48.0);
    }
    // Таблица занимает всю ширину колонки (как на GitHub), если не шире её.
    let total = col_w.iter().sum::<f32>();
    if total < avail && !col_w.is_empty() {
        let extra = (avail - total) / col_w.len() as f32;
        for w in &mut col_w {
            *w += extra;
        }
    }
    let total = col_w.iter().sum::<f32>().max(avail);

    // Раскладка ячеек с учётом переноса.
    let galleys: Vec<Vec<(Arc<Galley>, f32)>> = jobs
        .iter()
        .map(|row| {
            row.iter()
                .enumerate()
                .map(|(ci, job)| {
                    let mut job = job.clone();
                    job.wrap.max_width = col_w.get(ci).copied().unwrap_or(300.0) - pad_h * 2.0;
                    let g = ui.painter().layout_job(job);
                    let h = g.size().y + pad_v * 2.0;
                    (g, h)
                })
                .collect()
        })
        .collect();
    let total_h: f32 = galleys
        .iter()
        .map(|r| r.iter().map(|(_, h)| *h).sum::<f32>())
        .sum();

    scroll_h(ui, ("table", idx), avail, |ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(total, total_h), Sense::hover());
        let painter = ui.painter().clone();
        let mut y = rect.min.y;
        let x0 = rect.min.x;
        for (ri, row) in galleys.iter().enumerate() {
            let row_h: f32 = row.iter().map(|(_, h)| *h).sum();
            if ri == 0 {
                painter.rect_filled(
                    Rect::from_min_max(pos2(x0, y), pos2(x0 + total, y + row_h)),
                    CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 },
                    env.pal.table_head_bg,
                );
            }
            let mut x = x0;
            for (ci, (galley, cell_h)) in row.iter().enumerate() {
                let cw = col_w.get(ci).copied().unwrap_or(300.0);
                let align = aligns.get(ci).copied().unwrap_or(MdAlign::None);
                let tx = match align {
                    MdAlign::Right => x + cw - pad_h - galley.size().x,
                    MdAlign::Center => x + (cw - galley.size().x) / 2.0,
                    _ => x + pad_h,
                };
                let ty = y + (cell_h - galley.size().y) / 2.0;
                let cell_rect = Rect::from_min_size(pos2(tx, ty), galley.size());
                ui.put(cell_rect, eframe::egui::Label::new(galley.clone()));
                x += cw;
            }
            painter.line_segment(
                [pos2(x0, y + row_h), pos2(x0 + total, y + row_h)],
                Stroke::new(1.0, env.pal.border),
            );
            y += row_h;
        }
    });
    Vec::new()
}

fn show_rule(ui: &mut Ui, env: &Env) {
    let em = env.typo.size;
    ui.add_space(0.6 * em);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    let mid = rect.center();
    ui.painter().line_segment(
        [pos2(rect.min.x, mid.y), pos2(rect.max.x, mid.y)],
        Stroke::new(1.0, env.pal.border),
    );
    ui.add_space(0.6 * em);
}

fn show_image(ui: &mut Ui, url: &str, alt: &str, env: &Env) {
    let em = env.typo.size;
    ui.add_space(0.4 * em);
    let w = ui.available_width().min(560.0);
    let h = 84.0;
    let (rect, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    let painter = ui.painter().clone();
    painter.rect(
        rect,
        CornerRadius::same(6),
        env.pal.quote_bg,
        Stroke::new(1.0, env.pal.border),
        StrokeKind::Middle,
    );
    draw_image_icon(&painter, pos2(rect.min.x + 18.0, rect.center().y), env);

    let name = std::path::Path::new(url)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| url.to_string());
    let label = if alt.is_empty() { name } else { alt.to_string() };
    let spec = JobSpec {
        size: em * 0.85,
        color: env.pal.muted,
        wrap: w - 100.0,
        ..JobSpec::base(env)
    };
    let galley = spec.galley(&[plain_span(&label)], env.pal, ui);
    painter.galley(
        pos2(rect.min.x + 58.0, rect.center().y - galley.size().y * 0.5),
        galley,
        env.pal.muted,
    );
}

fn draw_image_icon(painter: &eframe::egui::Painter, center: eframe::egui::Pos2, env: &Env) {
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
