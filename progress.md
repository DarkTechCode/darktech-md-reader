# progress.md — инструкция для ИИ-агента, продолжающего работу

> Документ-передача. Прочитай его целиком, затем PLAN.md (задание), затем код.
> Актуально на: 2026-11-16. Владелец: Dark Wizard, https://darktech.ru.

## 1. Проект

DarkTechMD — быстрая читалка Markdown (замена MarkMello), Rust + egui.
Папка: `D:\WebWork\darktech.ru\scripts\darktech-md-reader` (свой локальный git, ветка `main`,
потом зальём на GitHub). Правила проекта — в `AGENTS.md` уровнем выше (`D:\WebWork\darktech.ru\AGENTS.md`):
один инструмент — одна папка; модуль желательно ≤ 400 строк; DRY; без обратной совместимости;
новые файлы UTF-8; временные файлы только в `temp/` (gitignored).

Команды:

```
cargo build            # сборка (debug)
cargo test             # тесты парсера (tests/parse_basic.rs)
cargo run -- temp/demo.md                 # запуск с демо-файлом
./target/x86_64-pc-windows-msvc/debug/darktech-md.exe temp/demo.md   # exe
```

`.cargo/config.toml` уже настроен: target `x86_64-pc-windows-msvc`, `+crt-static`.
Поведение в окне проверяет владелец визуально: запускай приложение в фоне
(`run_in_background`), проси скриншот, правь по фидбеку. Коммит — после каждого этапа.

## 2. Что готово (этап 0 плана + правки владельца)

- Парсер `pulldown-cmark` → плоские блоки (`src/md/`): заголовки, абзацы, спаны
  (bold/italic/code/strike/links с URL), списки (вложенность, task list, loose), цитаты
  и GFM-алерты, код (mermaid/html — с подписью), таблицы, hr, картинка-абзац.
  Спаны хранят диапазоны в source (`into_offset_iter`).
- Рендер блоков (`src/view/render_block.rs`): текст — через selectable-Label egui
  (выделение мышью, двойной клик — слово, Ctrl+C — копирование), остальное — painter.
- Ссылки: hover (цвет+подчёркивание+курсор+подсказка с URL), клик: веб — системный
  браузер (`ctx.open_url`), локальный `.md` — новая вкладка. `DocEvent` из doc_view → app.
- Таб-бар: плоский (без скруглений), текст 24px, активная вкладка — заливка фона читалки,
  hover — подсветка + курсор-рука. `+` открывает диалог (rfd).
