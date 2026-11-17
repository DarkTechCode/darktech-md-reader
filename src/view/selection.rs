//! Собственное выделение текста документа вместо плагина-наблюдателя egui
//! (жivoй счётчик, стабильность «выделено с первого раза»). Позиции живут в
//! координатах (блок, под-область, символ) — не зависят от раскладки и
//! переживают виртуальный скролл: подсвечиваются видимые части, выделение
//! хранится в Tab. Под-область: 0 для обычного текста/кода, индекс ячейки
//! таблицы (строка × колонки + столбец) для таблиц.

use std::ops::Range;
use std::sync::Arc;
use std::time::Instant;

use eframe::egui::text::{CCursor, Galley};
use eframe::egui::{Pos2, Rect};

use crate::md::blocks::{BlockKind, Doc, Span};

use super::doc_view::DocEvent;

/// Статистика выделения для статус-бара: (символы, слова, строки).
pub type SelStats = (usize, usize, usize);

/// Позиция в тексте документа. Ord — в порядке документа.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct SelPos {
    pub block: usize,
    pub sub: usize,
    pub ch: usize,
}

/// Шаг выделения при драге: посимвольно / по словам (двойной клик) /
/// по под-областям (тройной — абзац, ячейка).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Granularity {
    Char,
    Word,
    Paragraph,
}

/// Интервал двойного/тройного клика, мс.
const DOUBLE_CLICK_MS: u128 = 500;
/// Смещение клика от предыдущего, до которого клики — «в том же месте», px.
const CLICK_SAME_PX: f32 = 6.0;
/// Пройдено больше — это драг, а не «чистый» клик по ссылке, px.
const CLICK_MOVE_PX: f32 = 5.0;
/// Выделение больше (в байтах исходников) — слова/строки не пересчитываем
/// на каждом кадре драга (только по отпусканию кнопки).
const LIVE_STATS_MAX_BYTES: usize = 4_000_000;

/// Видимая текстовая область одного кадра: галлей + где он лежит на экране.
/// Регистрируется при отрисовке (place_text), собирается в SelFrame.
#[derive(Clone)]
pub struct Region {
    pub block: usize,
    pub sub: usize,
    pub galley: Arc<Galley>,
    /// Экранный прямоугольник галлея.
    pub rect: Rect,
    /// Смещение текста галлея в полном тексте под-области (видимый срез кода).
    pub base_ch: usize,
    /// Полная длина текста под-области, символов.
    pub total_ch: usize,
}

impl Region {
    /// Начало координат галлея на экране: у halign=Center строки лежат в
    /// [-w/2, +w/2] вокруг нуля (Label egui красит такой галлей от center_top),
    /// поэтому rect хранит ВИЗУАЛЬНЫЕ границы, а не позицию (0,0) галлея.
    pub fn origin(&self) -> Pos2 {
        self.rect.min - self.galley.rect.min.to_vec2()
    }

    /// Позиция под курсором (курсор клампится к тексту галлея).
    pub fn hit(&self, pos: Pos2) -> SelPos {
        let local = self.galley.cursor_from_pos(pos - self.origin());
        SelPos {
            block: self.block,
            sub: self.sub,
            ch: self.base_ch + local.index.0,
        }
    }

    fn start(&self) -> SelPos {
        SelPos {
            block: self.block,
            sub: self.sub,
            ch: self.base_ch,
        }
    }

    fn end(&self) -> SelPos {
        SelPos {
            block: self.block,
            sub: self.sub,
            ch: self.base_ch + self.galley.end().index.0,
        }
    }

    /// Расширение позиции до гранулярности: слово (в пределах текста галлея)
    /// или вся под-область. Возвращает диапазон в координатах под-области.
    pub fn around(&self, ch: usize, g: Granularity) -> (SelPos, SelPos) {
        let s = self.start();
        match g {
            Granularity::Char => (s, s),
            Granularity::Paragraph => (
                SelPos { ch: 0, ..s },
                SelPos {
                    ch: self.total_ch,
                    ..s
                },
            ),
            Granularity::Word => {
                let local = ch.saturating_sub(self.base_ch);
                let r = word_around(self.galley.text(), local);
                let a = SelPos {
                    ch: self.base_ch + r.start,
                    ..s
                };
                let b = SelPos {
                    ch: self.base_ch + r.end,
                    ..s
                };
                (a, b)
            }
        }
    }
}

