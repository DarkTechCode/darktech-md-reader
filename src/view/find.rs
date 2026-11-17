//! Поиск по документу (Ctrl+F): инкрементальный, фоновой нитью по тексту
//! блоков — тому, что видно на экране (спаны абзацев/заголовков/цитат/списков,
//! код, ячейки таблиц; синтаксис Markdown и сущности не ищутся). Попадания —
//! в координатах (блок, под-область, символы): place_text подсвечивает их
//! как выделение, навигация скроллит к блоку. Регистронезависимо (fold 1:1,
//! латиница/кириллица), memmem — SIMD-поиск подстроки.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};

use eframe::egui::Pos2;
use memchr::memmem;

use crate::layout::{measure, LayoutState};
use crate::md::blocks::{BlockKind, Doc, Span};
use crate::view::doc_view;
use crate::view::themes::Typo;

/// Максимум хранимых попаданий (полный счётчик в фоне идёт дальше; навигация
/// ходит по хранимым). 100 000 × 40 байт = 4 МБ на запрос.
pub const MAX_HITS: usize = 100_000;
/// Штук в пакете результатов фонового потока (канал не раздувается).
const CHUNK: usize = 4096;

/// Попадание: диапазон символов [start, end) в полном тексте под-области.
/// Порядок документа: (block, sub, start) возрастает.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    pub block: usize,
    pub sub: usize,
    pub start: usize,
    pub end: usize,
}

/// Пакет из фоновой нити.
enum FindMsg {
    Hits { generation: u64, hits: Vec<Hit> },
    Done { generation: u64, total: usize },
}

/// Результат прокачки канала за кадр.
pub struct Pumped {
    pub changed: bool,
    /// Первые попадания текущего запроса (выбрать стартовое и проскроллить).
    pub landed: bool,
}

/// Состояние поиска одной вкладки (как выделение — живёт в Tab).
pub struct FindState {
    pub open: bool,
    pub query: String,
    /// Накопленные попадания в порядке документа (кап MAX_HITS).
    pub hits: Vec<Hit>,
    /// Полное число попаданий (больше hits.len() при срезе по капу).
    pub total: usize,
    /// Текущее попадание (индекс в hits; Enter/F3 ходят по кругу).
    pub current: usize,
    /// Поиск ещё идёт (счётчик с «…»).
    pub searching: bool,
    /// Каретка своего поля ввода — индекс символа в query.
    pub caret: usize,
    /// Якорь выделения поля (равен caret — выделения нет).
    pub sel_anchor: usize,
    /// Серия кликов по полю (двойной/тройной): (время, место, номер).
    pub clicks: Option<(std::time::Instant, Pos2, u8)>,
    /// Позиция попапа на экране (None — по умолчанию, правый верх).
    pub pos: Option<Pos2>,
    /// Прямоугольник попапа прошлого кадра — выделение документа под ним
    /// глушится (иначе драг по полю выделяет текст позади попапа).
    pub popup_rect: Option<eframe::egui::Rect>,
    /// Поколение активного поиска (устаревшие пакеты игнорируются).
    generation: u64,
    rx: Option<Receiver<FindMsg>>,
    /// Отмена фоновой нити (новый запрос/закрытие).
    cancel: Option<Arc<AtomicBool>>,
    /// Запрос последнего запущенного поиска (перезапуск при смене).
    started: Option<String>,
    /// Первые попадания запроса ещё не «приземлены».
    landed: bool,
}

impl Default for FindState {
    fn default() -> Self {
        Self {
            open: false,
            query: String::new(),
            hits: Vec::new(),
            total: 0,
            current: 0,
            searching: false,
            caret: 0,
            sel_anchor: 0,
            clicks: None,
            pos: None,
            popup_rect: None,
            generation: 0,
            rx: None,
            cancel: None,
            started: None,
            landed: false,
        }
    }
}

impl FindState {
    /// Запускает поиск по готовому документу (вызывает App при открытой
    /// панели и изменившемся запросе).
    pub fn restart(&mut self, doc: &Arc<Doc>) {
        self.cancel_active();
        self.generation += 1;
        self.hits.clear();
        self.total = 0;
        self.current = 0;
        self.searching = true;
        self.landed = false;
        let (rx, cancel) = spawn_search(doc.clone(), self.query.clone(), self.generation);
        self.rx = Some(rx);
        self.cancel = Some(cancel);
        self.started = Some(self.query.clone());
    }

