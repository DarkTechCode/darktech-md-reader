//! pulldown-cmark → плоский вектор блоков.
//! Контейнеры (цитаты, списки) «разглаживаются»: вложенное содержимое становится
//! отдельными блоками с метаданными глубины/маркера.
//! Большие файлы разбираются в фоновом потоке чанками (стриминг в UI).

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::Sender;

use pulldown_cmark::{BlockQuoteKind, CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

use super::anchors::unique_slug;
use super::blocks::{Align, Alert, BlockKind, Doc, Fmt, Heading, ListMarker, Span, TableRow, Txt};

/// Файлы меньше этого размера читаются и разбираются синхронно (мгновенно).
pub const SYNC_LIMIT: u64 = 1 << 20;
/// Блоков в одном чанке стриминга.
pub const CHUNK_BLOCKS: usize = 512;

/// Сообщение фонового разбора.
pub enum ParseMsg {
    /// Исходный текст — первым сообщением сразу после чтения файла:
    /// блоки заимствуют из него строки (Txt::Src), UI может рендерить
    /// чанки ещё до конца разбора.
    Source(Arc<str>),
    Chunk {
        blocks: Vec<BlockKind>,
        headings: Vec<Heading>,
    },
    Done {
        word_count: usize,
        char_count: usize,
        line_count: usize,
    },
    Failed(String),
}

/// Читает файл как UTF-8 (с BOM); CRLF/LF не важны.
pub fn read_text(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let mut text =
        String::from_utf8(bytes).map_err(|_| format!("{}: не UTF-8", path.display()))?;
    if text.starts_with('\u{FEFF}') {
        text.replace_range(0..3, "");
    }
    Ok(text)
}

/// Фоновая загрузка большого файла: чтение + разбор в потоке, блоки
/// приходят чанками; в конце — весь исходник и счётчик слов.
pub fn parse_background(path: PathBuf, tx: Sender<ParseMsg>) {
    std::thread::spawn(move || {
        let text = match read_text(&path) {
            Ok(t) => t,
            Err(e) => {
                let _ = tx.send(ParseMsg::Failed(e));
                return;
            }
        };
        let source: Arc<str> = Arc::from(text.as_str());
        if tx.send(ParseMsg::Source(source.clone())).is_err() {
            return; // вкладку закрыли — бросаем разбор
        }
        let mut st = State::new();
        for (event, range) in Parser::new_ext(&source, opts()).into_offset_iter() {
            handle_event(&mut st, event, range, &source);
            if st.blocks.len() >= CHUNK_BLOCKS && !st.send_chunk(&tx) {
                return; // вкладку закрыли — бросаем разбор
            }
        }
        st.send_chunk(&tx);
        let (char_count, line_count) = count_stats(&source);
        let _ = tx.send(ParseMsg::Done {
            word_count: source.split_whitespace().count(),
            char_count,
            line_count,
        });
    });
}

fn opts() -> Options {
    let mut opts = Options::empty();
    opts.insert(
        Options::ENABLE_TABLES
            | Options::ENABLE_STRIKETHROUGH
            | Options::ENABLE_TASKLISTS
            | Options::ENABLE_GFM,
    );
    opts
}

/// Модификатор inline-контекста (стек: жирный внутри ссылки и т.п.).
#[derive(Clone, Debug)]
enum Inline {
    Emph,
    Strong,
    Strike,
    Link(String),
}

#[derive(Default)]
struct ListCtx {
    ordered: Option<u64>,
    /// Номер текущего пункта нумерованного списка.
    number: u64,
    next_number: u64,
    /// Уже был блок в текущем пункте (для `cont`).
    item_has_block: bool,
    /// Чекбокс текущего пункта.
    item_task: Option<bool>,
    /// Список с пустыми строками между пунктами.
    loose: bool,
}

struct TableCtx {
    aligns: Vec<Align>,
    rows: Vec<TableRow>,
    row: Vec<Vec<Span>>,
    in_head: bool,
}

/// Собираемый блок кода: текст копится, диапазон — от первого до последнего
/// события; если текст совпал со срезом исходника, код заимствуется из него.
struct CodeAcc {
    lang: Option<String>,
    text: String,
    start: usize,
    end: usize,
}

struct State {
    blocks: Vec<BlockKind>,
    headings: Vec<Heading>,
    /// Занятые якоря-слаги (дедупликация по всему документу, между чанками).
    slugs: std::collections::HashSet<String>,
    /// Накопленные inline-спаны текущего блока.
    spans: Vec<Span>,
    /// Стек inline-модификаторов.
    inline: Vec<Inline>,
    lists: Vec<ListCtx>,
    quote_depth: u8,
    alert: Option<Alert>,
    /// Уровень заголовка, который сейчас собираем.
    heading: Option<u8>,
    /// Собираемый блок кода.
    code: Option<CodeAcc>,
    /// Собираемый блок HTML: текст + охват диапазонов событий.
    html: String,
    html_range: Option<Range<usize>>,
    in_html_block: bool,
    table: Option<TableCtx>,
    /// Кандидат на «картинку-абзац»: (url, alt). Действует, пока абзац
    /// не получил другого контента.
    para_img: Option<(String, String)>,
    /// Внутри alt такой картинки.
    in_img_alt: bool,
    para_has_other: bool,
    /// Блоков уже отправлено чанками (block_idx заголовков — сквозной).
    blocks_sent: usize,
}

impl State {
    fn new() -> Self {
        Self {
            blocks: Vec::new(),
            headings: Vec::new(),
            slugs: std::collections::HashSet::new(),
            spans: Vec::new(),
            inline: Vec::new(),
            lists: Vec::new(),
            quote_depth: 0,
            alert: None,
            heading: None,
            code: None,
            html: String::new(),
            html_range: None,
            in_html_block: false,
            table: None,
            para_img: None,
            in_img_alt: false,
            para_has_other: false,
            blocks_sent: 0,
        }
    }

    /// Отправляет накопленные блокы чанком. false — приёмник умер (вкладку закрыли).
    fn send_chunk(&mut self, tx: &Sender<ParseMsg>) -> bool {
        if self.blocks.is_empty() {
            return true;
        }
        let n = self.blocks.len();
        let msg = ParseMsg::Chunk {
            blocks: std::mem::take(&mut self.blocks),
            headings: std::mem::take(&mut self.headings),
        };
        self.blocks_sent += n;
        tx.send(msg).is_ok()
    }
}

pub fn parse(source: Arc<str>) -> Doc {
    let mut st = State::new();
    for (event, range) in Parser::new_ext(&source, opts()).into_offset_iter() {
        handle_event(&mut st, event, range, &source);
    }
    let (char_count, line_count) = count_stats(&source);
    Doc {
        word_count: source.split_whitespace().count(),
        char_count,
        line_count,
        source,
        blocks: st.blocks,
        headings: st.headings,
    }
}

/// Символы и строки исходника (для статус-бара). Один проход.
fn count_stats(source: &str) -> (usize, usize) {
    let chars = source.chars().count();
    let lines = if source.is_empty() {
        0
    } else {
        source.bytes().filter(|b| *b == b'\n').count() + 1
    };
    (chars, lines)
}

fn handle_event(st: &mut State, event: Event, range: Range<usize>, source: &str) {
    match event {
        Event::Start(tag) => start_tag(st, tag),
        Event::End(end) => end_tag(st, end, source),
        Event::Text(text) => push_text(st, text.into_string(), range, source),
        Event::Code(text) => push_code(st, text.into_string(), range, source),
        Event::Html(html) => {
            if st.in_html_block {
                st.html.push_str(&html);
                let r = st.html_range.get_or_insert(range.clone());
                r.start = r.start.min(range.start);
                r.end = r.end.max(range.end);
            } else {
                push_inline_html(st, &html);
            }
        }
        Event::InlineHtml(html) => push_inline_html(st, &html),
        Event::SoftBreak => push_synthetic(st, " "),
        Event::HardBreak => push_synthetic(st, "\n"),
        Event::Rule => {
            flush_before_block(st);
            st.blocks.push(BlockKind::Rule);
        }
        Event::TaskListMarker(checked) => {
            if let Some(list) = st.lists.last_mut() {
                list.item_task = Some(checked);
            }
        }
        // Сноски не поддерживаем: ходить по ним не нужно, ссылку опускаем.
        Event::FootnoteReference(_) => {}
        // Опции выключены, эти события не приходят.
        Event::InlineMath(_) | Event::DisplayMath(_) => {}
    }
}

/// Вложенный блок (список/цитата/код внутри пункта) прерывает накопление:
/// сначала фиксируем текущий блок, потом начинаем новый.
fn flush_before_block(st: &mut State) {
    if !st.spans.is_empty() {
        flush_spans(st);
    }
}

fn start_tag(st: &mut State, tag: Tag) {
    match tag {
        Tag::Paragraph => {
            // Абзац внутри пункта списка помечает список как «рыхлый».
            if let Some(list) = st.lists.last_mut() {
                list.loose = true;
            }
            st.para_img = None;
            st.para_has_other = false;
        }
        Tag::Heading { level, .. } => {
            flush_before_block(st);
            st.heading = Some(level as u8);
            st.spans.clear();
        }
        Tag::BlockQuote(kind) => {
            flush_before_block(st);
            st.quote_depth += 1;
            if st.quote_depth == 1 {
                st.alert = kind.map(|k| match k {
                    BlockQuoteKind::Note => Alert::Note,
                    BlockQuoteKind::Tip => Alert::Tip,
                    BlockQuoteKind::Important => Alert::Important,
                    BlockQuoteKind::Warning => Alert::Warning,
                    BlockQuoteKind::Caution => Alert::Caution,
                });
            }
        }
        Tag::CodeBlock(kind) => {
            flush_before_block(st);
            let lang = match kind {
                CodeBlockKind::Indented => None,
                CodeBlockKind::Fenced(info) => info
                    .trim()
                    .split_whitespace()
                    .next()
                    .map(|s| s.to_ascii_lowercase()),
            };
            st.code = Some(CodeAcc {
                lang,
                text: String::new(),
                start: usize::MAX,
                end: 0,
            });
        }
        Tag::HtmlBlock => {
            flush_before_block(st);
            st.in_html_block = true;
        }
        Tag::List(start) => {
            flush_before_block(st);
            st.lists.push(ListCtx {
                ordered: start,
                next_number: start.unwrap_or(1),
                ..Default::default()
            });
        }
        Tag::Item => {
            if let Some(list) = st.lists.last_mut() {
                list.number = list.next_number;
                if list.ordered.is_some() {
                    list.next_number += 1;
                }
                list.item_has_block = false;
                list.item_task = None;
            }
        }
        Tag::Table(aligns) => {
            flush_before_block(st);
            st.table = Some(TableCtx {
                aligns: aligns
                    .into_iter()
                    .map(|a| match a {
                        pulldown_cmark::Alignment::None => Align::None,
                        pulldown_cmark::Alignment::Left => Align::Left,
                        pulldown_cmark::Alignment::Center => Align::Center,
                        pulldown_cmark::Alignment::Right => Align::Right,
                    })
                    .collect(),
                rows: Vec::new(),
                row: Vec::new(),
                in_head: false,
            });
        }
        Tag::TableHead => {
            if let Some(t) = st.table.as_mut() {
                t.in_head = true;
            }
        }
        Tag::TableRow | Tag::TableCell => {}
        Tag::Emphasis => st.inline.push(Inline::Emph),
        Tag::Strong => st.inline.push(Inline::Strong),
        Tag::Strikethrough => st.inline.push(Inline::Strike),
        Tag::Link { dest_url, .. } => st.inline.push(Inline::Link(dest_url.into_string())),
        Tag::Image { dest_url, .. } => {
            // Кандидат на «картинку-абзац»: картинка — первое содержимое абзаца.
            let standalone = st.spans.is_empty()
                && !st.para_has_other
                && st.para_img.is_none()
                && st.heading.is_none()
                && st.table.is_none()
                && st.code.is_none();
            if standalone {
                st.para_img = Some((dest_url.into_string(), String::new()));
                st.in_img_alt = true;
            }
        }
        // Над/подстрочные знаки рендерим как обычный текст.
        Tag::Superscript | Tag::Subscript => {}
        // Опции выключены (definition list, metadata), эти события не приходят.
        Tag::DefinitionList
        | Tag::DefinitionListTitle
        | Tag::DefinitionListDefinition
        | Tag::MetadataBlock(_)
        | Tag::FootnoteDefinition(_) => {}
    }
}

fn end_tag(st: &mut State, end: TagEnd, source: &str) {
    match end {
        TagEnd::Paragraph => flush_para(st),
        TagEnd::Heading(_) => flush_heading(st, source),
        TagEnd::BlockQuote(_) => {
            st.quote_depth = st.quote_depth.saturating_sub(1);
            if st.quote_depth == 0 {
                st.alert = None;
            }
        }
        TagEnd::CodeBlock => {
            if let Some(acc) = st.code.take() {
                let code = borrow_or_own(acc.text, acc.start..acc.end, source);
                st.blocks.push(BlockKind::Code {
                    lang: acc.lang,
                    code,
                });
            }
        }
        TagEnd::HtmlBlock => {
            st.in_html_block = false;
            let code = std::mem::take(&mut st.html);
            let range = st.html_range.take().unwrap_or(0..0);
            if !code.trim().is_empty() {
                st.blocks.push(BlockKind::Code {
                    lang: Some("html".into()),
                    code: borrow_or_own(code, range, source),
                });
            }
        }
        TagEnd::List(_) => {
            st.lists.pop();
        }
        TagEnd::Item => flush_spans(st),
        TagEnd::Table => {
            if let Some(t) = st.table.take() {
                if !t.rows.is_empty() {
                    st.blocks.push(BlockKind::Table {
                        aligns: t.aligns,
                        rows: t.rows,
                    });
                }
            }
        }
        TagEnd::TableHead => {
            if let Some(t) = st.table.as_mut() {
                t.in_head = false;
                let row = std::mem::take(&mut t.row);
                t.rows.push(TableRow { cells: row });
            }
        }
        TagEnd::TableRow => {
            if let Some(t) = st.table.as_mut() {
                let row = std::mem::take(&mut t.row);
                t.rows.push(TableRow { cells: row });
            }
        }
        TagEnd::TableCell => {
            if let Some(t) = st.table.as_mut() {
                let cell = std::mem::take(&mut st.spans);
                t.row.push(cell);
            }
        }
        TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => {
            st.inline.pop();
        }
        TagEnd::Link => {
            st.inline.pop();
        }
        TagEnd::Image => st.in_img_alt = false,
        TagEnd::Superscript | TagEnd::Subscript => {}
        TagEnd::DefinitionList
        | TagEnd::DefinitionListTitle
        | TagEnd::DefinitionListDefinition
        | TagEnd::MetadataBlock(_)
        | TagEnd::FootnoteDefinition => {}
    }
}

/// Картинка-абзац: абзац, состоящий из одного изображения.
fn flush_para(st: &mut State) {
    st.in_img_alt = false;
    if let Some((url, alt)) = st.para_img.take() {
        if !st.para_has_other {
            st.blocks.push(BlockKind::Image { url, alt });
            return;
        }
    }
    flush_spans(st);
}

fn flush_heading(st: &mut State, source: &str) {
    let level = st.heading.take().unwrap_or(6);
    let spans = std::mem::take(&mut st.spans);
    let text: String = spans.iter().map(|s| s.text.get(source)).collect();
    let heading_idx = st.headings.len();
    let slug = unique_slug(&text, &mut st.slugs);
    st.headings.push(Heading {
        level,
        text,
        slug,
        // Сквозной индекс: блоки частично уже отправлены чанками.
        block_idx: st.blocks_sent + st.blocks.len(),
    });
    st.blocks.push(BlockKind::Heading {
        level,
        spans,
        heading_idx,
    });
}

/// Превращает накопленные спаны в блок контента (абзац/цитату/пункт списка).
fn flush_spans(st: &mut State) {
    if st.spans.is_empty() {
        return;
    }
    let spans = std::mem::take(&mut st.spans);
    let quote_depth = st.quote_depth;

    if let Some(list) = st.lists.last_mut() {
        let cont = list.item_has_block;
        let task = if cont { None } else { list.item_task.take() };
        let marker = match list.ordered {
            Some(_) => ListMarker::Ordered(list.number),
            None => ListMarker::Bullet,
        };
        let loose = list.loose;
        list.item_has_block = true;
        let indent = st.lists.len() as u8;
        st.blocks.push(BlockKind::ListItem {
            indent,
            marker,
            task,
            cont,
            loose,
            quote_depth,
            spans,
        });
        return;
    }

    if quote_depth > 0 {
        let alert = if quote_depth == 1 { st.alert } else { None };
        st.blocks.push(BlockKind::Quote {
            depth: quote_depth,
            alert,
            spans,
        });
        return;
    }

    st.blocks.push(BlockKind::Paragraph { spans });
}

/// Текст события: в блоке кода копится код, в alt картинки — alt, иначе — спан.
fn push_text(st: &mut State, text: String, range: Range<usize>, source: &str) {
    if let Some(acc) = st.code.as_mut() {
        acc.text.push_str(&text);
        acc.start = acc.start.min(range.start);
        acc.end = acc.end.max(range.end);
        return;
    }
    if st.in_img_alt {
        if let Some((_, alt)) = st.para_img.as_mut() {
            alt.push_str(&text);
            return;
        }
    }
    st.para_has_other = true;
    st.spans.push(Span {
        text: borrow_or_own(text, range, source),
        fmt: current_fmt(st),
        link: current_link(st),
    });
}

/// Инлайн-код `…`.
fn push_code(st: &mut State, text: String, range: Range<usize>, source: &str) {
    st.para_has_other = true;
    let mut fmt = current_fmt(st);
    fmt.code = true;
    st.spans.push(Span {
        text: borrow_or_own(text, range, source),
        fmt,
        link: current_link(st),
    });
}

/// Синтезированный спан (перевод строки и т.п.): текст отличается от среза.
fn push_synthetic(st: &mut State, text: &str) {
    if st.in_img_alt {
        if let Some((_, alt)) = st.para_img.as_mut() {
            alt.push(' ');
            return;
        }
    }
    if st.code.is_some() {
        return;
    }
    st.para_has_other = true;
    st.spans.push(Span {
        text: Txt::Own(text.into()),
        fmt: current_fmt(st),
        link: current_link(st),
    });
}

/// Заимствует текст из исходника, если он точно совпал со срезом диапазона
/// (сущности `&amp;` и нормализации не совпадут — останутся своей строкой).
fn borrow_or_own(text: String, range: Range<usize>, source: &str) -> Txt {
    if source.get(range.clone()) == Some(text.as_str()) {
        Txt::Src(range)
    } else {
        Txt::Own(text)
    }
}

fn push_inline_html(st: &mut State, html: &str) {
    st.para_has_other = true;
    st.spans.push(Span {
        text: Txt::Own(html.to_string()),
        fmt: Fmt {
            html: true,
            ..current_fmt(st)
        },
        link: None,
    });
}

fn current_fmt(st: &State) -> Fmt {
    let mut fmt = Fmt::default();
    for item in &st.inline {
        match item {
            Inline::Emph => fmt.italic = true,
            Inline::Strong => fmt.bold = true,
            Inline::Strike => fmt.strike = true,
            Inline::Link(_) => {}
        }
    }
    fmt
}

fn current_link(st: &State) -> Option<Box<str>> {
    st.inline
        .iter()
        .rev()
        .find_map(|i| match i {
            Inline::Link(url) => Some(url.as_str().into()),
            _ => None,
        })
}