/// Границы слова вокруг позиции (как двойной клик в браузерах): прогоны
/// одного класса — пробелы / буквенно-цифровые (с «_») / прочие знаки.
fn word_around(text: &str, ch: usize) -> Range<usize> {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    if n == 0 {
        return 0..0;
    }
    let ch = ch.min(n);
    let cls = |i: usize| -> u8 {
        if chars[i].is_whitespace() {
            0
        } else if chars[i].is_alphanumeric() || chars[i] == '_' {
            1
        } else {
            2
        }
    };
    // Курсор в конце текста — класс последнего символа.
    let class = cls(ch.min(n - 1));
    let mut s = ch;
    let mut e = ch;
    while e < n && cls(e) == class {
        e += 1;
    }
    while s > 0 && cls(s - 1) == class {
        s -= 1;
    }
    s..e.max(s)
}

/// Состояние выделения одной вкладки.
pub struct SelectionState {
    /// Эффективные концы (после гранулярности); равны — выделения нет.
    anchor: SelPos,
    focus: SelPos,
    /// Сырая позиция нажатия.
    origin: SelPos,
    /// Слово/абзац, захваченный при двойном/тройном клике (якорь расширения).
    anchor_unit: Option<(SelPos, SelPos)>,
    dragging: bool,
    granularity: Granularity,
    press_pos: Option<Pos2>,
    press_moved: bool,
    /// Серия кликов: (время, место, номер 1/2/3).
    clicks: Option<(Instant, Pos2, u8)>,
    /// Кэш статистики (пересчитывается лениво, в doc_view).
    pub stats: Option<SelStats>,
    pub stats_dirty: bool,
}

impl Default for SelectionState {
    fn default() -> Self {
        Self {
            anchor: SelPos::default(),
            focus: SelPos::default(),
            origin: SelPos::default(),
            anchor_unit: None,
            dragging: false,
            granularity: Granularity::Char,
            press_pos: None,
            press_moved: false,
            clicks: None,
            stats: None,
            stats_dirty: false,
        }
    }
}

impl SelectionState {
    pub fn is_empty(&self) -> bool {
        self.anchor == self.focus
    }

    /// Концы выделения по порядку в документе.
    pub fn range(&self) -> Option<(SelPos, SelPos)> {
        if self.is_empty() {
            None
        } else if self.anchor <= self.focus {
            Some((self.anchor, self.focus))
        } else {
            Some((self.focus, self.anchor))
        }
    }

    /// Диапазон символов выделения в под-области (None — не пересекается).
    pub fn region_range(&self, block: usize, sub: usize) -> Option<Range<usize>> {
        let (s, e) = self.range()?;
        let key = (block, sub);
        if key < (s.block, s.sub) || key > (e.block, e.sub) {
            return None;
        }
        let a = if key == (s.block, s.sub) { s.ch } else { 0 };
        let b = if key == (e.block, e.sub) { e.ch } else { usize::MAX };
        (a < b).then(|| a..b)
    }

    pub fn granularity(&self) -> Granularity {
        self.granularity
    }

    pub fn is_dragging(&self) -> bool {
        self.dragging
    }

    /// Клик по ссылке сработает, только если между нажатием и отпусканием
    /// не было ни движения, ни двойного клика.
    pub fn link_click_ok(&self) -> bool {
        self.press_pos.is_some() && !self.press_moved && self.granularity == Granularity::Char
    }

