//! Базовые проверки парсера: структура блоков из типового markdown.

use std::sync::Arc;

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

#[test]
fn source_ranges_preserved() {
    let src = "Первый абзац сохраняет диапазоны.\n\nВторой тоже.\n";
    let doc = parse(Arc::from(src));
    for block in &doc.blocks {
        if let BlockKind::Paragraph { spans } = block {
            for span in spans {
                let r = span.src.clone().expect("src range");
                assert_eq!(&src[r], span.text, "спан совпадает с source");
            }
        }
    }
}

#[test]
fn code_block_keeps_text() {
    let src = "```python\nprint('привет')\n```\n";
    let doc = parse(Arc::from(src));
    match &doc.blocks[0] {
        BlockKind::Code { lang, code } => {
            assert_eq!(lang.as_deref(), Some("python"));
            assert!(code.contains("привет"));
        }
        other => panic!("ожидался Code, получен {other:?}"),
    }
}

#[test]
fn word_count() {
    let doc = parse(Arc::from("раз два три четыре\nпять\n"));
    assert_eq!(doc.word_count, 5);
}
