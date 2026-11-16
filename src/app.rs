//! Состояние приложения: вкладки, ввод (CLI/drag-drop/Ctrl+O), панели.
//! Большие файлы грузятся фоном: UI показывает блоки по мере разбора.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use eframe::egui::{
    self, pos2, vec2, Context, FontFamily, Frame, Key, Modifiers, Rect, Sense, Ui,
    ViewportCommand,
};
use eframe::egui::epaint::text::VariationCoords;
use eframe::egui::text::{ByteIndex, Galley, LayoutJob, LayoutSection, TextFormat};

use crate::i18n::{t, Lang};
use crate::layout::LayoutState;
use crate::md::blocks::Doc;
use crate::md::parser::{self, ParseMsg};
use crate::settings::model::{self, SettingsData, ThemeMode, WIDTH_PRESETS};
use crate::settings::store;
use crate::view::{doc_view, settings_panel, themes, welcome, Env};
use crate::fonts;

/// Задержка записи settings.json после последнего изменения (дебаунс слайдеров).
const SAVE_DELAY: Duration = Duration::from_millis(400);

/// Статистика выделения для статус-бара: (символы, слова, строки).
pub type SelStats = (usize, usize, usize);

/// egui отправляет TextSelectionSettled в конце прохода ПОСЛЕ кода
/// приложения (это делает встроенный плагин выделения), поэтому единственный
/// способ его увидеть — свой плагин, добавленный после встроенных.
struct SelectionWatcher;

impl eframe::egui::plugin::Plugin for SelectionWatcher {
    fn debug_name(&self) -> &'static str {
        "DarkTechMD::SelectionWatcher"
    }

    fn on_end_pass(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let settled: Option<String> = {
            let mut found = None;
            ctx.output(|o| {
                for cmd in &o.commands {
                    if let egui::OutputCommand::TextSelectionSettled(text) = cmd {
                        found = Some(text.clone());
                    }
                }
            });
            found
        };
        if let Some(text) = settled {
            let stats: Option<SelStats> = if text.is_empty() {
                None
            } else {
                Some((
                    text.chars().count(),
                    text.split_whitespace().count(),
                    text.matches('\n').count() + 1,
                ))
            };
            ctx.data_mut(|d| d.insert_temp(egui::Id::unique("sel_stats"), stats));
        } else {
            // Живого текста выделения у egui публично нет (только по факту
            // отпускания кнопки), поэтому чистим агрессивно: любое новое
            // нажатие мыши или исчезновение выделения — статистика долой.
            let pressed_now = ui.input(|i| i.pointer.any_pressed() && !i.modifiers.shift);
            let has = ctx
                .plugin::<eframe::egui::text_selection::LabelSelectionState>()
                .lock()
                .has_selection();
            if !has || pressed_now {
                ctx.data_mut(|d| {
                    if d.get_temp::<Option<SelStats>>(egui::Id::unique("sel_stats")).is_some() {
                        d.insert_temp(egui::Id::unique("sel_stats"), Option::<SelStats>::None);
                    }
                });
            }
        }
    }
}

pub struct App {
    tabs: Vec<Tab>,
    active: usize,
    /// Настройки (settings.json): типографика документа, тема, язык, UI-шрифт.
    settings: SettingsData,
    /// Применённый режим темы (чтобы поймать смену System↔явной).
    applied_theme: ThemeMode,
    /// Применённая (эффективная) тёмная тема: выбор пользователя или системы.
    dark: bool,
    /// Панель настроек открыта.
    show_settings: bool,
    /// Счётчик id вкладок (стабильные id скроллов/блоков на переключение вкладок).
    next_tab_id: u64,
    /// Последний выставленный заголовок окна (чтобы не спамить командой).
    title_sent: String,
    /// Отложенная запись настроек (последнее изменение + SAVE_DELAY).
    save_after: Option<Instant>,
}

pub struct Tab {
    /// Стабильный id вкладки (id скролла и блоков привязаны к нему).
    pub id: u64,
    pub path: Option<PathBuf>,
    pub state: TabState,
    /// Раскладка документа этой вкладки (высоты, кэш galley).
    pub layout: LayoutState,
}

pub enum TabState {
    Welcome,
    Error(String),
    /// Фоновая загрузка: документ растёт чанками.
    Loading {
        doc: Doc,
        rx: Receiver<ParseMsg>,
    },
    Ready(Arc<Doc>),
}