    /// Нажатие в области документа (сырая позиция + её расширение до
    /// гранулярности, если под курсором был текст).
    fn press(&mut self, raw: SelPos, unit: Option<(SelPos, SelPos)>, count: u8, pos: Pos2) {
        self.clicks = Some((Instant::now(), pos, count));
        self.granularity = match count {
            2 => Granularity::Word,
            3 => Granularity::Paragraph,
            _ => Granularity::Char,
        };
        self.origin = raw;
        self.anchor_unit = unit;
        self.dragging = true;
        self.press_pos = Some(pos);
        self.press_moved = false;
        match unit {
            Some((a, b)) if self.granularity != Granularity::Char => {
                self.anchor = a;
                self.focus = b;
            }
            _ => {
                self.anchor = raw;
                self.focus = raw;
            }
        }
        self.stats_dirty = true;
    }

    /// Драг: фокус под курсором (вызывается из place_text, когда курсор над
    /// областью, и из end_frame для клампа к ближайшему тексту).
    pub fn drag_focus(&mut self, raw: SelPos, unit: (SelPos, SelPos)) {
        match self.granularity {
            Granularity::Char => {
                self.anchor = self.origin;
                self.focus = raw;
            }
            _ => {
                let Some((as_, ae)) = self.anchor_unit else {
                    self.anchor = self.origin;
                    self.focus = raw;
                    return;
                };
                self.anchor = as_.min(unit.0);
                self.focus = ae.max(unit.1);
            }
        }
        self.stats_dirty = true;
    }

    /// Снять выделение (Esc, клик по пустому месту, смена документа).
    pub fn clear(&mut self) {
        self.anchor = SelPos::default();
        self.focus = SelPos::default();
        self.origin = SelPos::default();
        self.anchor_unit = None;
        self.dragging = false;
        self.granularity = Granularity::Char;
        self.press_pos = None;
        self.press_moved = false;
        self.clicks = None;
        if self.stats.is_some() {
            self.stats = None;
        }
        self.stats_dirty = false;
    }

    /// Ctrl+A: весь документ, виртуально (конец — по последнему блоку).
    pub fn select_all(&mut self, doc: &Doc) {
        self.clear();
        if doc.blocks.is_empty() {
            return;
        }
        self.focus = end_pos(doc);
        self.stats_dirty = true;
    }

    /// Пересчёт статистики, если поменялась (гейт LIVE_STATS_MAX_BYTES на
    /// время драга — гигантские выделения считаются по отпусканию).
    pub fn recompute_stats_if_due(&mut self, doc: &Doc) {
        if !self.stats_dirty {
            return;
        }
        let Some((s, e)) = self.range() else {
            self.stats = None;
            self.stats_dirty = false;
            return;
        };
        if self.dragging && selection_bytes(doc, s, e) > LIVE_STATS_MAX_BYTES {
            return; // посчитаем по release (end_frame сбросит dragging)
        }
        self.stats_dirty = false;
        self.stats = Some(compute_stats(doc, s, e));
    }
}

/// Область под курсором; по горизонтали мимо текста — ближайшая в той же
/// строке-полосе (драг левее/правее колонки тянет её крайнюю область).
fn hit(regions: &[Region], pos: Pos2) -> Option<(usize, SelPos)> {
    for (i, r) in regions.iter().enumerate() {
        if r.rect.contains(pos) {
            return Some((i, r.hit(pos)));
        }
    }
    let mut best: Option<(usize, f32)> = None;
    for (i, r) in regions.iter().enumerate() {
        if pos.y >= r.rect.min.y && pos.y < r.rect.max.y {
            let d = if pos.x < r.rect.min.x {
                r.rect.min.x - pos.x
            } else {
                (pos.x - r.rect.max.x).max(0.0)
            };
            if best.is_none_or(|(_, bd)| d < bd) {
                best = Some((i, d));
            }
        }
    }
    best.map(|(i, _)| (i, regions[i].hit(pos)))
}

/// Ближайшая позиция к курсору вне всех областей (выше/ниже/между блоками).
fn clamp_pos(regions: &[Region], pos: Pos2) -> (usize, SelPos) {
    for (i, r) in regions.iter().enumerate() {
        if pos.y < r.rect.min.y {
            return (i, r.start());
        }
    }
    let last = regions.len() - 1;
    (last, regions[last].end())
}

