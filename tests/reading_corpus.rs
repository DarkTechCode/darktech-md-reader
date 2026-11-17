//! Корпус «дня чтения» (PLAN §10): сложные, но реальные места файлов владельца.
//! Каждый кейс парсится и ПРОГОНЯЕТСЯ ЧЕРЕЗ ИЗМЕРИТЕЛЬ всех блоков во всех
//! трёх выравниваниях — ловим паники/бесконечности до ручного чтения
//! (грабля Center-ггаллеев этапа 32, длинные слова, вложенность, RTL/эмодзи).

use std::sync::Arc;

use darktech_md::layout::measure::{estimate, measure_block};
use darktech_md::md::parser::parse;
use darktech_md::settings::model::TextAlign;
use darktech_md::view::themes::{self, Typo};
use darktech_md::view::Env;
use darktech_md::{fonts, i18n::Lang};
use eframe::egui;

/// Один кейс корпуса: имя + исходник.
fn corpus() -> Vec<(&'static str, String)> {
    vec![
        ("bom_crlf", format!("\u{FEFF}# Заголовок\r\n\r\nАбзац с CRLF.\r\n\r\n- пункт\r\n- ещё\r\n\r\n```rust\r\nlet x = 1;\r\n```\r\n")),
        ("setext_breaks", "\
Заголовок setext
================

Второй уровень
--------------

Строка с жёстким переносом\
  (два пробела в конце).

Мягкий
перенос строки.
".into()),
        ("long_word", {
            // Одно «слово» на 120k символов — перенос обязан рубить по ширине.
            let w = "длинноеслово".repeat(10_000);
            format!("До {w} после\n")
        }),
        ("long_code_line", {
            let line = "x".repeat(50_000);
            format!("```\n{line}\nкороткая\n```\n")
        }),
        ("deep_quotes", format!("{}глубина цитаты\n", "> ".repeat(30))),
        ("deep_lists", {
            let mut s = String::new();
            for i in 0..20 {
                s.push_str(&" ".repeat(i * 2));
                s.push_str(&format!("- уровень {i}\n"));
            }
            s
        }),
        ("wide_table", {
            let mut s = String::from("|");
            for c in 0..60 { s.push_str(&format!(" кол{c} |")); }
            s.push_str("\n|");
            for _ in 0..60 { s.push_str("---|"); }
            for r in 0..5 {
                s.push_str("\n|");
                for c in 0..60 { s.push_str(&format!(" {r}×{c} |")); }
            }
            s.push('\n');
            s
        }),
        ("unicode_mixed", "\
# Эмодзи и скрипты 👨‍👩‍👧‍👦

Zalgo: Z̸̢̛a̷l̶g̵o̸ ̶t̴e̶x̷t̸ и combining: é è ê.

RTL: مرحبا بالعالم и שלום עולם вперемешку с LTR.

CJK: 日本語のテキスト、中文文本、한국어.

Флаги 🇷🇺🇯🇵 и мат. символы ∀ε>0 ∃δ.
".into()),
        ("entities", "\
Сущности: &amp; &lt; &gt; &quot; &nbsp; &#1040; &#x42F; &copy; &hellip;

И **жирный с &amp; внутри** и `код с &amp;`.
".into()),
        ("html_autolinks", "\
Автоссылка: <https://example.com/path?q=1&x=2> и почта <mailto:a@b.ru>.

<span style=\"color:red\">Inline HTML</span> в абзаце.

<div>
Блочный HTML
</div>

Текст после.
".into()),
        ("nested_emphasis", "\
***Жирный курсив***, ~~зачёркнутый~~, **жирный с *курсивом* и `кодом`**.

[ссылка **с жирным** и `кодом`](https://example.com).

``код с бэктиками: `inside` `` и `` `двойные` ``.
".into()),
        ("footnote_syntax", "\
Текст со сноской[^1] и ещё[^подробно].

[^1]: Текст сноски (рендерится как текст).

Дальше.
".into()),
        ("alerts_all", "\
> [!NOTE]
> Заметка

> [!TIP]
> Подсказка

> [!IMPORTANT]
> Важно

> [!WARNING]
> Предупреждение

> [!CAUTION]
> Опасность

> Обычная цитата для контраста
".into()),
        ("task_nested", "\
- [ ] задача
  - [x] выполнена
    - [ ] глубже
- обычный пункт
  1. нумерованный внутри
     - маркированный глубже
".into()),
        ("heading_only_formatting", "\
# **Жирный заголовок**

## `Код в заголовке`

### [Ссылка в заголовке](https://example.com)

#### Заголовок с эмодзи 🎯 и сущностью &amp;
".into()),
        ("edge_content", "\
---

---

![](empty-alt.png)

![Очень длинный alt-текст, который никуда не выводится, но парсится](pic.png)

&nbsp;
".into()),
        ("only_whitespace", "   \n\n\t\n  \n".into()),
        ("empty", String::new()),
    ]
}

#[test]
fn corpus_parses_and_measures_all_blocks() {
    let ctx = egui::Context::default();
    fonts::install(&ctx);

    let typo = Typo::default();
    let mut input = egui::RawInput::default();
    input.screen_rect = Some(egui::Rect::from_min_size(
        egui::pos2(0.0, 0.0),
        egui::vec2(1280.0, 900.0),
    ));

    let cases = corpus();
    for align in [TextAlign::Left, TextAlign::Center, TextAlign::Justify] {
        for (name, src) in &cases {
            let doc = parse(Arc::from(src.as_str()));
            let blocks: Vec<_> = doc.blocks.iter().collect();
            let mut measured = 0usize;

            let env = Env {
                pal: &themes::DARK,
                typo: &typo,
                lang: Lang::Ru,
                text_align: align,
                fit_width: false,
                table_top: true,
                table_scroll: true,
                code_line_numbers: true,
                code_highlight: false,
            };
            let col_w = typo.width;

            let mut out = ctx.run_ui(input.clone(), |ui| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| {
                        for b in &blocks {
                            // Оценка (до измерения) конечна и не отрицательна.
                            let est = estimate(b, &typo, col_w, &doc.source);
                            assert!(est.is_finite() && est >= 0.0, "{name}: estimate {est}");
                            let (prep, h) = measure_block(ui, &env, col_w, b, &doc.source);
                            assert!(h.is_finite() && h > 0.0, "{name}: высота {h}");
                            let _ = prep;
                            measured += 1;
                        }
                    });
            });
            out.textures_delta.clear();

            assert_eq!(
                measured,
                doc.blocks.len(),
                "{name}: измерены все блоки ({align:?})"
            );
        }
    }
}
