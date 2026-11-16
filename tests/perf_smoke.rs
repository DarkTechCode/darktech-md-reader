//! Бюджеты производительности (PLAN §7): стриминговый парсинг больших файлов
//! и логика виртуализации (оценки/префикс-суммы/видимый диапазон).
//! Тяжёлый тест 50 МБ помечен #[ignore] — запускать:
//! `cargo test --release --test perf_smoke -- --ignored`.

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Instant;

use darktech_md::layout::{measure, LayoutKey, LayoutState};
use darktech_md::md::parser::{parse, parse_background, ParseMsg, CHUNK_BLOCKS};
use darktech_md::view::themes::Typo;

/// Генератор «lorem markdown»: абзацы, заголовки, списки, таблицы, код.
fn gen_md(target_bytes: usize) -> String {
    const WORDS: &[&str] = &[
        "lorem", "ipsum", "dolor", "sit", "amet", "consectetur", "adipiscing", "elit",
        "sed", "do", "eiusmod", "tempor", "incididunt", "ut", "labore", "et", "dolore",
        "magna", "aliqua", "enim", "ad", "minim", "veniam", "quis", "nostrud",
    ];
    let mut out = String::with_capacity(target_bytes + 4096);
    let mut para = 0usize;
    while out.len() < target_bytes {
        if para % 40 == 0 {
            out.push_str(&format!("## Раздел {para}\n\n"));
        }
        if para % 97 == 96 {
            out.push_str(&format!(
                "```rust\nfn step_{para}() {{\n    let x = {para} + 1;\n    println!(\"{{x}}\");\n}}\n```\n\n"
            ));
        }
        if para % 53 == 52 {
            out.push_str("| col_a | col_b | col_c |\n|---|---:|:---:|\n");
            for i in 0..8 {
                out.push_str(&format!("| ячейка {i} | {} | метка {i} |\n", WORDS[i % WORDS.len()]));
            }
            out.push('\n');
        }
        let words: Vec<&str> = (0..60)
            .map(|i| WORDS[(para * 7 + i * 13) % WORDS.len()])
            .collect();
        out.push_str(&words.join(" "));
        out.push_str(" с русскими словами тоже.\n\n");
        if para % 7 == 6 {
            out.push_str("- пункт списка раз\n- пункт списка два\n  - вложенный пункт\n");
        }
        para += 1;
    }
    out
}

fn temp_file(name: &str, target_mb: usize) -> PathBuf {
    let dir = std::env::current_dir().unwrap().join("temp");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, gen_md(target_mb * 1024 * 1024)).unwrap();
    path
}

/// §7: 50 МБ → первый экран < 1 с. Здесь — первый чанк стрима 20 МБ < 1 c
/// (в debug тоже должен проходить с запасом: чтение + 512 блоков).
#[test]
fn streaming_first_chunk_under_1s() {
    let path = temp_file("perf20.md", 20);
    let (tx, rx) = mpsc::channel();
    let t0 = Instant::now();
    parse_background(path.clone(), tx);
    let first = rx.recv().expect("первый чанк");
    let dt = t0.elapsed();
    assert!(
        matches!(first, ParseMsg::Chunk { .. }),
        "первым сообщением должен быть чанк блоков"
    );
    assert!(dt.as_secs_f32() < 1.0, "первый чанк занял {dt:?}");

    // Дождаться конца и проверить целостность: все чанки, кроме последнего,
    // полные (512); последний — хвост любого размера.
    let mut blocks = 0;
    let mut prev_len: Option<usize> = None;
    while let Ok(msg) = rx.recv() {
        match msg {
            ParseMsg::Chunk { blocks: b, .. } => {
                if let Some(prev) = prev_len {
                    assert_eq!(prev, CHUNK_BLOCKS, "промежуточный чанк неполный");
                }
                blocks += b.len();
                prev_len = Some(b.len());
            }
            ParseMsg::Done {
                word_count,
                char_count,
                line_count,
                source,
            } => {
                assert!(word_count > 1_000_000, "слова посчитаны: {word_count}");
                // Байтов больше, чем символов (кириллица — 2 байта): 20 МБ
                // текста — заведомо больше 10 М символов.
                assert!(char_count > 10_000_000, "символы посчитаны");
                assert!(line_count > 10_000, "строки посчитаны");
                assert!(source.len() > 20 * 1024 * 1024);
                assert!(blocks > 10_000, "блоков: {blocks}");
                assert!(prev_len.is_some(), "хотя бы один чанк");
                return;
            }
            ParseMsg::Failed(e) => panic!("фоновой разбор упал: {e}"),
        }
    }
    panic!("канал закрылся без Done");
}

