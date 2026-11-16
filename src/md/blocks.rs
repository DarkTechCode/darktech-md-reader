//! Модель документа: плоский вектор блоков (не знают про экран) + inline-спаны.

use std::ops::Range;
use std::sync::Arc;

/// Разобранный документ. Иммутабелен после парсинга, раскладка — отдельно.
pub struct Doc {
    /// Исходный текст (для поиска, подсветки, копирования).
    pub source: Arc<str>,
    pub blocks: Vec<BlockKind>,
    /// Заголовки для оглавления и якорей; индекс — `heading_idx` в блоках.
    pub headings: Vec<Heading>,
    /// Приблизительное число слов (для статус-бара).
    pub word_count: usize,
}

impl Doc {
    /// Пустой документ-заготовка (растёт по чанкам при фоновой загрузке).
    pub fn empty() -> Self {
        Self {
            source: Arc::from(""),
            blocks: Vec::new(),
            headings: Vec::new(),
            word_count: 0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Heading {
    pub level: u8,
    /// Заголовок чистым текстом.
    pub text: String,
    /// Индекс блока-заголовка в `Doc::blocks`.
    pub block_idx: usize,
}

#[derive(Clone, Debug)]
pub enum BlockKind {
    Heading {
        level: u8,
        spans: Vec<Span>,
        heading_idx: usize,
    },
    Paragraph {
        spans: Vec<Span>,
    },
    /// Цитата (в т.ч. GFM alert). Глубокие цитаты разбиты на блоки по глубине.
    Quote {
        depth: u8,
        alert: Option<Alert>,
        spans: Vec<Span>,
    },
    ListItem {
        /// Уровень вложенности списка, 1 = верхний.
        indent: u8,
        marker: ListMarker,
        /// Чекбокс task list.
        task: Option<bool>,
        /// Продолжение элемента списка (второй абзац и т.п.) — без маркера.
        cont: bool,
        /// «Рыхлый» список — блоки внутри разделены интервалами абзацев.
        loose: bool,
        quote_depth: u8,
        spans: Vec<Span>,
    },
    /// Блок кода (в т.ч. mermaid и html — рендерятся как код с подписью).
    Code {
        /// Язык из fence (первый токен); None для отступных блоков.
        lang: Option<String>,
        code: String,
    },
    Table {
        aligns: Vec<Align>,
        /// rows[0] — шапка.
        rows: Vec<TableRow>,
    },
    Rule,
    /// Картинка-абзац (локальный путь или произвольный URL; загрузка — отдельно).
    Image {
        url: String,
        alt: String,
    },
}

#[derive(Clone, Debug, Default)]
pub struct TableRow {
    pub cells: Vec<Vec<Span>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    None,
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListMarker {
    Bullet,
    Ordered(u64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alert {
    Note,
    Tip,
    Important,
    Warning,
    Caution,
}

/// Inline-фрагмент текста с форматированием.
#[derive(Clone, Debug)]
pub struct Span {
    pub text: String,
    /// Диапазон в `Doc::source` (нет для синтезированных: перевод строки, сущности).
    pub src: Option<Range<usize>>,
    pub fmt: Fmt,
    /// URL ссылки, если спан внутри `[…](…)`.
    pub link: Option<Box<str>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Fmt {
    pub bold: bool,
    pub italic: bool,
    /// Инлайн-код `…`.
    pub code: bool,
    pub strike: bool,
    /// Сырой встроенный HTML — показываем приглушённо.
    pub html: bool,
}