/// Кадровый контекст выделения: строится в doc_view, проходит через все
/// place_text (регистрация областей, драг под курсором — в момент отрисовки,
/// чтобы подсветка не отставала). Попадания поиска — только видимых блоков.
pub struct SelFrame<'a> {
    pub sel: &'a mut SelectionState,
    pub regions: Vec<Region>,
    /// Позиция указателя этого кадра.
    pub pointer: Option<Pos2>,
    /// Курсор над ссылкой (ставит place_text) — глушит I-beam.
    pub link_hover: bool,
    /// Попадания поиска в видимых блоках (place_text красит как выделение).
    pub find_hits: &'a [super::find::Hit],
    /// Текущее попадание (яркая подсветка).
    pub find_current: Option<super::find::Hit>,
    /// Экранный прямоугольник, где указатель НЕ документ (попап поиска) —
    /// нажатия/драги под ним не трогают выделение документа.
    pub blocked: Option<Rect>,
}

/// Обработка мыши за кадр: вызывается из doc_view после отрисовки блоков
/// (все области уже зарегистрированы). Драг внутри области обновляет фокус
/// ещё при отрисовке (place_text) — подсветка не отстаёт от курсора.
pub fn end_frame(
    ui: &eframe::egui::Ui,
    doc: &Doc,
    regions: &[Region],
    area: Rect,
    sel: &mut SelectionState,
    link_hover: bool,
    events: &mut Vec<DocEvent>,
    blocked: Option<Rect>,
) {
    let pressed = ui.input(|i| i.pointer.primary_pressed());
    let down = ui.input(|i| i.pointer.primary_down());
    let released = ui.input(|i| i.pointer.primary_released());
    let secondary = ui.input(|i| i.pointer.secondary_clicked());
    let pos = ui.input(|i| i.pointer.interact_pos());
    let in_area = pos.is_some_and(|p| {
        area.contains(p) && !blocked.is_some_and(|b| b.contains(p))
    });

    // Нажатие вне документа (таб-бар, панели) — прерываем начатый драг,
    // чтобы отпускание над документом не открывало ссылку «из ниоткуда».
    if pressed && !in_area {
        sel.dragging = false;
        sel.press_pos = None;
    }

    if let Some(pos) = pos {
        // Правый клик — контекст-меню (выделение не трогаем, как в браузерах).
        if secondary && in_area {
            let link = link_at(doc, regions, pos);
            events.push(DocEvent::ContextMenu { pos, link });
        }

        if pressed && in_area {
            let count = match sel.clicks {
                Some((t, p, n))
                    if t.elapsed().as_millis() <= DOUBLE_CLICK_MS
                        && p.distance(pos) <= CLICK_SAME_PX
                        && n < 3 =>
                {
                    n + 1
                }
                _ => 1,
            };
            let gran = match count {
                2 => Granularity::Word,
                3 => Granularity::Paragraph,
                _ => Granularity::Char,
            };
            match hit(regions, pos) {
                Some((i, raw)) => {
                    let unit = regions[i].around(raw.ch, gran);
                    sel.press(raw, Some(unit), count, pos);
                }
                None => {
                    let raw = regions
                        .is_empty()
                        .then(SelPos::default)
                        .unwrap_or_else(|| clamp_pos(regions, pos).1);
                    sel.press(raw, None, count, pos);
                }
            }
        } else if sel.dragging && down {
            if sel
                .press_pos
                .is_some_and(|pp| pos.distance(pp) > CLICK_MOVE_PX)
            {
                sel.press_moved = true;
            }
            // Курсор вне текста — тянем до ближайшей видимой области.
            if hit(regions, pos).is_none() && !regions.is_empty() {
                let (i, raw) = clamp_pos(regions, pos);
                let unit = regions[i].around(raw.ch, sel.granularity);
                sel.drag_focus(raw, unit);
            }
        }
    }

    if released && sel.dragging {
        sel.dragging = false;
        sel.stats_dirty = true; // финальный пересчёт без гейта
    }

    // Изменение «в конце кадра» (двойной клик, отпускание) — статус-бар и
    // подсветка увидят его только в следующем кадре: попросим его.
    if sel.stats_dirty {
        ui.ctx().request_repaint();
    }

    // I-beam над текстом (над ссылками курсор ставит place_text).
    if in_area && !link_hover && pos.is_some_and(|p| hit(regions, p).is_some()) {
        ui.ctx().set_cursor_icon(eframe::egui::CursorIcon::Text);
    }
}

