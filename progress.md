# progress.md — инструкция для ИИ-агента, продолжающего работу

> Документ-передача. Прочитай его целиком, затем PLAN.md (задание), затем код.
> Актуально на: 2026-11-17. Владелец: Dark Wizard, https://darktech.ru.

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

**Этап 2 (типографика и настройки + сверх плана по фидбеку владельца):**
- `src/settings/` — serde-модель (camelCase, `#[serde(default)]`, повреждённый →
  дефолты молча) + store (`%APPDATA%\DarkTechMD\settings.json`, атомарно tmp+rename,
  запись с дебаунсом 400 мс). Опции: тема Светлая/Тёмная/Авто, язык Ru/En/Авто,
  шрифт Serif/Sans/Mono, размер 14–24, межстрочный 1.40–2.00, ширина 640/820/1080
  + слайдер 640–1280, межбуквенный 0–2, UI-шрифт 11–18, ячейки таблиц по верху,
  нумерация строк кода, подсветка кода, «по ширине окна» (fitWidth), сброс.
- Панель настроек `src/view/settings_panel.rs`: строки «подпись + плашки/слайдер»,
  свои плоские слайдеры/плашки (без egui-виджетов). Хоткеи Ctrl+=/-/0/1..3, Esc.
- Темы: `themes::apply` пишет ОБЕ палитры в ОБА слота egui (`set_visuals_of`) и
  фиксирует `text_options` (см. шпаргалку §4 — иначе атлас сбрасывается).
  Заголовок окна ОС — `ViewportCommand::SetTheme` (эффективный, не SystemDefault).
- Статус-бар: символы/строки/объём (Doc.char_count/line_count из парсера) +
  тумблер «По ширине окна» (справа, 25px от угла ресайза) + статистика выделения.
- Автоскролл средней кнопкой (браузерный): якорь-индикатор с шевронами, скорость
  от дистанции (мёртвая зона 16px, до 2400 px/s), выкл: клик/колесо/Esc; оффсет
  через temp-data → `vertical_scroll_offset` следующим кадром.
- Код-блоки: цельная карточка на общем фоне, отступ сверху 1.15em, кнопки
  «Копировать» сверху и снизу, гуттер номеров (вправо, 20px до кода), подсветка
  syntect (fancy-regex, без onig) — только видимый срез, кэш по (блок, срез, тема),
  прогрев фоновым потоком; темы base16-ocean.dark / InspiredGitHub.
- Таб-бар: «Открыть»-вкладка слева, вкладки вплотную, крестик красный при hover,
  активная вкладка синим (pal.link), hover неактивной = pal.bg (затемнение),
  стрелки ‹ › у правого края, Ctrl+колесо над полосой = переключение вкладок.
- Без аргументов CLI открывается temp/demo.md (если найден — cwd/exe/../temp).
- Экраны welcome/error/loading масштабируются UI-шрифтом; welcome-кнопка плоская.
- Правки по фидбеку владельца: ~14 раундов (цвета плашек через
  `galley_with_override_text_color`, мгновенный репейнт при смене настроек,
  анти-«узор»/кракозябры — см. §4, поля fit-режима 20px, таблицы в fit без
  h-скролла с пропорциональным сжатием колонок, квантование col до целых пикселей).

Отклонения от плана (осознанные): шрифт документа по умолчанию Sans (Inter), не Serif
(просьба владельца); выделение и ссылки сделаны в этапе 0; высота строки таблицы =
max по ячейкам (в этапе 0 была сумма — исправлено); выделение текста теряется при
скролле за пределы видимого окна (следствие виртуализации) — доработка на этапе 4;
сверх плана добавлены fitWidth/нумерация/подсветка/автоскролл/статус-бар/кнопка
«Открыть» (просьбы владельца в ходе этапа).

## 3. Карта кода