impl Tab {
    fn welcome(id: u64) -> Self {
        Self {
            id,
            path: None,
            state: TabState::Welcome,
            layout: LayoutState::default(),
        }
    }

    /// Маленькие файлы — синхронно (мгновенно), большие — фоновым потоком.
    fn load(id: u64, path: PathBuf) -> Self {
        let size = path.metadata().map(|m| m.len()).unwrap_or(0);
        let state = if size < parser::SYNC_LIMIT {
            match parser::read_text(&path) {
                Ok(text) => TabState::Ready(Arc::new(parser::parse(Arc::from(text.as_str())))),
                Err(e) => TabState::Error(e),
            }
        } else {
            let (tx, rx) = std::sync::mpsc::channel();
            parser::parse_background(path.clone(), tx);
            TabState::Loading {
                doc: Doc::empty(),
                rx,
            }
        };
        Self {
            id,
            path: Some(path),
            state,
            layout: LayoutState::default(),
        }
    }

    fn title(&self, lang: Lang) -> String {
        match &self.path {
            Some(p) => p
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.to_string_lossy().into_owned()),
            None => t(lang, "welcome_title").to_string(),
        }
    }
}

/// Результат прокачки канала фоновой загрузки за кадр.
enum Pumped {
    Still,
    Done {
        word_count: usize,
        char_count: usize,
        line_count: usize,
        source: Arc<str>,
    },
    Failed(String),
}

fn pump_rx(rx: &Receiver<ParseMsg>, doc: &mut Doc) -> Pumped {
    let mut res = Pumped::Still;
    while let Ok(msg) = rx.try_recv() {
        match msg {
            ParseMsg::Chunk { blocks, headings } => {
                doc.blocks.extend(blocks);
                doc.headings.extend(headings);
            }
            ParseMsg::Done {
                word_count,
                char_count,
                line_count,
                source,
            } => res = Pumped::Done {
                word_count,
                char_count,
                line_count,
                source,
            },
            ParseMsg::Failed(e) => res = Pumped::Failed(e),
        }
    }
    res
}

impl App {
    pub fn new(cc: &eframe::CreationContext, paths: Vec<PathBuf>) -> Self {
        fonts::install(&cc.egui_ctx);
        let settings = store::load();
        // Эффективная тема: настройка или системная; egui знает её с старта.
        let dark = effective_dark(&cc.egui_ctx, settings.theme);
        apply_theme(&cc.egui_ctx, settings.theme, dark);

        // Прогрев подсветки синтаксиса в фоне (один раз, ~50 мс).
        crate::highlight::warm_up();
        // Статистика выделения в статус-бар (см. SelectionWatcher).
        cc.egui_ctx.add_plugin(SelectionWatcher);

        let mut next_id = 0;
        let mut paths = paths;
        // Без аргументов — открываем demo.md для проверки (если найден).
        if paths.is_empty() {
            if let Some(demo) = find_demo_md() {
                paths.push(demo);
            }
        }
        let mut tabs: Vec<Tab> = paths
            .into_iter()
            .map(|p| {
                let tab = Tab::load(next_id, p);
                next_id += 1;
                tab
            })
            .collect();
        if tabs.is_empty() {
            tabs.push(Tab::welcome(next_id));
        }

        Self {
            tabs,
            active: 0,
            applied_theme: settings.theme,
            settings,
            dark,
            show_settings: false,
            next_tab_id: next_id + 1,
            title_sent: String::new(),
            save_after: None,
        }
    }

