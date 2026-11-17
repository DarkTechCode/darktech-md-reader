//! Базовые проверки парсера: структура блоков из типового markdown.

use std::sync::Arc;

use darktech_md::md::anchors;
use darktech_md::md::blocks::{BlockKind, ListMarker};
use darktech_md::md::parser::parse;

#[test]
fn typical_document() {
    let src = "\
# Заголовок

Абзац с **жирным**, *курсивом*, `кодом` и [ссылкой](https://example.com).

## Второй

- пункт раз
- пункт два
  - вложенный

1. первый
2. второй

> Цитата обычная

> [!NOTE]
> Алерт

```rust
fn main() {}
```

| A | B |
|---|--:|
| 1 | 2 |

---

![alt-текст](img/pic.png)
";
    let doc = parse(Arc::from(src));

    let count = |pred: &dyn Fn(&BlockKind) -> bool| doc.blocks.iter().filter(|b| pred(b)).count();

    assert_eq!(
        count(&|b| matches!(b, BlockKind::Heading { level: 1, .. })),
        1,
        "h1"
    );
    assert_eq!(
        count(&|b| matches!(b, BlockKind::Heading { level: 2, .. })),
        1,
        "h2"
    );
    assert_eq!(count(&|b| matches!(b, BlockKind::Paragraph { .. })), 1);
    // 5 пунктов списка: раз, два, вложенный + 2 нумерованных
    assert_eq!(
        count(&|b| matches!(b, BlockKind::ListItem { .. })),
        5,
        "list items"
    );
    assert_eq!(
        count(&|b| matches!(
            b,
            BlockKind::ListItem {
                marker: ListMarker::Ordered(1),
                ..
            }
        )),
        1,
        "ordered start"
    );
    assert_eq!(count(&|b| matches!(b, BlockKind::Quote { .. })), 2);
    assert_eq!(
        count(&|b| matches!(
            b,
            BlockKind::Quote {
                alert: Some(_),
                ..
            }
        )),
        1,
        "alert"
    );
    assert_eq!(
        count(&|b| matches!(b, BlockKind::Code { .. })),
        1,
        "code"
    );
    assert_eq!(count(&|b| matches!(b, BlockKind::Table { .. })), 1);
    assert_eq!(count(&|b| matches!(b, BlockKind::Rule)), 1);
    assert_eq!(count(&|b| matches!(b, BlockKind::Image { .. })), 1);

    // Оглавление: два заголовка
    assert_eq!(doc.headings.len(), 2);
    assert_eq!(doc.headings[0].text, "Заголовок");
}

#[test]
fn task_list_and_tight_spacing() {
    let src = "- [ ] todo\n- [x] done\n";
    let doc = parse(Arc::from(src));
    let tasks: Vec<_> = doc
        .blocks
        .iter()
        .filter_map(|b| match b {
            BlockKind::ListItem { task, .. } => Some(*task),
            _ => None,
        })
        .collect();
    assert_eq!(tasks, vec![Some(false), Some(true)]);
}

/// Обычный текст заимствуется из исходника (Txt::Src — без дубля строк),
/// декодированные сущности остаются собственной строкой (Txt::Own).
#[test]
fn spans_borrow_source_except_entities() {
    use darktech_md::md::blocks::Txt;
    let src = "Первый абзац сохраняет диапазоны.\n\nСущность &amp; и текст.\n";
    let doc = parse(Arc::from(src));
    let BlockKind::Paragraph { spans } = &doc.blocks[0] else {
        panic!("ожидался абзац");
    };
    assert!(
        spans.iter().all(|s| matches!(s.text, Txt::Src(_))),
        "обычный текст заимствован из source"
    );

    let BlockKind::Paragraph { spans } = &doc.blocks[1] else {
        panic!("ожидался абзац");
    };
    let entity = spans
        .iter()
        .find(|s| matches!(s.text, Txt::Own(_)))
        .expect("сущность декодирована — хранится своей строкой");
    assert_eq!(entity.text.get(&doc.source), "&");
    // Код — точный срез исходника: тоже заимствуется.
    let doc2 = parse(Arc::from("```python\nprint('привет')\n```\n"));
    match &doc2.blocks[0] {
        BlockKind::Code { code, .. } => {
            assert!(matches!(code, Txt::Src(_)));
            assert!(code.get(&doc2.source).contains("привет"));
        }
        other => panic!("ожидался Code, получен {other:?}"),
    }
}

#[test]
fn code_block_keeps_text() {
    let src = "```python\nprint('привет')\n```\n";
    let doc = parse(Arc::from(src));
    match &doc.blocks[0] {
        BlockKind::Code { lang, code } => {
            assert_eq!(lang.as_deref(), Some("python"));
            assert!(code.get(&doc.source).contains("привет"));
        }
        other => panic!("ожидался Code, получен {other:?}"),
    }
}

#[test]
fn word_count() {
    let doc = parse(Arc::from("раз два три четыре\nпять\n"));
    assert_eq!(doc.word_count, 5);
}

#[test]
fn anchors_slug_and_resolve() {
    let src = "# Введение\n\nтекст\n\n## Введение\n\n# Заключение\n";
    let doc = parse(Arc::from(src));
    let slugs: Vec<&str> = doc.headings.iter().map(|h| h.slug.as_str()).collect();
    assert_eq!(slugs, vec!["введение", "введение-1", "заключение"]);

    // Якорь находит индекс блока-заголовка.
    let blk = anchors::resolve(&doc, "#введение-1").expect("повторный slug");
    assert!(matches!(doc.blocks[blk], BlockKind::Heading { level: 2, .. }));

    // Кириллица в percent-кодировке (GitHub-стиль ссылок) и по тексту.
    let percent = format!("#{}", percent_encode("заключение"));
    assert_eq!(anchors::resolve(&doc, &percent), anchors::resolve(&doc, "#Заключение"));
    assert!(anchors::resolve(&doc, "#нет-такого").is_none());
}

/// Кодирование не-ASCII байтов в %XX (для проверки декодера).
fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}