```
src/main.rs            запуск, CLI-пути, NativeOptions
src/app.rs             App/Tab (TabState: Welcome/Error/Loading/Ready), таб-бар («Открыть»,
                       вкладки вплотную, ‹ ›, шестерёнка, Ctrl+колесо), хоткеи, drag&drop,
                       open_link, pump_tabs, статус-бар (статистика+выделение+fitWidth),
                       SelectionWatcher-плагин, find_demo_md, apply_theme/effective_dark
src/clipboard.rs       запись в клипборд ИЗ ФОНА (синхронная может блокировать навсегда)
src/fonts.rs           регистрация шрифтов (Inter→Proportional/Sans, NotoSerif→Serif, JBMono→Monospace/Mono)
src/highlight.rs       подсветка syntect: ленивые наборы, прогрев фоном, highlight()
src/i18n.rs            RU/EN, t(lang, key); system_lang()
src/md/blocks.rs       модель: Doc (word/char/line_count)/BlockKind/Span/Fmt/Alert/TableRow
src/md/parser.rs       pulldown-cmark → блоки; parse() + parse_background() (стрим);
                       count_stats() в Done
src/layout/mod.rs      LayoutState: высоты, offsets, visible_range, компенсация; LayoutKey
                       { typo, width, dark, lang, code_nums }
src/layout/cache.rs    кэш Prepared-блоков (вытеснение по дистанции от текущего блока)
src/layout/measure.rs  JobSpec→LayoutJob, estimate(), measure_block() → Prepared; отступы
                       блоков, гуттер номеров, CODE_FOOTER_H, fit-таблицы
src/settings/model.rs  SettingsData (serde camelCase, normalize-клампы), Typo из настроек
src/settings/store.rs  %APPDATA%\DarkTechMD\settings.json: load (битый→дефолты), save (tmp+rename)
src/view/mod.rs        Env { pal, typo, lang, fit_width, table_top, code_line_numbers, code_highlight }
src/view/themes.rs     Palette (DARK/LIGHT + fab_*), Typo, apply() (оба слота + text_options)
src/view/doc_view.rs   виртуальный скролл + колонка (fit = avail-40, floor) + FAB + автоскролл
src/view/render/…      отрисовка Prepared: text (put_text/ссылки), code (карточка, гуттер,
                       подсветка, 2×«Копировать»), table (top-align), misc
src/view/settings_panel.rs  панель настроек: строки «подпись + плашки/слайдер», сброс
src/view/widgets.rs    flat_text_button, plain_galley
src/view/welcome.rs    welcome-экран (масштаб UI-шрифта)
tests/parse_basic.rs   5 тестов парсера
tests/perf_smoke.rs    бюджеты §7 + LayoutKey-тест; #[ignore] 50 МБ
temp/demo.md, temp/documentation.md   ручные файлы проверки (gitignored; demo.md —
                       «все элементы», открывается по умолчанию без аргументов)
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
- ГЛАВНАЯ ГРАБЛЯ ЭТАПА 2 (кракозябры/пропадающий текст/фризы при смене темы): `Glyph`
  хранит `uv_rect` АТЛАСА, запечённый в galley при раскладке. У `Visuals::dark()` и
  `light()` РАЗНЫЕ `text_options.color_transfer_function` → смена темы меняет опции →
  egui делает `GlyphAtlas::new` (полный сброс атласа) → ВСЕ закэшированные нами
  `Arc<Galley>` (Prepared-кэш) остаются со старыми UV (сам egui чистит только свой кэш).
  ЛЕЧЕНИЕ: `themes::apply` берёт текущие `ctx.global_style().visuals.text_options`
  и пишет их в ОБА слота (`set_visuals_of(Dark/Light)`) — опции не меняются никогда.
- `Painter::galley(pos, g, color)` — цвет-аргумент это FALLBACK: игнорируется для
  галлеев с уже заданными цветами (jobs всегда цветные). Для перекраски —
  `galley_with_override_text_color` (плашки, кнопки, «Копировать»).
- Настройки применились «с задержкой»: тема/типографика применяются ТОЛЬКО в начале
  кадра, а репейнт после клика по плашке не планировался — ждать дебаунса сохранения
  (400 мс). Лечение: `ctx.request_repaint()` при изменении SettingsData.
- Заголовок окна Windows серый на старте: eframe не задаёт тему титульной строки,
  пока не придёт `ViewportCommand::SetTheme` (SystemDefault у winit = светлый).
  Шлём эффективную тему из apply_theme при каждом применении.
- `OutputCommand::TextSelectionSettled` шлётся встроенным плагином выделения
  В КОНЦЕ прохода ПОСЛЕ кода приложения — из App::ui не видно. Свой плагин
  (`ctx.add_plugin`, `egui::plugin::Plugin::on_end_pass`) добавляется после
  встроенных и видит команду. Живого текста выделения (во время драга) публично НЕТ.
- `Event::MouseWheel { delta, .. }` (не unit_delta); `Modifiers::ctrl_only()` НЕТ —
  проверять поля; `scroll_area::State::offset` приватный — программный скролл только
  через `.vertical_scroll_offset(abs)` следующим кадром (temp-data), как «наверх».
- `Id::new` deprecated → `Id::unique` (для глобальных) / `IdSalt::new`.

## 5. Следующий этап — 3. Навигация

Оглавление (слева, опционально: из h1–h6, клик — скролл к разделу, подсветка текущего
по позиции скролла; настройки сторона/макс. уровень — PLAN §4.6), миникарта-скелет
справа (полоски-блоки, O(высота миникарты), прямоугольник вьюпорта, клик/драг),
якоря `#heading` (сейчас заглушка в open_link; headings+block_idx уже в Doc),
статус-бар: слова и «чтение: N мин» (word_count уже есть, 220 сл/мин).
Опции миникарты/оглавления — в панель настроек (PLAN §4.9: Авто/Вкл/Выкл, сторона,
макс. уровень). Приёмка: навигация не мешает виртуализации (бюджеты §7).