    fn pal(&self) -> &'static themes::Palette {
        if self.dark {
            &themes::DARK
        } else {
            &themes::LIGHT
        }
    }

    fn lang(&self) -> Lang {
        self.settings.lang()
    }

    /// Масштаб UI-хрома (таб-бар, welcome, панель) от размера UI-шрифта.
    fn ui_scale(&self) -> f32 {
        self.settings.ui_font_size as f32 / model::UI_FONT_DEFAULT as f32
    }

    /// Прокачивает фоновые загрузки всех вкладок; true — пока есть живые.
    fn pump_tabs(&mut self, ctx: &Context) -> bool {
        let mut loading = false;
        for tab in &mut self.tabs {
            let mut finished: Option<TabState> = None;
            if let TabState::Loading { doc, rx } = &mut tab.state {
                match pump_rx(rx, doc) {
                    Pumped::Still => loading = true,
                    Pumped::Done {
                        word_count,
                        char_count,
                        line_count,
                        source,
                    } => {
                        doc.word_count = word_count;
                        doc.char_count = char_count;
                        doc.line_count = line_count;
                        doc.source = source;
                        let full = std::mem::replace(doc, Doc::empty());
                        finished = Some(TabState::Ready(Arc::new(full)));
                    }
                    Pumped::Failed(e) => finished = Some(TabState::Error(e)),
                }
            }
            if let Some(state) = finished {
                tab.state = state;
            }
        }
        if loading {
            ctx.request_repaint();
        }
        loading
    }

    /// Открывает файл новой вкладкой (или заменяет единственную welcome-вкладку).
    fn open(&mut self, path: PathBuf) {
        let replace_welcome = self.tabs.len() == 1 && self.tabs[0].path.is_none();
        let tab = Tab::load(self.next_tab_id, path);
        self.next_tab_id += 1;
        let has_content = !matches!(tab.state, TabState::Welcome);
        if replace_welcome {
            self.tabs[0] = tab;
            self.active = 0;
        } else if has_content {
            self.tabs.push(tab);
            self.active = self.tabs.len() - 1;
        }
    }

    fn close_tab(&mut self, idx: usize) {
        if idx < self.tabs.len() {
            self.tabs.remove(idx);
            if self.tabs.is_empty() {
                self.tabs.push(Tab::welcome(self.next_tab_id));
                self.next_tab_id += 1;
            }
            self.active = self.active.min(self.tabs.len() - 1);
        }
    }

    /// Клик по ссылке в документе: веб — в системный браузер, локальный .md —
    /// новой вкладкой (путь относительно текущего файла).
    fn open_link(&mut self, url: &str, ctx: &Context) {
        let lower = url.to_ascii_lowercase();
        if lower.starts_with("http://")
            || lower.starts_with("https://")
            || lower.starts_with("mailto:")
        {
            ctx.open_url(egui::OpenUrl::new_tab(url));
        } else if lower.ends_with(".md") || lower.ends_with(".markdown") {
            let base = self
                .tabs
                .get(self.active)
                .and_then(|t| t.path.as_ref())
                .and_then(|p| p.parent().map(|d| d.to_path_buf()));
            let path = match (std::path::Path::new(url).is_absolute(), base) {
                (true, _) => PathBuf::from(url),
                (false, Some(b)) => b.join(url),
                (false, None) => PathBuf::from(url),
            };
            self.open(path);
        } else if url.starts_with('#') {
            // Якорь внутри документа — появится на этапе навигации.
        } else {
            ctx.open_url(egui::OpenUrl::new_tab(url));
        }
    }

    fn handle_input(&mut self, ctx: &Context) {
        // Ctrl+O — диалог выбора файла.
        if ctx.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::O)) {
            if let Some(path) = pick_md_file() {
                self.open(path);
            }
        }

        // Ctrl+W — закрыть вкладку.
        if ctx.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::W)) {
            self.close_tab(self.active);
        }

        // Ctrl+Tab / Ctrl+Shift+Tab — переключение вкладок.
        if ctx.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::Tab)) {
            self.active = next_tab(self.active, self.tabs.len(), false);
        } else if ctx.input_mut(|i| {
            i.consume_key(Modifiers::CTRL | Modifiers::SHIFT, Key::Tab)
        }) {
            self.active = next_tab(self.active, self.tabs.len(), true);
        }

        // Esc — закрыть панель настроек.
        if self.show_settings
            && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape))
        {
            self.show_settings = false;
        }

        // Ctrl+= / Ctrl+Plus — крупнее, Ctrl+- — мельче, Ctrl+0 — обычный размер.
        if ctx.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::Equals))
            || ctx.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::Plus))
        {
            self.settings.font_size = (self.settings.font_size + 1).min(model::FONT_MAX);
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::Minus)) {
            self.settings.font_size =
                self.settings.font_size.saturating_sub(1).max(model::FONT_MIN);
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::Num0)) {
            self.settings.font_size = model::FONT_DEFAULT;
        }

        // Ctrl+1..3 — пресеты ширины текста (узкая/средняя/широкая).
        let preset_keys = [Key::Num1, Key::Num2, Key::Num3];
        for (key, &w) in preset_keys.iter().zip(WIDTH_PRESETS.iter()) {
            if ctx.input_mut(|i| i.consume_key(Modifiers::CTRL, *key)) {
                self.settings.text_width = w;
                self.settings.fit_width = false;
            }
        }

        // Drag&drop файлов.
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| Some(f.path().to_path_buf()))
                .filter(|p| {
                    matches!(
                        p.extension()
                            .and_then(|e| e.to_str())
                            .map(|e| e.to_ascii_lowercase())
                            .as_deref(),
                        Some("md" | "markdown" | "mdown")
                    )
                })
                .collect()
        });
        for path in dropped {
            self.open(path);
        }
    }

    fn sync_title(&mut self, ctx: &Context) {
        let title = match self.tabs.get(self.active) {
            Some(tab) if tab.path.is_some() => {
                format!("{} — DarkTechMD", tab.title(self.lang()))
            }
            _ => "DarkTechMD".to_string(),
        };
        if title != self.title_sent {
            self.title_sent = title.clone();
            ctx.send_viewport_cmd(ViewportCommand::Title(title));
        }
    }
}