/// Ссылка под позицией (для «Копировать адрес» в контекст-меню).
fn link_at(doc: &Doc, regions: &[Region], pos: Pos2) -> Option<String> {
    let (i, _) = hit(regions, pos)?;
    let r = &regions[i];
    let spans = block_sub_spans(doc.blocks.get(r.block)?, r.sub)?;
    let source = doc.source.as_ref();
    let starts = crate::layout::measure::char_starts(spans, source);
    for (si, span) in spans.iter().enumerate() {
        let Some(url) = span.link.as_deref() else { continue };
        let cs = starts[si];
        let ce = cs + span.text.get(source).chars().count();
        let rect = r
            .galley
            .pos_from_cursor(CCursor::new(cs))
            .union(r.galley.pos_from_cursor(CCursor::new(ce)))
            .translate(r.origin().to_vec2());
        if rect.contains(pos) {
            return Some(url.to_string());
        }
    }
    None
}

/// Спаны под-области блока (текстовые блоки — sub 0, таблицы — ячейка).
fn block_sub_spans(block: &BlockKind, sub: usize) -> Option<&[Span]> {
    match block {
        BlockKind::Heading { spans, .. }
        | BlockKind::Paragraph { spans }
        | BlockKind::Quote { spans, .. }
        | BlockKind::ListItem { spans, .. } => Some(spans),
        BlockKind::Table { aligns, rows } => {
            let (_, ri, ci) = table_pos(aligns.len(), sub);
            rows.get(ri)?.cells.get(ci).map(|v| v.as_slice())
        }
        _ => None,
    }
}

/// (число колонок, строка, столбец) под-области таблицы.
fn table_pos(ncols: usize, sub: usize) -> (usize, usize, usize) {
    let ncols = ncols.max(1);
    (ncols, sub / ncols, sub % ncols)
}

/// Конец документа для Ctrl+A (последняя под-область последнего блока).
pub fn end_pos(doc: &Doc) -> SelPos {
    let Some(b) = doc.blocks.len().checked_sub(1) else {
        return SelPos::default();
    };
    let source = doc.source.as_ref();
    let spans_ch = |spans: &[Span]| spans.iter().map(|s| s.text.get(source).chars().count()).sum();
    match &doc.blocks[b] {
        BlockKind::Code { code, .. } => SelPos {
            block: b,
            ch: code.get(source).chars().count(),
            ..SelPos::default()
        },
        BlockKind::Table { aligns, rows } => {
            let (ncols, ri, ci) = match rows.last() {
                Some(row) if !row.cells.is_empty() => {
                    (aligns.len().max(1), rows.len() - 1, row.cells.len() - 1)
                }
                _ => (aligns.len().max(1), 0, 0),
            };
            SelPos {
                block: b,
                sub: ri * ncols + ci,
                ch: rows
                    .get(ri)
                    .and_then(|r| r.cells.get(ci).map(|c| spans_ch(c)))
                    .unwrap_or(0),
            }
        }
        BlockKind::Heading { spans, .. }
        | BlockKind::Paragraph { spans }
        | BlockKind::Quote { spans, .. }
        | BlockKind::ListItem { spans, .. } => SelPos {
            block: b,
            ch: spans_ch(spans),
            ..SelPos::default()
        },
        BlockKind::Rule | BlockKind::Image { .. } => SelPos {
            block: b,
            ..SelPos::default()
        },
    }
}

