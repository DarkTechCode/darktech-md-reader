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
cargo test             # тесты: parse_basic + perf_smoke
cargo test --release -- --ignored   # тяжёлый перф-тест 50 МБ (вручную)
cargo run -- temp/demo.md                 # запуск с демо-файлом
./target/x86_64-pc-windows-msvc/debug/darktech-md.exe temp/documentation.md  # exe
```

`.cargo/config.toml` уже настроен: target `x86_64-pc-windows-msvc`, `+crt-static`.
Поведение в окне проверяет владелец визуально: запускай приложение в фоне
(`run_in_background`), проси скриншот, правь по фидбеку. Коммит — после каждого этапа.

## 2. Что готово (этапы 0–1 плана + правки владельца)

**Этап 0 (каркас)** — парсер `pulldown-cmark` → плоские блоки (`src/md/`): заголовки,
абзацы, спаны (bold/italic/code/strike/links с URL, диапазоны в source), списки
(вложенность, task list, loose), цитаты и GFM-алерты, код (mermaid/html — с подписью),
таблицы, hr, картинка-абзац. Вкладки (CLI/drag&drop/Ctrl+O/W/Tab), welcome, экран ошибки.
Тёмная тема, RU/EN словарь, шрифты встроены (Noto Serif, Inter, JetBrains Mono — все
вариативные, bold координатой `wght`). Текст — selectable-Label, ссылки с hover/кликом,
широкий тёмный скроллбар, FAB «наверх» с прогрессом.

**Этап 1 (виртуализация)** — рендерится только видимое окно блоков ±600px:
- `src/layout/` — `LayoutState` (высоты: измеренные + оценки `символы/симв_в_строке`,
  префикс-суммы, бинарный поиск `visible_range`, компенсация «прилипания» якорем),
  `cache.rs` (готовые блоки, вытеснение по дистанции, cap 1536), `measure.rs`
  (JobSpec, оценки, измерение → `Prepared` с позициями всех декораций отрисовки).
- Код и таблицы виртуализованы изнутри: рисуются/измеряются только видимые строки;
  ширина кода — advance моно × max символов в строке; колонки таблицы — по шапке +
  64 строкам-образцу с потолком 66% ширины колонки (шире — перенос в ячейках).
- Парсинг >1 МБ — фоновым потоком чанками по 512 блоков (`parse_background`,
  `ParseMsg`), UI показывает документ по мере разбора; источник возвращается в `Done`.
  Замеры release: 50 МБ → полный разбор 188 мс (159 904 блока), память 265 МБ
  (бюджет < 400). Бюджеты §7 закрыты.
- Отрисовка разбита: `view/render/{text,code,table,misc}.rs`, по абсолютным
  координатам; каждый блок — scope со стабильным id («blk», tab_key, idx).
- Скролл: `show_viewport` + `set_min_height(total)`; viewport — координаты контента;
  принудительный оффсет (компенсация / «наверх») — `.vertical_scroll_offset()` только
  в нужных кадрах (каждый кадр перебил бы драг пользователя).
- id скролла/блоков привязаны к id вкладки — переключение вкладок не путает позиции.

**Правки по фидбеку владельца (documentation.md):** выравнивание по левому краю и
хит-тест ссылок — в Label кладётся ГОТОВЫЙ `Arc<Galley>` (не job — см. шпаргалку);
таблицы GitHub/Discord-стиля (перенос в ячейках) + внешние отступы; код/таблицы
резервируют 22px под горизонтальный скроллбар; вертикальные отступы всех блоков
увеличены (абзац 0.6em, код 0.85em, таблица 0.95/0.7em, правила в measure.rs);
таб-бар 18px/40px, имя файла с `.md`; «Копировать» → «Скопировано!» на 3 с.

Отклонения от плана (осознанные): шрифт документа по умолчанию Sans (Inter), не Serif
(просьба владельца); выделение и ссылки сделаны в этапе 0; высота строки таблицы =
max по ячейкам (в этапе 0 была сумма — исправлено); выделение текста теряется при
скролле за пределы видимого окна (следствие виртуализации) — доработка на этапе 4.

## 3. Карта кода

```
src/main.rs            запуск, CLI-пути, NativeOptions
src/app.rs             App/Tab (TabState: Welcome/Error/Loading/Ready), таб-бар, хоткеи,
                       drag&drop, open_link, pump_tabs (стриминг), заголовок окна