fn next_tab(cur: usize, len: usize, back: bool) -> usize {
    if len == 0 {
        return 0;
    }
    if back {
        (cur + len - 1) % len
    } else {
        (cur + 1) % len
    }
}

/// Эффективная тёмная тема: System — по данным ОС (пока неизвестно → тёмная).
fn effective_dark(ctx: &Context, mode: ThemeMode) -> bool {
    match mode {
        ThemeMode::Dark => true,
        ThemeMode::Light => false,
        ThemeMode::System => ctx.system_theme() != Some(egui::Theme::Light),
    }
}

/// Применяет тему: preference egui + наши палитры в оба слота стилей +
/// тема заголовка окна ОС (иначе на старте заголовок системно-серый).
/// Фиксация preference важна (см. шпаргалку §4): иначе наш стиль уйдёт в один
/// слот, а рендериться будет дефолт другого.
fn apply_theme(ctx: &Context, mode: ThemeMode, dark: bool) {
    let pref = match mode {
        ThemeMode::Dark => egui::ThemePreference::Dark,
        ThemeMode::Light => egui::ThemePreference::Light,
        ThemeMode::System => egui::ThemePreference::System,
    };
    ctx.set_theme(pref);
    themes::apply(ctx);
    // Тема титульной строки Windows: шлём эффективную, а не SystemDefault —
    // «по системе» у winit оставляет светлый заголовок по умолчанию.
    ctx.send_viewport_cmd(ViewportCommand::SetTheme(if dark {
        egui::SystemTheme::Dark
    } else {
        egui::SystemTheme::Light
    }));
}

fn pick_md_file() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter("Markdown", &["md", "markdown", "mdown"])
        .pick_file()
}