/// Обход выделенного текста кусками: срезы спанов/кода + разделители
/// (блоки «\n\n», строки таблиц «\n», ячейки «\t») — в порядке документа.
fn walk_selection(doc: &Doc, s: SelPos, e: SelPos, mut f: impl FnMut(&str)) {
    let source = doc.source.as_ref();
    let last_block = doc.blocks.len().saturating_sub(1);
    for bi in s.block..=e.block.min(last_block) {
        let first = bi == s.block;
        let last = bi == e.block;
        let a = |sub: usize| if first && sub == s.sub { s.ch } else { 0 };
        let b = |sub: usize| if last && sub == e.sub { e.ch } else { usize::MAX };
        match &doc.blocks[bi] {
            BlockKind::Heading { spans, .. }
            | BlockKind::Paragraph { spans }
            | BlockKind::Quote { spans, .. }
            | BlockKind::ListItem { spans, .. } => {
                if bi > s.block {
                    f("\n\n");
                }
                walk_spans(spans, a(0), b(0), source, &mut f);
            }
            BlockKind::Code { code, .. } => {
                if bi > s.block {
                    f("\n\n");
                }
                f(char_slice(code.get(source), a(0), b(0)));
            }
            BlockKind::Table { aligns, rows } => {
                if bi > s.block {
                    f("\n\n");
                }
                let ncols = aligns.len().max(1);
                let mut emitted_row = false;
                for (ri, row) in rows.iter().enumerate() {
                    let mut emitted_cell = false;
                    for (ci, cell) in row.cells.iter().enumerate() {
                        let sub = ri * ncols + ci;
                        if (first && sub < s.sub) || (last && sub > e.sub) {
                            continue;
                        }
                        if emitted_cell {
                            f("\t");
                        } else if emitted_row {
                            f("\n");
                        }
                        walk_spans(cell, a(sub), b(sub), source, &mut f);
                        emitted_cell = true;
                        emitted_row = true;
                    }
                }
            }
            BlockKind::Rule | BlockKind::Image { .. } => {}
        }
    }
}

/// Срезы спанов, пересекающих диапазон [a, b) символов конкатенации.
fn walk_spans(spans: &[Span], a: usize, b: usize, source: &str, f: &mut impl FnMut(&str)) {
    let mut acc = 0usize;
    for span in spans {
        let text = span.text.get(source);
        let len = text.chars().count();
        let (s, e) = (acc.max(a), (acc + len).min(b));
        if s < e {
            f(char_slice(text, s - acc, e - acc));
        }
        acc += len;
    }
}

/// Срез строки по символьным индексам (клампится к длине).
fn char_slice(s: &str, a: usize, b: usize) -> &str {
    let n = s.chars().count();
    let (a, b) = (a.min(n), b.min(n));
    if a >= b {
        return "";
    }
    let byte = |i: usize| s.char_indices().map(|(b, _)| b).nth(i).unwrap_or(s.len());
    &s[byte(a)..byte(b)]
}

/// Выделенный текст как plain text (для Ctrl+C и контекст-меню).
pub fn copy_text(doc: &Doc, sel: &SelectionState) -> Option<String> {
    let (s, e) = sel.range()?;
    let mut out = String::new();
    walk_selection(doc, s, e, |piece| out.push_str(piece));
    (!out.is_empty()).then_some(out)
}

/// Точная статистика по кусочкам выделения (разделители учитываются).
fn compute_stats(doc: &Doc, s: SelPos, e: SelPos) -> SelStats {
    let (mut chars, mut words, mut lines) = (0usize, 0usize, 0usize);
    walk_selection(doc, s, e, |p| {
        chars += p.chars().count();
        words += p.split_whitespace().count();
        lines += p.matches('\n').count();
    });
    // Между блоками «\n\n» уже посчитаны; строка есть и при одном переносе.
    (chars, words, lines + usize::from(chars > 0))
}