    /// Закрыть панель: результаты и нить гасим, запрос помним (F3 повторит).
    pub fn close(&mut self) {
        self.open = false;
        self.reset_results();
    }

    /// Снять результаты (пустой запрос / закрытие), не трогая сам запрос.
    pub fn reset_results(&mut self) {
        self.cancel_active();
        self.hits.clear();
        self.total = 0;
        self.current = 0;
        self.searching = false;
        self.rx = None;
        self.cancel = None;
        self.started = None;
        self.landed = false;
    }

    fn cancel_active(&mut self) {
        if let Some(c) = self.cancel.take() {
            c.store(true, Ordering::Relaxed);
        }
    }

    /// Прокачка канала фоновой нити (каждый кадр, все вкладки — чтобы канал
    /// не копился у скрытых).
    pub fn pump(&mut self) -> Pumped {
        let mut res = Pumped {
            changed: false,
            landed: false,
        };
        let Some(rx) = self.rx.take() else {
            return res;
        };
        loop {
            match rx.try_recv() {
                Ok(FindMsg::Hits { generation, hits }) => {
                    if generation != self.generation {
                        continue; // устаревшее поколение
                    }
                    if !self.landed && !hits.is_empty() {
                        self.landed = true;
                        res.landed = true;
                    }
                    if self.hits.len() < MAX_HITS {
                        let room = MAX_HITS - self.hits.len();
                        self.hits.extend(hits.into_iter().take(room));
                    }
                    res.changed = true;
                }
                Ok(FindMsg::Done { generation, total }) => {
                    if generation == self.generation {
                        self.total = total;
                        self.searching = false;
                        res.changed = true;
                    }
                    return res; // канал исчерпан, rx не возвращаем
                }
                Err(TryRecvError::Empty) => {
                    self.rx = Some(rx);
                    return res;
                }
                Err(TryRecvError::Disconnected) => {
                    self.searching = false;
                    return res;
                }
            }
        }
    }

    /// Шаг по попаданиям (Enter/F3/кнопки): вперёд/назад, по кругу.
    pub fn step(&mut self, back: bool) -> Option<Hit> {
        if self.hits.is_empty() {
            return None;
        }
        let n = self.hits.len();
        self.current = if back {
            (self.current + n - 1) % n
        } else {
            (self.current + 1) % n
        };
        self.hits.get(self.current).copied()
    }

    pub fn cur_hit(&self) -> Option<Hit> {
        self.hits.get(self.current).copied()
    }

    /// Запрос, по которому запущен/завершён поиск (None — результатов нет).
    pub fn started(&self) -> Option<&str> {
        self.started.as_deref()
    }
}

// ---------------------------------------------------------------------------
// Фоновой поиск.
// ---------------------------------------------------------------------------