/// demo.md «на все элементы» для проверки без аргументов CLI.
fn find_demo_md() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("temp").join("demo.md"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("temp").join("demo.md"));
            candidates.push(dir.join("..").join("temp").join("demo.md"));
            candidates.push(dir.join("..").join("..").join("temp").join("demo.md"));
        }
    }
    candidates.into_iter().find(|p| p.is_file())
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let before = self.settings.clone();

        // Тема: настройка или система (может переключиться во время работы).
        // Смену режима ловим отдельно от цвета: Светлая→Системная при светлой
        // ОС не меняет рендер, но должна вернуть слежение за системой.
        // Визуальный стиль egui может сбрасываться — тогда вернём наш ниже.
        let eff = effective_dark(&ctx, self.settings.theme);
        if self.settings.theme != self.applied_theme || eff != self.dark {
            self.applied_theme = self.settings.theme;
            self.dark = eff;
            apply_theme(&ctx, self.settings.theme, eff);
        } else if ui.visuals().panel_fill != self.pal().bg {
            themes::apply(&ctx);
        }

        self.handle_input(&ctx);
        self.pump_tabs(&ctx);

        egui::Panel::top("tab_strip")
            .resizable(false)
            .frame(Frame::NONE.fill(self.pal().code_bg))
            .show(ui, |ui| {
                tab_strip(ui, self);
            });

        if self.show_settings {
            let lang = self.lang();
            let pal = self.pal();
            let w = (430.0 * self.ui_scale()).clamp(360.0, 640.0);
            egui::Panel::right("settings_panel")
                .resizable(false)
                .exact_size(w)
                .frame(Frame::NONE.fill(pal.code_bg))
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("settings_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| settings_panel::show(ui, &mut self.settings, pal, lang));
                    // Плоская граница слева (сам рисуем — стили egui не используем).
                    let r = ui.min_rect();
                    ui.painter().line_segment(
                        [pos2(r.min.x, r.min.y), pos2(r.min.x, r.max.y)],
                        egui::Stroke::new(1.0, pal.border),
                    );
                });
        }

        // Статистика выделения (считается плагином в конце прохода).
        let sel_stats: Option<SelStats> = ctx
            .data(|d| d.get_temp(egui::Id::unique("sel_stats")))
            .flatten();

        // Статус-бар: статистика документа + тумблер «по ширине окна».
        let has_doc = matches!(
            self.tabs.get(self.active).map(|t| &t.state),
            Some(TabState::Ready(_)) | Some(TabState::Loading { .. })
        );
        if has_doc {
            let pal = self.pal();
            egui::Panel::bottom("status_bar")
                .resizable(false)
                .exact_size(26.0)
                .frame(Frame::NONE.fill(pal.code_bg))
                .show(ui, |ui| {
                    status_bar(ui, self, sel_stats);
                });
        }

        let pal = self.pal();
        let typo = self.settings.typo();
        let lang = self.lang();
        let ui_em = self.settings.ui_font_size as f32;
        let env = Env {
            pal,
            typo: &typo,
            lang,
            fit_width: self.settings.fit_width,
            table_top: self.settings.table_top,
            code_line_numbers: self.settings.code_line_numbers,
            code_highlight: self.settings.code_highlight,
        };

        let mut link_events: Vec<String> = Vec::new();
        let mut welcome_clicked = false;
        egui::CentralPanel::default()
            .frame(Frame::NONE.fill(pal.bg))
            .show(ui, |ui| {
                let Some(tab) = self.tabs.get_mut(self.active) else {
                    return;
                };
                let Tab {
                    id,
                    state,
                    layout,
                    path,
                    ..
                } = tab;
                match state {
                    TabState::Error(err) => {
                        show_error(ui, path, err, env.lang, ui_em);
                    }
                    TabState::Welcome => {
                        welcome_clicked = welcome::show(ui, env.lang, pal, ui_em);
                    }
                    TabState::Loading { doc, .. } => {
                        if doc.blocks.is_empty() {
                            show_loading(ui, env.lang, ui_em);
                        } else {
                            for ev in doc_view::show(ui, doc, layout, &env, *id) {
                                match ev {
                                    doc_view::DocEvent::OpenLink(url) => {
                                        link_events.push(url)
                                    }
                                }
                            }
                        }
                    }
                    TabState::Ready(arc) => {
                        for ev in doc_view::show(ui, arc, layout, &env, *id) {
                            match ev {
                                doc_view::DocEvent::OpenLink(url) => link_events.push(url),
                            }
                        }
                    }
                }
            });
        for url in link_events {
            self.open_link(&url, &ctx);
        }
        if welcome_clicked {
            if let Some(path) = pick_md_file() {
                self.open(path);
            }
        }

        // Настройки изменились → нормализуем и запишем в settings.json
        // (с дебаунсом, чтобы слайдер не жал в диск каждый кадр).
        // Репейнт сразу: тема/типографика применяются только в начале кадра —
        // без этого eframe ждал бы debounce-таймер (400 мс «задержки»).
        if self.settings != before {
            self.settings.normalize();
            self.save_after = Some(Instant::now() + SAVE_DELAY);
            ctx.request_repaint();
        }
        if let Some(at) = self.save_after {
            if Instant::now() >= at {
                self.save_after = None;
                if let Err(e) = store::save(&self.settings) {
                    log::warn!("settings.json: {e}");
                }
            } else {
                ctx.request_repaint_after(at - Instant::now());
            }
        }

        self.sync_title(&ctx);
    }
}

/// Действия из таб-бара (применяются после отрисовки, чтобы не мучиться с заимствованиями).
enum TabAction {
    Select(usize),
    Close(usize),
    NewTab,
    ToggleSettings,
}