Дальше по PLAN.md §8: 4 (добить: контекст-меню, Ctrl+A, правый клик на ссылке,
выделение при виртуальном скролле, живой счётчик выделения — см. хвосты) →
5 (поиск Ctrl+F, инкрементальный, memchr/aho-corasick в фоне) → 6 (сессия вкладок+позиций,
watcher notify c debounce 300мс, баннер «файл недоступен», последние файлы на welcome) →
7 (иконка winres, метаданные, perf-тесты, README; оптимизация памяти: спаны заимствуют
текст из source вместо дублей строк).

## 6. Известные недоделки/хвосты

- Статистика «выделено» в статус-баре обновляется только по факту ОТПУСКАНИЯ кнопки
  мыши (egui не отдаёт текст выделения во время драга) и иногда «через раз» —
  подозрение на гонку settle-команды с удалением состояния плагина выделения.
  Сделать собственный трекинг выделения в этапе 4 (заодно живой счётчик посимвольно).
- Ссылки не кликаются в ячейках таблиц (put_text применяется к абзацам/цитатам/спискам/заголовкам).
- Выделение текста теряется при скролле за пределы окна (виртуализация); Ctrl+A — этап 4.
- Контекст-меню (правый клик) — этап 4.
- Ширины колонок таблиц: по шапке + первым 64 строкам; в fit-режиме колонки сжимаются
  пропорционально (пол 24px) — очень широкие таблицы без пола могут быть тесными.
- Память: текст хранится дважды (source + строки спанов ≈ 2× файла) + база egui ~75 МБ.
  Оптимизация — этап 7.
- Локальные картинки — плейсхолдер (img_store: decode в фоне, LRU — этап 3+).
- Подсветка кода красит видимый срез с перезапуском состояния на границе среза
  (многострочные токены, начавшиеся выше, могут подсветиться неверно) — принято.
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