/// Грубая оценка размера выделения в байтах (гейт живого пересчёта):
/// без нарезки, только длины спанов/кода.
fn selection_bytes(doc: &Doc, s: SelPos, e: SelPos) -> usize {
    let mut total = 0usize;
    let source = doc.source.as_ref();
    let last_block = doc.blocks.len().saturating_sub(1);
    for bi in s.block..=e.block.min(last_block) {
        let mut add = |spans: &[Span]| {
            total += spans.iter().map(|x| x.text.get(source).len()).sum::<usize>()
        };
        match &doc.blocks[bi] {
            BlockKind::Heading { spans, .. }
            | BlockKind::Paragraph { spans }
            | BlockKind::Quote { spans, .. }
            | BlockKind::ListItem { spans, .. } => add(spans),
            BlockKind::Code { code, .. } => total += code.get(source).len(),
            BlockKind::Table { rows, .. } => {
                for row in rows {
                    for cell in &row.cells {
                        add(cell);
                    }
                }
            }
            BlockKind::Rule | BlockKind::Image { .. } => {}
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::md::parser;

    fn doc(md: &str) -> Doc {
        parser::parse(md.into())
    }

    fn sel_of(a: (usize, usize, usize), f: (usize, usize, usize)) -> SelectionState {
        let mut s = SelectionState::default();
        s.anchor = SelPos {
            block: a.0,
            sub: a.1,
            ch: a.2,
        };
        s.focus = SelPos {
            block: f.0,
            sub: f.1,
            ch: f.2,
        };
        s
    }

    #[test]
    fn word_around_latin_cyrillic_punct() {
        assert_eq!(word_around("привет мир", 0), 0..6);
        assert_eq!(word_around("привет мир", 7), 7..10);
        assert_eq!(word_around("foo, bar", 2), 0..3);
        // клик точно по знаку препинания / пробелу выделяет их прогон (браузеры)
        assert_eq!(word_around("foo, bar", 3), 3..4);
        assert_eq!(word_around("foo, bar", 4), 4..5);
        assert_eq!(word_around("foo, bar", 5), 5..8);
        assert_eq!(word_around("  x  ", 0), 0..2);
        assert_eq!(word_around("abc", 3), 0..3); // курсор в конце — всё слово
        assert_eq!(word_around("", 0), 0..0);
    }

    #[test]
    fn region_range_partial_and_full() {
        let sel = sel_of((0, 0, 3), (2, 0, 5));
        assert_eq!(sel.region_range(0, 0), Some(3..usize::MAX));
        assert_eq!(sel.region_range(1, 0), Some(0..usize::MAX));
        assert_eq!(sel.region_range(2, 0), Some(0..5));
        assert_eq!(sel.region_range(3, 0), None);
        let same = sel_of((1, 0, 2), (1, 0, 7));
        assert_eq!(same.region_range(1, 0), Some(2..7));
        assert_eq!(same.region_range(0, 0), None);
    }

    #[test]
    fn copy_text_across_blocks_and_table() {
        let d = doc("Первый абзац.\n\nВторой с **жирным**.\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\nХвост.");
        // Три абзаца-блока? Проверим структуру: 0=Paragraph,1=Paragraph,2=Table,3=Paragraph.
        let sel = sel_of((0, 0, 7), (2, 1, 1));
        let text = copy_text(&d, &sel).unwrap();
        // конец — первый символ второй ячейки шапки (sub=1): «a<TAB>b»
        assert_eq!(text, "абзац.\n\nВторой с жирным.\n\na\tb");
    }

    #[test]
    fn copy_whole_document_and_stats() {
        let md = "Раз два.\n\n```rust\nfn a() {}\n```\n\n| x | y |\n|---|---|\n| 1 | 2 |";
        let d = doc(md);
        let mut sel = SelectionState::default();
        sel.select_all(&d);
        let text = copy_text(&d, &sel).unwrap();
        // код у парсера с хвостовым «\n» + разделитель «\n\n» между блоками
        assert_eq!(text, "Раз два.\n\nfn a() {}\n\n\nx\ty\n1\t2");
        let (chars, words, lines) = compute_stats(&d, sel.range().unwrap().0, sel.range().unwrap().1);
        assert_eq!(chars, text.chars().count());
        assert_eq!(words, text.split_whitespace().count());
        assert_eq!(lines, text.matches('\n').count() + 1);
    }

    #[test]
    fn select_all_empty_doc() {
        let d = Doc::empty();
        let mut sel = SelectionState::default();
        sel.select_all(&d);
        assert!(sel.is_empty());
    }
}