/// Плоские вкладки без скруглений: активная подсвечена заливкой фона читалки.
/// Текст — размера ссылок в документе, имя файла с расширением.
fn tab_strip(ui: &mut Ui, app: &mut App) {
    let pal = *app.pal();
    let scale = app.ui_scale();
    let h = 40.0;
    let text_size = 16.0 * scale;
    let mut actions: Vec<TabAction> = Vec::new();

    ui.horizontal(|ui| {
        // Вкладки вплотную (плашки цельные, зазоры не нужны).
        ui.spacing_mut().item_spacing.x = 0.0;

        // «Открыть» — как ещё одна (неактивная) вкладка, всегда первая:
        // видно и при большом числе вкладок.
        {
            let g = title_galley(ui, t(app.lang(), "st_open"), text_size, pal.muted, false);
            let w = g.size().x + 12.0 * 2.0;
            let (rect, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click());
            let painter = ui.painter();
            let hovered = resp.hovered();
            if hovered {
                painter.rect_filled(rect, egui::CornerRadius::same(0), pal.bg);
            }
            painter.galley_with_override_text_color(
                pos2(rect.min.x + 12.0, rect.center().y - g.size().y * 0.5),
                g,
                if hovered { pal.fg } else { pal.muted },
            );
            if hovered {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.clicked() {
                actions.push(TabAction::NewTab);
            }
        }
        ui.add_space(2.0);

        let titles: Vec<String> = app
            .tabs
            .iter()
            .map(|tab| tab.title(app.lang()))
            .collect();
        let active = app.active;
        for (i, title) in titles.iter().enumerate() {
            let is_active = i == active;
            let title_color = if is_active { pal.link } else { pal.muted };
            let g = title_galley(ui, title, text_size, title_color, is_active);
            let close_w = 30.0;
            let w = g.size().x + 12.0 * 2.0 + close_w;
            let (rect, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click());
            let painter = ui.painter();
            if is_active {
                // Активная вкладка тоже реагирует на наведение — чуть светлее фон.
                let fill = if resp.hovered() {
                    themes::lighten(pal.bg, 0.10)
                } else {
                    pal.bg
                };
                painter.rect_filled(rect, egui::CornerRadius::same(0), fill);
            } else if resp.hovered() {
                // Фон читалки темнее полосы — hover «затемняет», а не серит.
                painter.rect_filled(rect, egui::CornerRadius::same(0), pal.bg);
            }
            painter.galley(
                pos2(rect.min.x + 12.0, rect.center().y - g.size().y * 0.5),
                g,
                title_color,
            );

            // Крестик закрытия — отдельная зона; красный при наведении,
            // чтобы отличать «закрыть» от «перейти к файлу».
            let xg = title_galley(ui, "×", 19.0 * scale, pal.faint, false);
            let x_min = pos2(
                rect.max.x - close_w + (close_w - xg.size().x) * 0.5,
                rect.center().y - xg.size().y * 0.5,
            );
            let x_rect = Rect::from_min_size(x_min, xg.size());
            let x_resp = ui.interact(
                x_rect,
                ui.scope_id().with(("tab_close", i)),
                Sense::click(),
            );
            let x_color = if x_resp.hovered() {
                egui::Color32::from_rgb(228, 82, 72)
            } else {
                pal.faint
            };
            painter.galley_with_override_text_color(x_min, xg, x_color);
            if resp.hovered() || x_resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }

            if x_resp.clicked() {
                actions.push(TabAction::Close(i));
            } else if resp.clicked() {
                actions.push(TabAction::Select(i));
            }
        }

        // Правый край полосы: стрелки переключения вкладок + настройки.
        let multi = app.tabs.len() > 1;
        let lead = (ui.available_width() - 30.0 * 2.0 - 44.0).max(4.0);
        ui.add_space(lead);
        if nav_arrow_button(ui, "tab_prev", true, multi, &pal, 16.0 * scale) {
            actions.push(TabAction::Select(next_tab(app.active, app.tabs.len(), true)));
        }
        if nav_arrow_button(ui, "tab_next", false, multi, &pal, 16.0 * scale) {
            actions.push(TabAction::Select(next_tab(app.active, app.tabs.len(), false)));
        }
        if settings_button(ui, app.show_settings, &pal, 18.0 * scale) {
            actions.push(TabAction::ToggleSettings);
        }
    });

    // Ctrl + колесо над полосой вкладок — переключение вкладок.
    {
        let strip = ui.min_rect();
        let over = ui
            .input(|i| i.pointer.interact_pos())
            .is_some_and(|p| strip.contains(p));
        let ctrl = ui.input(|i| i.modifiers.ctrl && !i.modifiers.shift && !i.modifiers.alt);
        let wheel: f32 = ui.input(|i| {
            i.events
                .iter()
                .map(|e| match e {
                    egui::Event::MouseWheel { delta, .. } => delta.y,
                    _ => 0.0,
                })
                .sum()
        });
        if over && ctrl && wheel != 0.0 && app.tabs.len() > 1 {
            actions.push(TabAction::Select(next_tab(
                app.active,
                app.tabs.len(),
                wheel < 0.0,
            )));
        }
    }

    // Горизонтальная линия под таб-баром.
    let r = ui.min_rect();
    ui.painter().line_segment(
        [pos2(r.min.x, r.max.y - 1.0), pos2(r.max.x, r.max.y - 1.0)],
        egui::Stroke::new(1.0, pal.border),
    );

    for action in actions {
        match action {
            TabAction::Select(i) => app.active = i,
            TabAction::Close(i) => app.close_tab(i),
            TabAction::NewTab => {
                if let Some(path) = pick_md_file() {
                    app.open(path);
                }
            }
            TabAction::ToggleSettings => app.show_settings = !app.show_settings,
        }
    }
}