- Вкладки: CLI-аргументы, drag&drop .md, Ctrl+O/W/Tab, welcome-экран, экран ошибки чтения.
- Скроллбар: сплошной, 20px, всегда видимый, тёмный (трек #222226, ручка чуть светлее),
  без скруглений.
- Кнопка «наверх»: круглая, справа снизу (с отступом от скроллбара), фон #222226,
  шеврон #6d6d6c, hover светлее; ярко-синяя обводка = % прокрутки; клик — в самое начало.
- Блок кода: шапка внутри блока (язык слева, кнопка «Копировать» справа в рамке,
  hover-подсветка), фон на всю ширину колонки, внешние отступы.
- Шрифты (встроены, вариативные, кириллица, OFL в `assets/fonts/`): Noto Serif, Inter,
  JetBrains Mono. Bold — координатой `wght` (400/700), курсив — синтез egui.
- Тёмная тема (`themes.rs`), RU/EN словарь (`i18n.rs`), все строки UI — через `t()`.

Отклонения от плана (осознанные): шрифт документа по умолчанию Sans (Inter), не Serif
(просьба владельца); выделение и ссылки сделаны уже в этапе 0 (часть этапа 4) —
потому что egui даёт их почти бесплатно через Label.

## 3. Карта кода

```
src/main.rs            запуск, CLI-пути, NativeOptions
src/app.rs             App/Tab, таб-бар, хоткеи, drag&drop, open_link, заголовок окна
src/fonts.rs           регистрация шрифтов (Inter→Proportional/Sans, NotoSerif→Serif, JBMono→Monospace/Mono)
src/i18n.rs            RU/EN, t(lang, key); system_lang()
src/md/blocks.rs       модель: Doc/BlockKind/Span/Fmt/Alert/ListMarker/TableRow
src/md/parser.rs       pulldown-cmark → блоки (flush_before_block при вложенности!)
src/view/mod.rs        Env { pal, typo, lang }
src/view/themes.rs     Palette (DARK/LIGHT), Typo, apply() (visuals+scrollbar), lighten()
src/view/doc_view.rs   ScrollArea (show_viewport) + колонка + FAB «наверх» + DocEvent
src/view/render_block.rs  отрисовка блоков, JobSpec→LayoutJob, put_text (ссылки), код, таблицы
src/view/widgets.rs    flat_text_button, flat_pill_button
src/view/welcome.rs    welcome-экран
tests/parse_basic.rs   5 тестов парсера
temp/demo.md           ручной файл для проверки (gitignored)
assets/fonts/          *.ttf + OFL-*.txt
```

## 4. egui 0.37 — шпаргалка (текстовый стек переписан, много отличий!)

- `eframe::App` — метод `fn ui(&mut self, ui: &mut egui::Ui, frame: &mut Frame)` (не `update`).
- Панели: `egui::Panel::top("id").show(ui, …)`, `CentralPanel::default().show(ui, …)`
  (не `show_inside`, `TopBottomPanel` больше нет).
- ТЕМЫ: egui держит стиль ПО ТЕМЕ (Dark/Light) и выбирает по ThemePreference (по умолчанию
  System!). `ctx.set_visuals` пишет в ТЕКУЩУЮ тему — если системная светлая, стиль уйдёт в
  Dark, а рендер — Light-дефолт. Лечится `ctx.set_theme(ThemePreference::Dark)` при старте
  (app.rs) + страховка в начале кадра: если `ui.visuals().panel_fill != наша` — пере-применить
  `themes::apply`. Цвета скроллбара: трек = `visuals.extreme_bg_color`, ручка =
  `visuals.widgets.{inactive,hovered}.bg_fill`, углы = `widgets.*.corner_radius`.
- `painter.rect(...)` требует 5-й аргумент `StrokeKind::Middle`; `rect_filled` — 4 аргумента.
- `FontId { size, family }` — веса нет; bold = `TextFormat.coords = VariationCoords::new([(b"wght", 700.0)])`
  (пути: `egui::epaint::text::VariationCoords`); курсив — `TextFormat.italics = true` (синтез).
- `TextFormat.extra_letter_spacing`, `TextFormat.line_height: Option<f32>` — есть.
- `LayoutSection.byte_range` — `ByteIndex(..)` (не usize).
- Галереи: `painter.layout_job(job) -> Arc<Galley>`; хит-тест: `galley.pos_from_cursor(CCursor::new(char_idx))`.
- Скроллбар: `style.spacing.scroll = ScrollStyle { floating: false, bar_width: 20.0, … }`,
  цвета: трек = `visuals.extreme_bg_color`, ручка = `visuals.widgets.{inactive,hovered}.bg_fill`,
  углы = `widgets.*.corner_radius`. `ctx.all_styles_mut(...)` (не `set_style`).
- Программный скролл: внутри контента `ui.scroll_to_rect(rect, Some(Align::Min))`
  (None = минимальный сдвиг, НЕ «наверх»!).
- Drag&drop: `ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path()))`.
- `ctx.set_cursor_icon(...)` — ставить ПОСЛЕ отрисовки Label (иначе перебьётся).
- `Color32::lerp_to_white` НЕТ — есть `themes::lighten()`.
- `Id::new`/`Ui::id()` deprecated: `ui.scope_id().with(...)`, `.id_salt(...)` у ScrollArea.
- Временное состояние между кадрами: `ctx.data_mut(|d| d.get_temp/insert_temp(id, v))`.
- Галка: не забыть `eframe = { default-features = false, features = ["glow","clipboard","links","default_fonts"] }`.

## 5. Следующий этап — 1. Виртуализация (самый ответственный)

Цель (план §7): 10 МБ без фризов; 50 МБ → первый экран < 1 c; память < 400 МБ.

Дизайн:
- Блоки не знают про экран. В `doc_view` вместо цикла по всем блокам — рендер только
  видимого окна ± overscan (~600px). Высоты: измеренные (из galley) + оценки
  (символов/символов_в_строке × высота строки) для неизмеренных; префикс-суммы высот,
  бинарный поиск видимого диапазона; позиция скроллбара уточняется по мере измерения
  (компенсация «прилипания»: при изменении суммы высот выше вьюпорта корректировать offset).
- Кэш galley: ключ (block_idx, ширина колонки, font/size/line_height/letter_spacing, theme);
  LRU (сотни–пара тысяч), инвалидация при смене Typo — файл не перечитывать.
- Парсинг больших файлов — в фоновом потоке (std::thread + channel; по готовности блоков
  — `ctx.request_repaint`); UI показывает первые блоки не дожидаясь конца (стриминг чанками).
- Таблицы: сейчас один блок рендерит ВСЕ строки — при виртуализации резать на видимые
  строки внутри блока (высоты строк из кэша).
- Проверка: `tests/perf_smoke.rs` по бюджетам §7; генератор больших файлов в `temp/` (gitignored).
- Заодно: `render_block.rs` ~700 строк — при переделке разбить (текст/код/таблицы/прочее).

Дальше по PLAN.md §8: 2 (панель настроек §4.9, persistence %APPDATA%\DarkTechMD\settings.json,
serde camelCase) → 3 (оглавление, миникарта-скелет, якоря #, статус-бар слова/мин) →
4 (добить: контекст-меню, Ctrl+A, правый клик на ссылке) → 5 (поиск Ctrl+F, инкрементальный,
memchr/aho-corasick в фоне) → 6 (сессия вкладок+позиций, watcher notify c debounce 300мс,
баннер «файл недоступен», последние файлы на welcome) → 7 (иконка winres, метаданные,
статический CRT уже есть, perf-тесты, README).

## 6. Известные недоделки/хвосты

- Ссылки не кликаются в ячейках таблиц (put_text применяется к абзацам/цитатам/спискам/заголовкам).
- Ctrl+A (выделить всё) и контекст-меню (правый клик) — этап 4.
- Якоря `#heading` — заглушка (этап 3).
- Локальные картинки — плейсхолдер (img_store: decode в фоне, LRU — этап по плану 2–3).
- Светлая тема задана (LIGHT), но владельцем не проверялась.
- FAB «наверх» использует константы #222226/#6d6d6c (просьба владельца), не из Palette.
- Заголовки окна/таб-бар — минималистичные, вид владелец утвердил; не переусердствовать
  со «стилем egui» — везде плоско, без скруглений.

## 7. Рабочий процесс в чате с агентом

1. Прочитать этот файл и PLAN.md, `git log --oneline` для контекста.
2. Сделать этап целиком: код → `cargo build` (0 warnings) → `cargo test` →
   запуск exe для владельца (`run_in_background` с `temp/demo.md`) → правки по фидбеку.
3. Коммит с русским сообщением («Этап N: …»).
4. Обновить этот раздел статуса (что готово / что дальше) — он должен всегда отражать реальность.