/// Регистронезависимый fold 1:1 (латиница/кириллица складываются в один
/// символ; экзотику не трогаем). Символьные индексы совпадают с исходником.
fn fold(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// Есть буквы с регистром (латиница/кириллица…) — haystack сворачиваем.
fn needs_fold(s: &str) -> bool {
    s.chars().any(|c| c.is_uppercase() || c.is_lowercase())
}

fn fold_str(s: &str) -> String {
    s.chars().map(fold).collect()
}

/// Запуск поиска в фоновой нити: (канал пакетов, флаг отмены).
fn spawn_search(doc: Arc<Doc>, query: String, generation: u64) -> (Receiver<FindMsg>, Arc<AtomicBool>) {
    let (tx, rx) = std::sync::mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancelled = cancel.clone();
    std::thread::spawn(move || {
        let mut buf: Vec<Hit> = Vec::with_capacity(CHUNK);
        let mut total = 0usize;
        search_doc(&doc, &query, |hit| {
            if cancelled.load(Ordering::Relaxed) {
                return false;
            }
            total += 1;
            if buf.len() < CHUNK {
                buf.push(hit);
                if buf.len() == CHUNK
                    && tx.send(FindMsg::Hits {
                        generation,
                        hits: std::mem::take(&mut buf),
                    })
                    .is_err()
                {
                    return false;
                }
            }
            true
        });
        // Хвостовой пакет: без него результаты < CHUNK вообще не доходили
        // (нить считала total, но не отправляла ни одного Hits).
        if !buf.is_empty() {
            let _ = tx.send(FindMsg::Hits {
                generation,
                hits: buf,
            });
        }
        let _ = tx.send(FindMsg::Done { generation, total });
    });
    (rx, cancel)
}

/// Обход всех попаданий документа в порядке чтения; колбэк вернул false —
/// остановка (отмена нити). Ищем по тексту блоков, а не по исходнику:
/// подсветка и навигация живут в координатах (блок/под-область/символ), а
/// `code` и расшифрованные сущности исходником не являются.
fn search_doc(doc: &Doc, query: &str, mut on_hit: impl FnMut(Hit) -> bool) {
    if query.is_empty() {
        return;
    }
    let fold_case = needs_fold(query);
    let needle = if fold_case {
        fold_str(query)
    } else {
        query.to_string()
    };
    let finder = memmem::Finder::new(needle.as_bytes());
    let needle_ch = needle.chars().count();
    let source = doc.source.as_ref();
    for (bi, block) in doc.blocks.iter().enumerate() {
        search_block(block, bi, &finder, needle_ch, fold_case, source, &mut on_hit);
    }
}

/// Синхронный поиск по всему документу (та же машина, что в фоновой нити) —
/// для интеграционных тестов бюджета §7 (первый результат < 300 мс на 50 МБ).
pub fn search_sync(doc: &Doc, query: &str, on_hit: impl FnMut(Hit) -> bool) {
    search_doc(doc, query, on_hit);
}

/// Все под-области блока в порядке документа.
fn search_block(
    block: &BlockKind,
    bi: usize,
    finder: &memmem::Finder,
    needle_ch: usize,
    fold_case: bool,
    source: &str,
    on_hit: &mut impl FnMut(Hit) -> bool,
) {
    match block {
        BlockKind::Heading { spans, .. }
        | BlockKind::Paragraph { spans }
        | BlockKind::Quote { spans, .. }
        | BlockKind::ListItem { spans, .. } => {
            search_spans(bi, 0, spans, finder, needle_ch, fold_case, source, on_hit);
        }
        BlockKind::Code { code, .. } => {
            search_text(bi, 0, code.get(source), finder, needle_ch, fold_case, on_hit);
        }
        BlockKind::Table { aligns, rows } => {
            let ncols = aligns.len().max(1);
            for (ri, row) in rows.iter().enumerate() {
                for (ci, cell) in row.cells.iter().enumerate() {
                    search_spans(
                        bi,
                        ri * ncols + ci,
                        cell,
                        finder,
                        needle_ch,
                        fold_case,
                        source,
                        on_hit,
                    );
                }
            }
        }
        BlockKind::Rule | BlockKind::Image { .. } => {}
    }
}

/// Поиск в конкатенации спанов под-области. Один спан без fold — без копии.
fn search_spans(
    bi: usize,
    sub: usize,
    spans: &[Span],
    finder: &memmem::Finder,
    needle_ch: usize,
    fold_case: bool,
    source: &str,
    on_hit: &mut impl FnMut(Hit) -> bool,
) {
    if !fold_case && spans.len() == 1 {
        search_text(
            bi,
            sub,
            spans[0].text.get(source),
            finder,
            needle_ch,
            fold_case,
            on_hit,
        );
        return;
    }
    let mut text = String::new();
    for span in spans {
        text.push_str(span.text.get(source));
    }
    search_text(bi, sub, &text, finder, needle_ch, fold_case, on_hit);
}

/// Поиск в тексте под-области: байтовые вхождения memmem → символьные Hit
/// (курсоры галлеев — символьные). Свёрнутый текст 1:1 по символам.
fn search_text(
    bi: usize,
    sub: usize,
    text: &str,
    finder: &memmem::Finder,
    needle_ch: usize,
    fold_case: bool,
    on_hit: &mut impl FnMut(Hit) -> bool,
) {
    let needle_bytes = finder.needle().len();
    if text.len() < needle_bytes {
        return;
    }
    let folded;
    let hay = if fold_case {
        folded = fold_str(text);
        folded.as_str()
    } else {
        text
    };
    let (mut byte, mut ch) = (0usize, 0usize);
    // find_iter отдаёт байтовые начала неперекрывающихся вхождений (usize).
    for m in finder.find_iter(hay.as_bytes()) {
        while byte < m {
            match hay[byte..].chars().next() {
                Some(c) => {
                    byte += c.len_utf8();
                    ch += 1;
                }
                None => return,
            }
        }
        let start = ch;
        if !on_hit(Hit {
            block: bi,
            sub,
            start,
            end: start + needle_ch,
        }) {
            return;
        }
        let end = m + needle_bytes;
        while byte < end {
            match hay[byte..].chars().next() {
                Some(c) => {
                    byte += c.len_utf8();
                    ch += 1;
                }
                None => return,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Навигация по попаданиям.
// ---------------------------------------------------------------------------

/// Первое попадание не выше текущего окна (стартуем с видимого), иначе —
/// самое первое. Вызывается при «приземлении» первых результатов запроса.
pub fn initial_hit(hits: &[Hit], layout: &LayoutState, offset_y: f32) -> usize {
    let limit = offset_y - 40.0;
    hits.iter()
        .position(|h| doc_view::TOP_PAD + layout.offset(h.block) >= limit)
        .unwrap_or(0)
}

/// Приблизительный y попадания в координатах контента (для автоскролла):
/// верх блока + оценка строки. Точно для кода (строки не переносятся);
/// для текста — по переводам строк (перенос слов даёт сдвиг вниз — попадание
/// остаётся ниже точки прокрутки, в зоне overscan); таблица — доля строки.
pub fn hit_y(doc: &Doc, layout: &LayoutState, typo: &Typo, hit: Hit) -> f32 {
    let Some(block) = doc.blocks.get(hit.block) else {
        return 0.0;
    };
    let top = doc_view::TOP_PAD + layout.offset(hit.block);
    let em = typo.size;
    match block {
        BlockKind::Code { code, .. } => {
            let line_h = em * 0.84 * 1.55;
            top + measure::gap_code_top(em)
                + 32.0
                + 12.0
                + newlines_before(code.get(&doc.source), hit.start) as f32 * line_h
        }
        BlockKind::Table { aligns, rows } => {
            let ncols = aligns.len().max(1);
            let ri = (hit.sub / ncols) as f32;
            let n = rows.len().max(1) as f32;
            let rows_h = (layout.height(hit.block) - 1.65 * em).max(0.0);
            top + 0.95 * em + (ri + 0.5) / n * rows_h
        }
        BlockKind::Heading { spans, .. }
        | BlockKind::Paragraph { spans }
        | BlockKind::Quote { spans, .. }
        | BlockKind::ListItem { spans, .. } => {
            top + spans_newlines_before(spans, hit.start, &doc.source) as f32 * typo.row_height()
        }
        BlockKind::Rule | BlockKind::Image { .. } => top,
    }
}

/// Переводы строк до символа ch (символьный индекс) в строке.
fn newlines_before(text: &str, ch: usize) -> usize {
    let byte = char_to_byte(text, ch);
    text[..byte].matches('\n').count()
}

/// Переводы строки в конкатенации спанов до символа ch.
fn spans_newlines_before(spans: &[Span], ch: usize, source: &str) -> usize {
    let mut acc = 0usize;
    let mut nl = 0usize;
    for span in spans {
        if acc >= ch {
            break;
        }
        let text = span.text.get(source);
        let take = (ch - acc).min(text.chars().count());
        nl += newlines_before(text, take);
        acc += text.chars().count();
    }
    nl
}

/// Байтовое смещение символьного индекса (поле ввода в find_bar.rs).
pub(super) fn char_to_byte(s: &str, ch: usize) -> usize {
    s.char_indices().nth(ch).map_or(s.len(), |(b, _)| b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::md::parser;

    fn doc(md: &str) -> Doc {
        parser::parse(md.into())
    }

    fn all(md: &str, q: &str) -> Vec<(usize, usize, usize, usize)> {
        let d = doc(md);
        let mut out = Vec::new();
        search_doc(&d, q, |h| {
            out.push((h.block, h.sub, h.start, h.end));
            true
        });
        out
    }

    #[test]
    fn paragraphs_case_insensitive_cyrillic() {
        let hits = all("Привет мир\n\nМИР и мир", "мир");
        assert_eq!(
            hits,
            vec![(0, 0, 7, 10), (1, 0, 0, 3), (1, 0, 6, 9)]
        );
    }

    #[test]
    fn code_table_and_multibyte_offsets() {
        let md = "🎉 тайтл\n\n```rust\nfn find() {}\n```\n\n| Яблоко | Груша |\n|---|---|\n| а | б |";
        // блоки: 0=абзац, 1=код, 2=таблица (шапка — строка 0).
        let hits = all(md, "find");
        assert_eq!(hits, vec![(1, 0, 3, 7)]);
        let hits = all(md, "яблоко");
        assert_eq!(hits, vec![(2, 0, 0, 6)]);
        // Груша — вторая ячейка шапки (sub 1).
        let hits = all(md, "груша");
        assert_eq!(hits, vec![(2, 1, 0, 5)]);
    }

    #[test]
    fn stop_callback() {
        let d = doc("а б а б а б");
        let mut n = 0;
        search_doc(&d, "а", |_| {
            n += 1;
            n < 2
        });
        assert_eq!(n, 2); // остановился после второго
    }

    #[test]
    fn initial_hit_first_at_or_below() {
        let d = doc("первый\n\nвторой\n\nтретий");
        let mut hits = Vec::new();
        search_doc(&d, "тор", |h| {
            hits.push(h);
            true
        });
        let mut layout = LayoutState::default();
        // смоделируем высоты: ensure не звался, но offset() безопасен (0)
        layout.ensure(
            &d,
            crate::layout::LayoutKey {
                typo: Typo::default(),
                width: 800.0,
                dark: true,
                lang: crate::i18n::Lang::Ru,
                code_nums: false,
                table_scroll: true,
            },
        );
        assert_eq!(initial_hit(&hits, &layout, 0.0), 0);
    }

    #[test]
    fn hit_y_code_lines() {
        let d = doc("```\nодин\nдва\nтри\n```");
        let mut hits = Vec::new();
        search_doc(&d, "три", |h| {
            hits.push(h);
            true
        });
        let layout = LayoutState::default();
        let typo = Typo::default();
        let y = hit_y(&d, &layout, &typo, hits[0]);
        // верх кода: TOP_PAD + отступ + шапка + 2 строки до попадания
        let line_h = typo.size * 0.84 * 1.55;
        let want = doc_view::TOP_PAD + measure::gap_code_top(typo.size) + 32.0 + 12.0 + 2.0 * line_h;
        assert!((y - want).abs() < 0.01);
    }

    #[test]
    fn fold_is_one_to_one() {
        assert_eq!(fold('Ы'), 'ы');
        assert_eq!(fold('A'), 'a');
        assert_eq!(fold('1'), '1');
        assert_eq!(fold_str("ЫIndex"), "ыindex");
        assert!(needs_fold("мир"));
        assert!(!needs_fold("123"));
    }

    /// Полный цикл: restart → пакеты из фоновой нити → Done (как в app).
    #[test]
    fn restart_pump_cycle() {
        let doc = Arc::new(parser::parse("Документ demo\n\nтекст Документ".into()));
        let mut f = FindState::default();
        f.open = true;
        f.query = "документ".into();
        f.restart(&doc);
        let mut guard = 0;
        while f.searching && guard < 500 {
            let _ = f.pump();
            guard += 1;
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(!f.searching, "поиск должен завершиться (кадры: {guard})");
        assert_eq!(f.total, 2);
        assert_eq!(f.hits.len(), 2);
        assert_eq!(f.hits[0].block, 0);
        // шаг навигации ходит по кругу
        assert_eq!(f.step(false), Some(f.hits[1]));
        assert_eq!(f.step(false), Some(f.hits[0]));
    }
}