/// Кнопка-стрелка ‹/› для переключения вкладок (шеврон рисуем сами —
/// глифы не во всех шрифтах). `enabled=false` при единственной вкладке.
fn nav_arrow_button(
    ui: &mut Ui,
    salt: &'static str,
    left: bool,
    enabled: bool,
    pal: &themes::Palette,
    size: f32,
) -> bool {
    let (rect, _) = ui.allocate_exact_size(vec2(30.0, 40.0), Sense::hover());
    let resp = ui.interact(rect, ui.scope_id().with(salt), Sense::click());
    let hovered = resp.hovered() && enabled;
    let color = if !enabled {
        pal.faint
    } else if hovered {
        pal.fg
    } else {
        pal.muted
    };
    let painter = ui.painter().clone();
    let c = rect.center();
    let s = size * 0.28;
    // Шеврон остриём в сторону переключения.
    let (tip, back) = if left {
        (pos2(c.x - s * 0.5, c.y), pos2(c.x + s * 0.5, c.y - s))
    } else {
        (pos2(c.x + s * 0.5, c.y), pos2(c.x - s * 0.5, c.y - s))
    };
    painter.line_segment([back, tip], egui::Stroke::new(1.8, color));
    painter.line_segment(
        [pos2(back.x, back.y + s * 2.0), tip],
        egui::Stroke::new(1.8, color),
    );
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    enabled && resp.clicked()
}

/// Кнопка-«ползунки» (открыть панель настроек): три линии, плоская,
/// без глифов-шрифтов (глиф шестерёнки не во всех шрифтах есть).
fn settings_button(ui: &mut Ui, active: bool, pal: &themes::Palette, size: f32) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(44.0, 40.0), Sense::click());
    let color = if active {
        pal.link
    } else if resp.hovered() {
        pal.fg
    } else {
        pal.muted
    };
    let painter = ui.painter().clone();
    let c = rect.center();
    let half = size * 0.5;
    for dy in [-size * 0.26, 0.0, size * 0.26] {
        painter.line_segment(
            [pos2(c.x - half, c.y + dy), pos2(c.x + half, c.y + dy)],
            egui::Stroke::new(1.6, color),
        );
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.clicked()
}