/// Оценки высот адекватны: положительные и ограниченные для обычных блоков.
#[test]
fn estimates_are_sane() {
    let src = gen_md(64 * 1024);
    let doc = parse(src.as_str().into());
    assert!(!doc.blocks.is_empty());
    let typo = Typo::default();
    for b in &doc.blocks {
        let h = measure::estimate(b, &typo, 820.0);
        assert!(h.is_finite() && h > 0.0 && h < 100_000.0, "высота {h} у {b:?}");
    }
}

/// Префикс-суммы и видимый диапазон: монотонность, полнота окна, стабильность
/// якоря при измерении (компенсация «прилипания»).
#[test]
fn layout_state_visible_range_and_anchor() {
    let src = gen_md(256 * 1024);
    let doc = parse(src.as_str().into());
    let typo = Typo::default();
    let key = LayoutKey {
        typo,
        width: 820.0,
        dark: true,
        lang: darktech_md::i18n::Lang::Ru,
        code_nums: false,
    };
    let mut layout = LayoutState::default();
    layout.ensure(&doc, key);
    let n = doc.blocks.len();
    assert!(n > 100, "блоков: {n}");

    let total = layout.total_height();
    assert!(total > 1000.0);

    // Всё в диапазоне.
    let all = layout.visible_range(0.0, total + 100.0);
    assert_eq!(all, 0..n);

    // Окно в середине: непустое и внутри границ.
    let mid = total * 0.5;
    let win = layout.visible_range(mid, mid + 600.0);
    assert!(!win.is_empty());
    assert!(win.start > 0 && win.end < n);

    // Окно за пределами снизу — пустое.
    assert!(layout.visible_range(total + 10.0, total + 100.0).is_empty());

    // Измерение блока в окне меняет высоту → dirty → finish_frame
    // пересобирает суммы и не двигает якорь (блоки выше не трогали).
    let i = win.start;
    let off_before = layout.offset(i);
    let measured = measure::estimate(&doc.blocks[i], &typo, 820.0) * 2.0;
    layout.set_measured(i, measured);
    let changed = layout.finish_frame(i);
    assert!(changed, "пересборка сумм требуется");
    assert_eq!(layout.height(i), measured);
    // Якорь записан, компенсации нет (высоты выше вьюпорта не менялись).
    assert!(layout.take_pending_scroll().is_none());
    let _ = off_before;

    // Теперь сдвинем высоту блока ВЫШЕ якоря — компенсация должна появиться.
    layout.set_measured(0, layout.height(0) + 100.0);
    layout.finish_frame(i);
    let delta = layout.take_pending_scroll().expect("компенсация прилипания");
    assert!((delta - 100.0).abs() < 0.5, "дельта {delta}");
}

/// Тяжёлый: полный разбор 50 МБ (release). Запуск: --release -- --ignored.
#[test]
#[ignore = "тяжёлый тест: cargo test --release -- --ignored"]
fn parse_50mb_full() {
    let path = temp_file("perf50.md", 50);
    let (tx, rx) = mpsc::channel();
    let t0 = Instant::now();
    parse_background(path, tx);
    let mut blocks = 0usize;
    while let Ok(msg) = rx.recv() {
        match msg {
            ParseMsg::Chunk { blocks: b, .. } => blocks += b.len(),
            ParseMsg::Done { .. } => break,
            ParseMsg::Failed(e) => panic!("{e}"),
        }
    }
    let dt = t0.elapsed();
    eprintln!("50 МБ: {blocks} блоков за {dt:?}");
    assert!(dt.as_secs_f32() < 10.0, "разбор 50 МБ занял {dt:?}");
}