src/clipboard.rs       запись в клипборд ИЗ ФОНА (синхронная может блокировать навсегда)
src/fonts.rs           регистрация шрифтов (Inter→Proportional/Sans, NotoSerif→Serif, JBMono→Monospace/Mono)
src/i18n.rs            RU/EN, t(lang, key); system_lang()
src/md/blocks.rs       модель: Doc/BlockKind/Span/Fmt/Alert/ListMarker/TableRow; Doc::empty()
src/md/parser.rs       pulldown-cmark → блоки; parse() (синх.) + parse_background() (стрим)
src/layout/mod.rs      LayoutState: высоты, offsets, visible_range, компенсация, LayoutKey
src/layout/cache.rs    кэш Prepared-блоков (вытеснение по дистанции от текущего блока)
src/layout/measure.rs  JobSpec→LayoutJob, estimate(), measure_block() → Prepared; отступы блоков
src/view/mod.rs        Env { pal, typo, lang }
src/view/themes.rs     Palette (DARK/LIGHT), Typo, apply() (visuals+scrollbar), lighten()
src/view/doc_view.rs   виртуальный скролл (show_viewport) + колонка + FAB «наверх» + DocEvent
src/view/render/…      отрисовка Prepared: text (put_text/ссылки), code, table, misc
src/view/widgets.rs    flat_text_button, flat_pill_button
src/view/welcome.rs    welcome-экран
tests/parse_basic.rs   5 тестов парсера
tests/perf_smoke.rs    бюджеты §7: первый чанк стрима < 1 c, оценки, префикс-суммы/якорь;
                       #[ignore] полный разбор 50 МБ (`cargo test --release -- --ignored`)
temp/demo.md, temp/documentation.md   ручные файлы проверки (gitignored)
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
- ДЕДЛОК (у нас на нём зависало окно!): `ctx` = `parking_lot::RwLock` (не реентерабельный).
  Любой вложенный вызов ctx-метода ВНУТРИ `data_mut`/`input`/`write` (например
  `ctx.data_mut(|d| … ctx.time() …)` — time() берёт тот же write-lock) = само-дедлок.
  Вычислять всё ДО замыкания. В debug-сборке egui может поймать это panic'ом, в release — молча висит.
- Label: `ui.put(rect, Label::new(Arc<LayoutJob>))` ПЕРЕКЛАДЫВАЕТ job (put создаёт
  ско́п `centered_and_justified` → halign=Center, justify) → текст по центру и хит-тест
  ссылок разъезжается. Правильно: `painter.layout_job(job)` и `Label::new(Arc<Galley>)` —
  используется как есть, координаты galley == координаты отрисовки.
- Клипборд: `SetClipboardData` broadcast'ит SendMessage всем окнам системы; если какое-то
  не отвечает — синхронный вызов блокируется НАВСЕГДА (eframe пишет в конце кадра →
  зависает окно). Писать только из фонового потока (`src/clipboard.rs`, arboard напрямую).
- Виртуальный скролл: viewport из `show_viewport` — в координатах контента (min.y = offset);
  экранная позиция контента = `ui.cursor().min` в начале замыкания. Принудительный оффсет —
  `.vertical_scroll_offset(v)` только в кадрах, где нужен (state читается
  `scroll_area::State::load(ctx, id)`), иначе перебьёт драг. Состояние скролла у
  ScrollArea — персистентное по id (у нас id_salt = ("doc_scroll", tab_key)).
- Галка: не забыть `eframe = { default-features = false, features = ["glow","clipboard","links","default_fonts"] }`.
- Статический CRT: rustflags живут в `.cargo/config.toml` в секции **[build]** (`rustflags = ["-C","target-feature=+crt-static"]`).
  Секция `[rust]` НЕ существует — cargo молча игнорирует, exe требует VCRUNTIME140.dll (проверять:
  `grep -ao "VCRUNTIME[0-9_]*\.dll" exe` — пусто = ок). Релизный exe складываем в `bin/DarkTechMD.exe` (вне git).
- Release-сборка без окна консоли: `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]` в main.rs.

## 5. Следующий этап — 2. Типографика и настройки