/// Статус-бар: слева — символы/строки/объём файла, справа — тумблер
/// «растягивать текст по ширине окна».
fn status_bar(ui: &mut Ui, app: &mut App, sel: Option<SelStats>) {
    let pal = *app.pal();
    let lang = app.lang();
    let em = app.settings.ui_font_size as f32 * 0.85;
    let h = 26.0;

    // Статистика активной вкладки (для загрузки — «чтение…»).
    let stats = match app.tabs.get(app.active).map(|t| &t.state) {
        Some(TabState::Ready(doc)) => Some(format!(
            "{} {} · {} {} · {}",
            fmt_num(doc.char_count),
            t(lang, "sb_symbols"),
            fmt_num(doc.line_count),
            t(lang, "sb_lines"),
            fmt_size(doc.source.len(), lang),
        )),
        Some(TabState::Loading { .. }) => Some(t(lang, "loading").to_string()),
        _ => None,
    };

    ui.horizontal(|ui| {
        ui.add_space(12.0);
        // Одна строка одним галлеем: гарантированно общий базлайн.
        let mut text = stats.unwrap_or_default();
        if let Some((chars, words, lines)) = sel {
            text.push_str(&format!(
                "   {}: {} {}, {} {}, {} {}",
                t(lang, "sb_selected"),
                fmt_num(chars),
                t(lang, "sb_symbols"),
                fmt_num(words),
                t(lang, "sb_words"),
                fmt_num(lines),
                t(lang, "sb_lines"),
            ));
        }
        if !text.is_empty() {
            let g = crate::view::widgets::plain_galley(ui, &text, em, pal.muted);
            let (r, _) = ui.allocate_exact_size(vec2(g.size().x, h), Sense::hover());
            ui.painter().galley(
                pos2(r.min.x, r.center().y - g.size().y * 0.5),
                g,
                pal.muted,
            );
        }

        // Тумблер «по ширине окна»: правый край — с запасом от угла ресайза окна.
        let label = t(lang, "st_fit_width");
        let g = crate::view::widgets::plain_galley(ui, label, em, pal.fg);
        let w = g.size().x + 18.0;
        let lead = (ui.available_width() - w - 25.0).max(0.0);
        ui.add_space(lead);
        let (r, _) = ui.allocate_exact_size(vec2(w, h - 8.0), Sense::hover());
        let resp = ui.interact(
            r,
            ui.scope_id().with("fit_width"),
            Sense::click(),
        );
        let on = app.settings.fit_width;
        let hovered = resp.hovered();
        let painter = ui.painter().clone();
        if on {
            painter.rect_filled(r, egui::CornerRadius::same(0), pal.fg);
        } else {
            let border = if hovered { pal.muted } else { pal.border };
            painter.rect(
                r,
                egui::CornerRadius::same(0),
                egui::Color32::TRANSPARENT,
                egui::Stroke::new(1.0, border),
                egui::StrokeKind::Middle,
            );
        }
        let color = if on {
            pal.bg
        } else if hovered {
            pal.fg
        } else {
            pal.muted
        };
        // painter.galley игнорирует цвет для «цветных» галлеев — только override.
        painter.galley_with_override_text_color(r.center() - g.size() * 0.5, g, color);
        if hovered {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if resp.clicked() {
            app.settings.fit_width = !on;
        }
    });

    // Верхняя граница бара.
    let r = ui.min_rect();
    ui.painter().line_segment(
        [pos2(r.min.x, r.min.y), pos2(r.max.x, r.min.y)],
        egui::Stroke::new(1.0, pal.border),
    );
}

/// Число с разделителями разрядов (пробел по 3 цифры).
fn fmt_num(n: usize) -> String {
    let s = n.to_string();
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (b.len() - i) % 3 == 0 {
            out.push(' ');
        }
        out.push(ch);
    }
    out
}

/// Размер файла: Б/КБ/МБ/ГБ (одна цифра после запятой).
fn fmt_size(bytes: usize, lang: Lang) -> String {
    let b = bytes as f64;
    let kb = 1024.0;
    let (v, unit) = if b < kb {
        (b, t(lang, "sb_b"))
    } else if b < kb * kb {
        (b / kb, t(lang, "sb_kb"))
    } else if b < kb * kb * kb {
        (b / (kb * kb), t(lang, "sb_mb"))
    } else {
        (b / (kb * kb * kb), t(lang, "sb_gb"))
    };
    if bytes < 1024 {
        format!("{} {unit}", v as usize)
    } else {
        format!("{v:.1} {unit}")
    }
}

fn title_galley(ui: &Ui, text: &str, size: f32, color: egui::Color32, bold: bool) -> Arc<Galley> {
    let mut job = LayoutJob::default();
    let mut fmt = TextFormat {
        font_id: eframe::egui::FontId::new(size, FontFamily::Proportional),
        color,
        ..Default::default()
    };
    if bold {
        fmt.coords = VariationCoords::new([(b"wght", 600.0)]);
    }
    job.sections.push(LayoutSection {
        leading_space: 0.0,
        byte_range: ByteIndex(0)..ByteIndex(text.len()),
        format: fmt,
    });
    job.text.push_str(text);
    ui.painter().layout_job(job)
}

fn show_loading(ui: &mut Ui, lang: Lang, em: f32) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() * 0.3);
        ui.label(egui::RichText::new(t(lang, "loading")).size(em * 1.4).weak());
    });
}

fn show_error(ui: &mut Ui, path: &Option<PathBuf>, err: &str, lang: Lang, em: f32) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() * 0.3);
        ui.label(egui::RichText::new(t(lang, "err_read")).size(em * 1.4).strong());
        ui.add_space(8.0);
        let path = path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        ui.label(egui::RichText::new(path).size(em).weak());
        ui.label(
            egui::RichText::new(err)
                .size(em)
                .weak()
                .color(egui::Color32::LIGHT_RED),
        );
    });
}
