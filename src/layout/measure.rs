//! Раскладка блоков: оценка высот без шрифтов, измерение (galley) и
//! «подготовленные» данные отрисовки. Отрисовка не считает размеры —
//! только красит по готовым позициям, поэтому измерение и рендер не расходятся.

use std::sync::Arc;

use eframe::egui::{self, pos2, vec2, Color32, FontFamily, FontId, Rect, Ui, Vec2};
use eframe::egui::epaint::text::VariationCoords;
use eframe::egui::text::{
    ByteIndex, Galley, LayoutJob, LayoutSection, TextFormat, TextWrapping,
};

use crate::fonts;
use crate::i18n::t;
use crate::md::blocks::{Alert, BlockKind, ListMarker, Span};

use crate::view::themes::{heading_ratio, Palette, Typo};
use crate::view::Env;

/// Параметры LayoutJob для набора спанов.
#[derive(Clone)]
pub struct JobSpec {
    pub size: f32,
    pub family: FontFamily,
    pub color: Color32,
    pub bold: bool,
    pub wrap: f32,
    pub line_height: f32,
    pub letter_spacing: f32,
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
    pub fn job(&self, spans: &[Span], pal: &Palette, hover_link: Option<&str>) -> LayoutJob {
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
                fmt.strikethrough = egui::Stroke::new(1.0, fmt.color);
            }
            if let Some(url) = span.link.as_deref() {
                fmt.color = pal.link;
                if Some(url) == hover_link {
                    fmt.color = crate::view::themes::lighten(pal.link, 0.35);
                    fmt.underline = egui::Stroke::new(1.0, fmt.color);
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

/// Блок, готовый к отрисовке: все позиции — относительно левого верхнего
/// угла блока, высота включена (высоты блока и кэша не разойдутся).
pub enum Prepared {
    Text(TextPrep),
    Code(CodePrep),
    Table(TablePrep),
    Rule,
    Image(ImagePrep),
}

/// Текстовый блок (абзац/заголовок/цитата/пункт списка) + декорации.
pub struct TextPrep {
    pub galley: Arc<Galley>,
    pub spec: JobSpec,
    /// Смещения (в символах) начала каждого спана в тексте job (хит-тест ссылок).
    pub char_starts: Vec<usize>,
    pub text_pos: Vec2,
    pub height: f32,
    /// Подпись GFM-alert цитаты.
    pub alert_label: Option<(Arc<Galley>, Vec2, Color32)>,
    /// Полоса цитаты слева.
    pub quote_bar: Option<(Rect, Color32)>,
    /// Фон alert-цитаты.
    pub alert_bg: Option<Rect>,
    /// Маркер списка.
    pub marker: Option<Marker>,
    /// Подчёркивание заголовка h1/h2: (y, ширина).
    pub underline: Option<(f32, f32)>,
}

pub enum Marker {
    /// Текст маркера («•», «1.») в позиции.
    Text(Arc<Galley>, Vec2, Color32),
    /// Чекбокс task list в позиции левого верхнего угла.
    Checkbox(bool, Vec2),
}

pub struct CodePrep {
    /// Подпись шапки (язык, для mermaid — с пометкой).
    pub label: Option<String>,
    pub em: f32,
    pub pad: f32,
    pub header_h: f32,
    pub line_h: f32,
    /// Байтовые смещения начал строк кода (для нарезки видимого окна).
    pub line_starts: Vec<u32>,
    /// Полная ширина контента (самая длинная строка + паддинги).
    pub content_w: f32,
    /// Резерв под горизонтальный скроллбар (если строки шире колонки).
    pub bar_space: f32,
    pub height: f32,
}

pub struct TablePrep {
    pub col_w: Vec<f32>,
    pub total_w: f32,
    pub pad_h: f32,
    pub pad_v: f32,
    /// Внешние вертикальные отступы блока.
    pub gap_top: f32,
    pub gap_bottom: f32,
    /// Резерв под горизонтальный скроллбар (если таблица шире колонки).
    pub bar_space: f32,
    /// Спецификация ячеек (для сборки job'ов на отрисовке).
    pub cell: JobSpec,
    /// Измеренные высоты строк (0 — не измерена, берём оценку).
    pub row_h: Vec<f32>,
    pub row_est: f32,
    pub height: f32,
}

/// Высота полосы горизонтального скроллбара egui (сплошной стиль, 20px + поля).
pub const BAR_H: f32 = 22.0;

impl TablePrep {
    pub fn row_height(&self, i: usize) -> f32 {
        let h = self.row_h[i];
        if h > 0.0 {
            h
        } else {
            self.row_est
        }
    }

    /// Пересчёт высоты блока после измерения строк (неизмеренные — по оценке).
    pub fn recompute_height(&mut self) {
        self.height = self.gap_top
            + self.gap_bottom
            + self.bar_space
            + (0..self.row_h.len()).map(|i| self.row_height(i)).sum::<f32>();
    }
}

pub struct ImagePrep {
    pub label: Arc<Galley>,
    pub width: f32,
    pub height: f32,
}

pub fn plain_span(text: &str) -> Span {
    Span {
        text: text.into(),
        src: None,
        fmt: Default::default(),
        link: None,
    }
}

/// Смещения (в символах) начала каждого спана в тексте job.
pub fn char_starts(spans: &[Span]) -> Vec<usize> {
    let mut out = Vec::with_capacity(spans.len());
    let mut acc = 0;
    for span in spans {
        out.push(acc);
        acc += span.text.chars().count();
    }
    out
}

// ---------------------------------------------------------------------------
// Вертикальные внешние отступы блоков (кратные em) — единые для оценки
// и измерения, чтобы скроллбар не прыгал.
// ---------------------------------------------------------------------------

/// Внешний отступ блока кода сверху/снизу.
pub fn gap_code(em: f32) -> f32 {
    0.85 * em
}

fn heading_before(level: u8) -> f32 {
    match level {
        1 => 1.15,
        2 => 1.05,
        3 => 0.9,
        _ => 0.8,
    }
}

fn heading_after(level: u8) -> f32 {
    if level <= 2 {
        1.1
    } else {
        0.5
    }
}

// ---------------------------------------------------------------------------
// Оценки высот (без шрифтов) — для скроллбара и неизмеренных зон.
// ---------------------------------------------------------------------------

/// Оценка высоты блока: символы / символы_в_строке × высота строки + отступы.
pub fn estimate(block: &BlockKind, typo: &Typo, col_w: f32) -> f32 {
    let em = typo.size;
    let rh = typo.row_height();
    let chars_per_line = ((col_w - 16.0) / (em * 0.52)).max(8.0);
    let text_h = |spans: &[Span], size: f32| {
        let chars: usize = spans.iter().map(|s| s.text.chars().count()).sum();
        ((chars as f32 / chars_per_line).ceil().max(1.0) * size * typo.line_height).max(rh)
    };
    match block {
        BlockKind::Heading { level, spans, .. } => {
            let before = heading_before(*level) * em;
            let after = heading_after(*level) * em;
            before + text_h(spans, em * heading_ratio(*level)) + after
        }
        BlockKind::Paragraph { spans } => 0.6 * em + text_h(spans, em),
        BlockKind::Quote { alert, spans, .. } => {
            let gap = if alert.is_some() { 0.7 } else { 0.45 } * em;
            let label = if alert.is_some() {
                em * 0.82 * typo.line_height + 0.3 * em
            } else {
                0.0
            };
            gap + 0.9 * em + text_h(spans, em) + label
        }
        BlockKind::ListItem { spans, loose, .. } => {
            let gap = if *loose { 0.6 } else { 0.3 } * em;
            gap + text_h(spans, em)
        }
        BlockKind::Code { code, .. } => {
            let lines = code.bytes().filter(|b| *b == b'\n').count() + 1;
            gap_code(em) * 2.0 + 32.0 + 24.0 + lines as f32 * (em * 0.84 * 1.55)
        }
        BlockKind::Table { rows, .. } => 1.65 * em + rows.len() as f32 * (em * 1.4 + 6.0),
        BlockKind::Rule => 1.6 * em + 1.0,
        BlockKind::Image { .. } => 0.6 * em + 84.0,
    }
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

// ---------------------------------------------------------------------------
// Измерение блоков.
// ---------------------------------------------------------------------------

pub fn measure_block(ui: &mut Ui, env: &Env, col_w: f32, block: &BlockKind) -> (Prepared, f32) {
    match block {
        BlockKind::Heading { level, spans, .. } => {
            measure_heading(ui, env, col_w, *level, spans)
        }
        BlockKind::Paragraph { spans } => measure_paragraph(ui, env, col_w, spans),
        BlockKind::Quote {
            depth,
            alert,
            spans,
        } => measure_quote(ui, env, col_w, *depth, *alert, spans),
        BlockKind::ListItem {
            indent,
            marker,
            task,
            cont,
            loose,
            quote_depth,
            spans,
        } => measure_list_item(
            ui, env, col_w, *indent, marker, *task, *cont, *loose, *quote_depth, spans,
        ),
        BlockKind::Code { lang, code } => measure_code(ui, env, col_w, lang, code),
        BlockKind::Table { aligns, rows } => measure_table(ui, env, col_w, aligns, rows),
        BlockKind::Rule => {
            let h = 1.6 * env.typo.size + 1.0;
            (Prepared::Rule, h)
        }
        BlockKind::Image { url, alt } => measure_image(ui, env, col_w, url, alt),
    }
}

fn empty_text_prep(galley: Arc<Galley>, spec: JobSpec, spans: &[Span], text_pos: Vec2, height: f32) -> TextPrep {
    TextPrep {
        galley,
        spec,
        char_starts: char_starts(spans),
        text_pos,
        height,
        alert_label: None,
        quote_bar: None,
        alert_bg: None,
        marker: None,
        underline: None,
    }
}

fn measure_heading(
    ui: &mut Ui,
    env: &Env,
    col_w: f32,
    level: u8,
    spans: &[Span],
) -> (Prepared, f32) {
    let em = env.typo.size;
    let before = heading_before(level) * em;
    let after = heading_after(level) * em;
    let spec = JobSpec {
        size: em * heading_ratio(level),
        wrap: col_w,
        bold: true,
        ..JobSpec::base(env)
    };
    let galley = spec.galley(spans, env.pal, ui);
    let h = before + galley.size().y + after;
    let mut prep = empty_text_prep(galley, spec, spans, vec2(0.0, before), h);
    if level <= 2 {
        prep.underline = Some((before + prep.galley.size().y + 0.4 * em, prep.galley.size().x));
    }
    (Prepared::Text(prep), h)
}

fn measure_paragraph(
    ui: &mut Ui,
    env: &Env,
    col_w: f32,
    spans: &[Span],
) -> (Prepared, f32) {
    let em = env.typo.size;
    let spec = JobSpec {
        wrap: col_w,
        ..JobSpec::base(env)
    };
    let galley = spec.galley(spans, env.pal, ui);
    let h = 0.6 * em + galley.size().y;
    let prep = empty_text_prep(galley, spec, spans, vec2(0.0, 0.6 * em), h);
    (Prepared::Text(prep), h)
}

fn measure_quote(
    ui: &mut Ui,
    env: &Env,
    col_w: f32,
    depth: u8,
    alert: Option<Alert>,
    spans: &[Span],
) -> (Prepared, f32) {
    let em = env.typo.size;
    let (color, label_key) = alert_colors(alert, env.pal);
    let indent = 14.0 * depth as f32;
    let gap = if alert.is_some() { 0.7 } else { 0.45 } * em;
    let text_w = col_w - indent - 12.0;
    let spec = JobSpec {
        wrap: text_w,
        color: if alert.is_some() { env.pal.fg } else { env.pal.muted },
        ..JobSpec::base(env)
    };
    let galley = spec.galley(spans, env.pal, ui);
    let pad_v = 0.45 * em;

    let mut label_h = 0.0;
    let alert_label = alert.map(|_| {
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
        label_h = g.size().y + 0.3 * em;
        (g, vec2(indent + 12.0, gap + pad_v), color)
    });

    let h = gap + pad_v * 2.0 + galley.size().y + label_h;
    let mut prep = empty_text_prep(
        galley,
        spec,
        spans,
        vec2(indent + 12.0, gap + pad_v + label_h),
        h,
    );
    if alert.is_some() {
        prep.alert_bg = Some(Rect::from_min_size(pos2(0.0, gap), vec2(col_w, h - gap)));
    }
    prep.quote_bar = Some((
        Rect::from_min_size(pos2(indent, gap), vec2(3.0, h - gap)),
        color,
    ));
    prep.alert_label = alert_label;
    (Prepared::Text(prep), h)
}

#[allow(clippy::too_many_arguments)]
fn measure_list_item(
    ui: &mut Ui,
    env: &Env,
    col_w: f32,
    indent: u8,
    marker: &ListMarker,
    task: Option<bool>,
    cont: bool,
    loose: bool,
    quote_depth: u8,
    spans: &[Span],
) -> (Prepared, f32) {
    let em = env.typo.size;
    let row_h = env.typo.row_height();
    let indent_px = 24.0 * (indent as f32 - 1.0) + 14.0 * quote_depth as f32;
    let gap = if loose { 0.6 } else { 0.3 } * em;

    let marker_text = match (marker, task) {
        (_, Some(_)) => String::new(),
        (ListMarker::Bullet, _) => "•".into(),
        (ListMarker::Ordered(n), _) => format!("{n}."),
    };
    let marker_galley = if task.is_none() {
        let spec = JobSpec {
            size: em,
            color: env.pal.muted,
            ..JobSpec::base(env)
        };
        Some(spec.galley(&[plain_span(&marker_text)], env.pal, ui))
    } else {
        None
    };
    let marker_w = if task.is_some() {
        20.0
    } else {
        marker_galley
            .as_ref()
            .map(|g| g.size().x.max(20.0))
            .unwrap_or(20.0)
            + 6.0
    };

    let text_x = indent_px + marker_w;
    let spec = JobSpec {
        wrap: col_w - text_x,
        ..JobSpec::base(env)
    };
    let galley = spec.galley(spans, env.pal, ui);
    let h = gap + galley.size().y.max(row_h);

    let first_line_cy = gap + row_h * 0.5;
    let marker_prep = match (task, cont) {
        (Some(checked), _) => {
            Some(Marker::Checkbox(checked, vec2(indent_px, first_line_cy - 7.5)))
        }
        (None, false) => marker_galley.map(|g| {
            let pos = vec2(
                indent_px + marker_w - g.size().x - 6.0,
                first_line_cy - g.size().y * 0.5,
            );
            Marker::Text(g, pos, env.pal.muted)
        }),
        (None, true) => None,
    };

    let mut prep = empty_text_prep(galley, spec, spans, vec2(text_x, gap), h);
    prep.marker = marker_prep;
    (Prepared::Text(prep), h)
}

fn measure_code(
    ui: &mut Ui,
    env: &Env,
    col_w: f32,
    lang: &Option<String>,
    code: &str,
) -> (Prepared, f32) {
    let em = env.typo.size;
    let code_em = em * 0.84;
    let line_h = code_em * 1.55;

    let is_diagram = lang.as_deref() == Some("mermaid");
    let label = match lang {
        Some(l) if is_diagram => Some(format!("{l} · {}", t(env.lang, "diagram_label"))),
        Some(l) => Some(l.clone()),
        None => None,
    };

    // Строки и ширина: моно — ширина = число символов ×advance.
    let mut line_starts = Vec::new();
    let mut max_chars = 0usize;
    let mut pos = 0usize;
    for line in code.split('\n') {
        line_starts.push(pos as u32);
        pos += line.len() + 1;
        max_chars = max_chars.max(line.chars().count());
    }
    let char_w = mono_char_w(ui, code_em);
    let content_w = max_chars as f32 * char_w + 24.0;
    let lines = line_starts.len().max(1);
    // Широкие строки → горизонтальный скроллбар, резервируем под него место.
    let bar_space = if content_w > col_w { BAR_H } else { 0.0 };
    let h = gap_code(em) * 2.0 + 32.0 + 24.0 + lines as f32 * line_h + bar_space;

    let prep = CodePrep {
        label,
        em,
        pad: 12.0,
        header_h: 32.0,
        line_h,
        line_starts,
        content_w,
        bar_space,
        height: h,
    };
    (Prepared::Code(prep), h)
}

/// Ширина одного символа моноширинного шрифта при данном размере.
fn mono_char_w(ui: &Ui, size: f32) -> f32 {
    let sample = "0".repeat(64);
    let g = ui.painter().layout_job(plain_job_mono(&sample, size, Color32::WHITE));
    g.size().x / 64.0
}

pub fn plain_job_mono(text: &str, size: f32, color: Color32) -> LayoutJob {
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

/// Строк таблицы для оценки ширин колонок: шапка + первые строки.
const TABLE_SAMPLE_ROWS: usize = 64;
/// Максимум колонки от ширины контейнера: широкие колонки переносятся,
/// а не растягивают таблицу (как на GitHub/Discord).
const TABLE_COL_CAP_K: f32 = 0.66;

fn measure_table(
    ui: &mut Ui,
    env: &Env,
    col_w: f32,
    aligns: &[crate::md::blocks::Align],
    rows: &[crate::md::blocks::TableRow],
) -> (Prepared, f32) {
    let em = env.typo.size;
    let cell = JobSpec {
        size: em * 0.94,
        line_height: em * 1.4,
        wrap: f32::INFINITY,
        ..JobSpec::base(env)
    };

    // Естественные ширины колонок по шапке и образцу строк, с потолком:
    // очень широкие колонки переносятся по словам внутри ячеек.
    let cap = (col_w * TABLE_COL_CAP_K).max(160.0);
    let mut cols = vec![0.0f32; aligns.len()];
    for (ri, row) in rows.iter().enumerate().take(TABLE_SAMPLE_ROWS + 1) {
        let mut spec = cell.clone();
        spec.bold = ri == 0;
        for (ci, cell_spans) in row.cells.iter().enumerate() {
            let job = spec.job(cell_spans, env.pal, None);
            let w = ui.painter().layout_job(job).size().x;
            if ci < cols.len() {
                cols[ci] = cols[ci].max(w);
            }
        }
    }
    for w in &mut cols {
        *w = (w.min(cap) + 20.0).max(48.0);
    }
    // Таблица занимает всю ширину колонки (как на GitHub), если не шире её.
    let total = cols.iter().sum::<f32>();
    if total < col_w && !cols.is_empty() {
        let extra = (col_w - total) / cols.len() as f32;
        for w in &mut cols {
            *w += extra;
        }
    }
    let total_w = total.max(col_w);
    let bar_space = if total_w > col_w { BAR_H } else { 0.0 };

    let row_est = em * 1.4 + 6.0;
    let height = 0.95 * em + rows.len() as f32 * row_est + 0.7 * em + bar_space;
    let prep = TablePrep {
        col_w: cols,
        total_w,
        pad_h: 10.0,
        pad_v: 5.0,
        gap_top: 0.95 * em,
        gap_bottom: 0.7 * em,
        bar_space,
        cell,
        row_h: vec![0.0; rows.len()],
        row_est,
        height,
    };
    (Prepared::Table(prep), height)
}

fn measure_image(
    ui: &mut Ui,
    env: &Env,
    col_w: f32,
    url: &str,
    alt: &str,
) -> (Prepared, f32) {
    let em = env.typo.size;
    let w = col_w.min(560.0);
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
    let h = 0.6 * em + 84.0;
    let prep = ImagePrep {
        label: galley,
        width: w,
        height: 84.0,
    };
    (Prepared::Image(prep), h)
}
