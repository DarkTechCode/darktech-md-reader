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
    /// Символов в исходнике (для статус-бара).
    pub char_count: usize,
    /// Строк в исходнике (для статус-бара).
    pub line_count: usize,
}

impl Doc {
    /// Пустой документ-заготовка (растёт по чанкам при фоновой загрузке).
    pub fn empty() -> Self {
        Self {
            source: Arc::from(""),
            blocks: Vec::new(),
            headings: Vec::new(),
            word_count: 0,
            char_count: 0,
            line_count: 0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Heading {
    pub level: u8,
    /// Заголовок чистым текстом.
    pub text: String,
    /// Якорь для ссылок `#…` (уникален в документе, стиль GitHub).
    pub slug: String,
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

/// Длина текстового префикса блока-якоря (позиция чтения при перечитывании
/// файла и восстановлении сессии).
pub const ANCHOR_LEN: usize = 64;

/// Текстовый префикс блока — «отпечаток» для сверки позиции чтения после
/// изменения файла (индексы блоков при перечитывании плывут). None — блок
/// без текста (разделитель).
pub fn text_prefix(doc: &Doc, i: usize) -> Option<String> {
    let mut out = String::new();
    let mut add = |s: &str| {
        if out.chars().count() < ANCHOR_LEN {
            let remain = ANCHOR_LEN - out.chars().count();
            out.extend(s.chars().take(remain));
        }
    };
    match doc.blocks.get(i)? {
        BlockKind::Heading { spans, .. }
        | BlockKind::Paragraph { spans }
        | BlockKind::Quote { spans, .. }
        | BlockKind::ListItem { spans, .. } => {
            for s in spans {
                add(&s.text);
            }
        }
        BlockKind::Code { code, .. } => add(code),
        BlockKind::Table { rows, .. } => {
            if let Some(cell) = rows.first().and_then(|r| r.cells.first()) {
                for s in cell {
                    add(&s.text);
                }
            }
        }
        BlockKind::Image { alt, .. } => add(alt),
        BlockKind::Rule => {}
    }
    let out = out.trim().to_string();
    (!out.is_empty()).then_some(out)
}

/// Ищет блок с тем же текстовым префиксом (якорь позиции в новом документе).
pub fn find_by_prefix(doc: &Doc, prefix: &str) -> Option<usize> {
    (0..doc.blocks.len()).find(|&i| text_prefix(doc, i).as_deref() == Some(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::md::parser;

    fn doc(text: &str) -> Doc {
        parser::parse(std::sync::Arc::from(text))
    }

    #[test]
    fn prefix_and_find_survive_edits_above() {
        let d = doc("# Первый\n\nтекст один\n\n## Второй\n\nтекст два\n\n---\n");
        assert_eq!(text_prefix(&d, 0).as_deref(), Some("Первый"));
        assert_eq!(text_prefix(&d, 1).as_deref(), Some("текст один"));
        assert_eq!(text_prefix(&d, 3).as_deref(), Some("текст два"));
        assert_eq!(text_prefix(&d, 5), None, "у разделителя текста нет");

        // Вставка блока выше: якорь «текст два» находит НОВЫЙ индекс.
        let d2 = doc("# Первый\n\nвставка\n\nтекст один\n\n## Второй\n\nтекст два\n\n---\n");
        assert_eq!(find_by_prefix(&d2, "текст два"), Some(4));
        assert_eq!(find_by_prefix(&d2, "Первый"), Some(0));
        assert_eq!(find_by_prefix(&d2, "нет такого"), None);
    }

    #[test]
    fn prefix_truncates_to_anchor_len() {
        let long = "а".repeat(ANCHOR_LEN + 50);
        let d = doc(format!("{long}\n").as_str());
        assert_eq!(
            text_prefix(&d, 0).map(|p| p.chars().count()),
            Some(ANCHOR_LEN)
        );
    }
}