Все опции из PLAN §4.9: тема (Светлая/Тёмная/Системная), шрифт (Serif/Sans/Mono),
размер 14–24, межстрочный 1.40–2.00, ширина колонки 640/820/1080 (+произвольная),
межбуквенный 0–2, размер UI-шрифта, панель настроек (строки «подпись + плашки/слайдер»),
persistence `%APPDATA%\DarkTechMD\settings.json` (serde, camelCase, повреждённый →
дефолты молча), «Сбросить настройки». Смена Typo инвалидирует кэш раскладки
(`LayoutKey` в `src/layout/`), файл НЕ перечитывается. Горячие клавиши Ctrl+=/-/0/1..3.
Приёмка: чек-лист опций 1 в 1 к MarkMello.

Дальше по PLAN.md §8: 3 (оглавление, миникарта-скелет, якоря #, статус-бар слова/мин) →
4 (добить: контекст-меню, Ctrl+A, правый клик на ссылке, выделение при виртуальном скролле) →
5 (поиск Ctrl+F, инкрементальный, memchr/aho-corasick в фоне) → 6 (сессия вкладок+позиций,
watcher notify c debounce 300мс, баннер «файл недоступен», последние файлы на welcome) →
7 (иконка winres, метаданные, статический CRT уже есть, perf-тесты, README;
заодно оптимизация памяти: спаны заимствуют текст из source вместо дублей строк).

## 6. Известные недоделки/хвосты

- Ссылки не кликаются в ячейках таблиц (put_text применяется к абзацам/цитатам/спискам/заголовкам).
- Выделение текста теряется при скролле за пределы окна (виртуализация); Ctrl+A — этап 4.
- Контекст-меню (правый клик) — этап 4. Якоря `#heading` — заглушка (этап 3).
- Ширины колонок таблиц считаются по шапке + первым 64 строкам: у очень длинных таблиц
  глубокие строки шире образца не расширяют колонки (переносятся) — принято.
- Память: текст хранится дважды (source + строки спанов ≈ 2× файла) + база egui ~75 МБ
  (атлас глифов, шрифты, GL-контекст). Оптимизация — этап 7.
- Локальные картинки — плейсхолдер (img_store: decode в фоне, LRU — этап 2–3).
- Светлая тема задана (LIGHT), но владельцем не проверялась.
- FAB «наверх» использует константы #222226/#6d6d6c (просьба владельца), не из Palette.
- Заголовки окна/таб-бар — минималистичные, вид владелец утвердил; не переусердствовать
  со «стилем egui» — везде плоско, без скруглений.

## 7. Рабочий процесс в чате с агентом

1. Прочитать этот файл и PLAN.md, `git log --oneline` для контекста.
2. Сделать этап целиком: код → `cargo build` (0 warnings) → `cargo test` →
   запуск exe для владельца (`run_in_background`, `temp/documentation.md` и `temp/demo.md`)
   → правки по фидбеку владельца (скриншоты делает владелец).
3. **Завершение этапа (обязательно, каждый раз):**
   - коммит с русским сообщением («Этап N: …»);
   - обновить этот файл (разделы «Что готово», «Карта кода», «Следующий этап», хвосты);
   - отметить этап ✅ в PLAN.md §8 (краткая сводка сделанного);
   - удалить из `temp/` одноразовые файлы (генерённые большие .md, логи, диагностические
     скрипты); оставлять только ручные файлы проверки (demo.md, documentation.md);
   - обновить `bin/DarkTechMD.exe` копией свежей release-сборки;
   - дать владельцу промпт для нового чата (контекст старого к этому моменту перегружен) —
     шаблон ниже, подставить номер и название следующего этапа.
4. Разделы статуса должны всегда отражать реальность.

### Шаблон промпта для нового чата

```
Продолжи разработку DarkTechMD (D:\WebWork\darktech.ru\scripts\darktech-md-reader).
Сначала целиком прочитай progress.md и PLAN.md в корне проекта, затем git log --oneline.
Выполни следующий этап по progress.md (сейчас это этап N — <название>).
После кода: cargo build (0 предупреждений), cargo test, запуск exe
(temp/documentation.md и temp/demo.md) для визуальной проверки — скриншоты делаю я сам
по твоей просьбе. Коммит после этапа и регламент завершения — по §7 progress.md.
```
