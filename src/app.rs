//! Состояние приложения: вкладки, ввод (CLI/drag-drop/Ctrl+O), панели.
//! Большие файлы грузятся фоном: UI показывает блоки по мере разбора.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
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
use crate::md::anchors;
use crate::md::blocks::{self, Doc};
use crate::md::parser::{self, ParseMsg};
use crate::settings::model::{self, PanelMode, SettingsData, SessionData, Side, TabSession, ThemeMode, WIDTH_PRESETS};
use crate::settings::store;
use crate::view::selection::{self, SelStats, SelectionState};
use crate::view::themes::Typo;
use crate::view::{doc_view, find, find_bar, minimap, nav, outline, settings_panel, themes, welcome, Env};
use crate::watcher::{self, FileEvent};
use crate::fonts;
use eframe::egui::Pos2;

/// Задержка записи settings.json после последнего изменения (дебаунс слайдеров).
const SAVE_DELAY: Duration = Duration::from_millis(400);

pub struct App {
    tabs: Vec<Tab>,
    active: usize,
    /// Настройки (settings.json): типографика документа, тема, язык, UI-шрифт.
    settings: SettingsData,
    /// Контекст egui (сохранение сессии при выходе/перезапуске).
    egui_ctx: Context,
    /// Слежение за файлами вкладок: перечитать при изменении (этап 6).
    watcher: watcher::Watcher,
    /// Применённый режим темы (чтобы поймать смену System↔явной).
    applied_theme: ThemeMode,
    /// Применённая (эффективная) тёмная тема: выбор пользователя или системы.
    dark: bool,
    /// Панель настроек открыта.
    show_settings: bool,
    /// Открытое контекст-меню документа (правый клик).
    ctx_menu: Option<CtxMenu>,
    /// Открытое контекст-меню вкладки (правый клик по имени файла).
    tab_menu: Option<TabMenu>,
    /// Счётчик id вкладок (стабильные id скроллов/блоков на переключение вкладок).
    next_tab_id: u64,
    /// Последний выставленный заголовок окна (чтобы не спамить командой).
    title_sent: String,
    /// Отложенная запись настроек (последнее изменение + SAVE_DELAY).
    save_after: Option<Instant>,
}

/// Открытое контекст-меню документа: позиция и ссылка под кликом.
struct CtxMenu {
    pos: Pos2,
    link: Option<String>,
}

/// Открытое контекст-меню вкладки: позиция клика и вкладка.
struct TabMenu {
    pos: Pos2,
    idx: usize,
}

/// Действие контекст-меню (применяется после отрисовки).
enum CtxAction {
    CopyUrl(String),
    Copy,
    SelectAll,
}

/// Действие контекст-меню вкладки.
enum TabCtxAction {
    Explorer,
    SaveAs,
}

/// Слова в минуту для оценки времени чтения (как у MarkMello).
const READ_WPM: usize = 220;

pub struct Tab {
    /// Стабильный id вкладки (id скролла и блоков привязаны к нему).
    pub id: u64,
    pub path: Option<PathBuf>,
    pub state: TabState,
    /// Раскладка документа этой вкладки (высоты, кэш galley).
    pub layout: LayoutState,
    /// Состояние оглавления вкладки (высоты строк с переносами).
    pub toc: outline::TocState,
    /// Собственное выделение текста (переживает виртуальный скролл).
    pub selection: SelectionState,
    /// Поиск по документу этой вкладки.
    pub find: find::FindState,
    /// Файл недоступен (watcher): баннер над документом, содержимое
    /// остаётся читаемым; при появлении файла вкладка перечитается.
    pub missing: bool,
    /// Отложенное восстановление позиции чтения (сессия/перезагрузка):
    /// применяет doc_view, когда документ готов и раскладка обеспечена.
    pub pending_restore: Option<nav::RestorePos>,
    /// Префикс текста якорного блока — сверка с новым документом
    /// (индексы блоков при перечитывании плывут).
    pub anchor: Option<String>,
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
            toc: outline::TocState::default(),
            selection: SelectionState::default(),
            find: find::FindState::default(),
            missing: false,
            pending_restore: None,
            anchor: None,
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
            toc: outline::TocState::default(),
            selection: SelectionState::default(),
            find: find::FindState::default(),
            missing: false,
            pending_restore: None,
            anchor: None,
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
    pub fn new(cc: &eframe::CreationContext, paths: Vec<PathBuf>, mut settings: SettingsData) -> Self {
        fonts::install(&cc.egui_ctx);
        // Эффективная тема: настройка или системная; egui знает её с старта.
        let dark = effective_dark(&cc.egui_ctx, settings.theme);
        apply_theme(&cc.egui_ctx, settings.theme, dark);

        // Прогрев подсветки синтаксиса в фоне (один раз, ~50 мс).
        crate::highlight::warm_up();

        let mut next_id = 0;
        let mut tabs: Vec<Tab> = Vec::new();
        let mut active = 0;
        if !paths.is_empty() {
            // Явные аргументы CLI: сессию не восстанавливаем.
            for p in paths {
                settings.session.push_recent(&p);
                tabs.push(Tab::load(next_id, p));
                next_id += 1;
            }
        } else if !settings.session.tabs.is_empty() {
            // Сессия: вкладки прошлого запуска с позициями чтения; файлы,
            // которых больше нет, пропускаем молча.
            for ts in &settings.session.tabs {
                let p = PathBuf::from(&ts.path);
                if !p.is_file() {
                    continue;
                }
                let mut tab = Tab::load(next_id, p);
                tab.pending_restore = Some(nav::RestorePos {
                    y: ts.y,
                    block: ts.block,
                    block_top: ts.block_top,
                });
                tab.anchor = ts.anchor.clone();
                tabs.push(tab);
                next_id += 1;
            }
            active = settings.session.active.min(tabs.len().saturating_sub(1));
        } else if let Some(demo) = find_demo_md() {
            // Без аргументов и сессии — demo.md для проверки (если найден).
            settings.session.push_recent(&demo);
            tabs.push(Tab::load(next_id, demo));
            next_id += 1;
        }
        if tabs.is_empty() {
            tabs.push(Tab::welcome(next_id));
            next_id += 1;
        }

        let mut app = Self {
            tabs,
            active,
            applied_theme: settings.theme,
            egui_ctx: cc.egui_ctx.clone(),
            watcher: watcher::Watcher::new(),
            settings,
            dark,
            show_settings: false,
            ctx_menu: None,
            tab_menu: None,
            next_tab_id: next_id + 1,
            title_sent: String::new(),
            save_after: None,
        };
        // Синхронно готовые вкладки: вооружить восстановление позиций.
        let ctx = app.egui_ctx.clone();
        for i in 0..app.tabs.len() {
            app.arm_restore(&ctx, i);
        }
        app
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
        let mut finished: Vec<(usize, TabState)> = Vec::new();
        for (i, tab) in self.tabs.iter_mut().enumerate() {
            let mut done: Option<TabState> = None;
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
                        done = Some(TabState::Ready(Arc::new(full)));
                    }
                    Pumped::Failed(e) => done = Some(TabState::Error(e)),
                }
            }
            if let Some(state) = done {
                finished.push((i, state));
            }
        }
        for (i, state) in finished {
            self.tabs[i].state = state;
            // Позиция чтения фоново загруженной вкладки — после раскладки.
            self.arm_restore(ctx, i);
        }
        if loading {
            ctx.request_repaint();
        }
        loading
    }

    /// Слежение за файлами вкладок: набор путей, события с дебаунсом 300 мс →
    /// перечитать изменившиеся, пропавшие — баннер «файл недоступен».
    fn pump_watcher(&mut self, ctx: &Context) {
        let files: Vec<PathBuf> = self.tabs.iter().filter_map(|t| t.path.clone()).collect();
        self.watcher.set_files(&files);
        let (events, until) = self.watcher.pump(Instant::now());
        for ev in events {
            match ev {
                FileEvent::Changed(p) => {
                    for i in 0..self.tabs.len() {
                        if paths_eq(self.tabs[i].path.as_deref(), Some(&p)) {
                            self.reload_tab(ctx, i);
                        }
                    }
                }
                FileEvent::Gone(p) => {
                    let mut hit = false;
                    for tab in &mut self.tabs {
                        if paths_eq(tab.path.as_deref(), Some(&p)) && !tab.missing {
                            tab.missing = true;
                            hit = true;
                        }
                    }
                    if hit {
                        ctx.request_repaint();
                    }
                }
            }
        }
        if let Some(d) = until {
            ctx.request_repaint_after(d);
        }
    }

    /// Перечитывает файл вкладки (watcher), сохраняя позицию чтения по якорю
    /// ближайшего блока: префикс текста первого видимого блока ищется в новом
    /// документе (индексы блоков при перечитывании плывут). Выделение и
    /// поиск сбрасываются — их координаты относились к старому документу.
    fn reload_tab(&mut self, ctx: &Context, idx: usize) {
        let Some(tab) = self.tabs.get(idx) else { return };
        let Some(path) = tab.path.clone() else { return };
        let (pos, anchor) = match nav::metrics(ctx, tab.id) {
            Some(m) => {
                let i0 = tab
                    .layout
                    .visible_range((m.offset - doc_view::TOP_PAD).max(0.0), m.offset + m.viewport_h)
                    .start;
                let anchor = state_doc(&tab.state).and_then(|doc| blocks::text_prefix(doc, i0));
                let pos = nav::RestorePos {
                    y: m.offset,
                    block: i0,
                    block_top: doc_view::TOP_PAD + tab.layout.offset(i0),
                };
                (Some(pos), anchor)
            }
            None => (None, None),
        };
        let id = tab.id;
        let mut fresh = Tab::load(id, path);
        fresh.pending_restore = pos;
        fresh.anchor = anchor;
        self.tabs[idx] = fresh;
        self.arm_restore(ctx, idx);
    }

    /// Применяет отложенное восстановление позиции вкладки: готовый документ —
    /// сразу (doc_view применит при готовой раскладке), фоновый — по готовности
    /// (pump_tabs). Якорь блока сверяется с новым документом по префиксу.
    fn arm_restore(&mut self, ctx: &Context, idx: usize) {
        let Some(tab) = self.tabs.get_mut(idx) else { return };
        if !matches!(tab.state, TabState::Ready(_)) {
            return; // ещё грузится — позовут при Done
        }
        let Some(mut pos) = tab.pending_restore.take() else {
            return;
        };
        if let Some(a) = tab.anchor.as_deref()
            && let Some(doc) = state_doc(&tab.state)
            && let Some(i) = blocks::find_by_prefix(doc, a)
        {
            pos.block = i;
        }
        nav::set_restore(ctx, tab.id, pos);
    }

    /// Снимок сессии: вкладки с позициями чтения (оффсет + якорь блока),
    /// последние файлы, геометрия окна. Вкладки без метрик прокрутки (ещё не
    /// показывались в этом запуске) сохраняют прошлое значение позиции.
    fn session_snapshot(&self) -> SessionData {
        let old: HashMap<&str, &TabSession> = self
            .settings
            .session
            .tabs
            .iter()
            .map(|t| (t.path.as_str(), t))
            .collect();
        let mut tabs = Vec::new();
        for tab in &self.tabs {
            let Some(path) = tab.path.as_ref() else { continue };
            if matches!(tab.state, TabState::Error(_)) {
                continue; // нечитаемый файл — вкладку не сохраняем
            }
            let ps = path.display().to_string();
            let entry = match nav::metrics(&self.egui_ctx, tab.id) {
                Some(m) => {
                    let i0 = tab
                        .layout
                        .visible_range(
                            (m.offset - doc_view::TOP_PAD).max(0.0),
                            m.offset + m.viewport_h,
                        )
                        .start;
                    TabSession {
                        path: ps.clone(),
                        y: m.offset,
                        block: i0,
                        block_top: doc_view::TOP_PAD + tab.layout.offset(i0),
                        anchor: state_doc(&tab.state)
                            .and_then(|doc| blocks::text_prefix(doc, i0)),
                    }
                }
                None => old.get(ps.as_str()).map(|t| (*t).clone()).unwrap_or(TabSession {
                    path: ps.clone(),
                    y: 0.0,
                    block: 0,
                    block_top: doc_view::TOP_PAD,
                    anchor: None,
                }),
            };
            tabs.push(entry);
        }
        let active = self.active.min(tabs.len().saturating_sub(1));

        // Геометрия окна: позиция и размер — только в нормальном состоянии
        // (развёрнутый прямоугольник не сохраняем); поинты, координаты
        // монитора egui.
        let (inner, maximized) = self
            .egui_ctx
            .input(|i| (i.viewport().inner_rect, i.viewport().maximized));
        let (mut win_pos, mut win_size) =
            (self.settings.session.win_pos, self.settings.session.win_size);
        let mut win_max = self.settings.session.win_maximized;
        if maximized == Some(true) {
            win_max = true;
        } else if let Some(r) = inner {
            win_max = false;
            win_pos = Some([r.min.x.round() as i32, r.min.y.round() as i32]);
            win_size = Some([r.width().round(), r.height().round()]);
        }

        SessionData {
            tabs,
            active,
            recent: self.settings.session.recent.clone(),
            win_pos,
            win_size,
            win_maximized: win_max,
        }
    }

    /// Немедленная запись сессии (выход из приложения, «Перезапуск»).
    fn persist_session(&self) {
        let mut data = self.settings.clone();
        data.session = self.session_snapshot();
        if let Err(e) = store::save(&data) {
            log::warn!("settings.json: {e}");
        }
    }

    /// Перезапуск: сохраняем сессию и стартуем новый процесс без аргументов —
    /// он восстановит вкладки и позиции (например, A/B шрифтов владельцем).
    fn restart(&self) -> ! {
        self.persist_session();
        let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("DarkTechMD.exe"));
        if let Err(e) = std::process::Command::new(exe).spawn() {
            log::warn!("перезапуск не удался: {e}");
        }
        std::process::exit(0);
    }

    /// Документ активной вкладки: готовый или ещё стримящийся (для панелей
    /// навигации и якорей).
    fn active_doc(&self) -> Option<(&Doc, u64)> {
        let tab = self.tabs.get(self.active)?;
        Some((state_doc(&tab.state)?, tab.id))
    }

    /// Шаг по попаданиям поиска (Enter/F3/кнопки): вперёд/назад, по кругу;
    /// `open_bar` — открыть панель, если закрыта (F3).
    fn find_step(&mut self, ctx: &Context, back: bool, open_bar: bool) {
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        if state_doc(&tab.state).is_none() {
            return;
        }
        if !tab.find.open {
            if !open_bar {
                return;
            }
            tab.find.open = true;
        }
        let Some(hit) = tab.find.step(back) else {
            return;
        };
        let typo = self.settings.typo();
        let (tab_id, layout) = (tab.id, &tab.layout);
        let doc = state_doc(&tab.state).expect("документ проверен выше");
        request_scroll_to_hit(ctx, tab_id, doc, layout, &typo, hit);
    }

    /// Прокачка поиска всех вкладок: (пере)запуск при изменившемся запросе,
    /// приём пакетов, выбор стартового попадания (активная вкладка).
    fn pump_find(&mut self, ctx: &Context) {
        let typo = self.settings.typo();
        for ti in 0..self.tabs.len() {
            let tab = &mut self.tabs[ti];
            // Запрос стёрли — гасим результаты (подсветки быть не должно).
            if tab.find.query.is_empty() && tab.find.started().is_some() {
                tab.find.reset_results();
            }
            // (Пере)запуск: панель открыта, документ готов, запрос изменился.
            if tab.find.open
                && !tab.find.query.is_empty()
                && tab.find.started() != Some(tab.find.query.as_str())
                && let TabState::Ready(arc) = &tab.state
            {
                tab.find.restart(arc);
            }
            let p = tab.find.pump();
            if p.changed {
                ctx.request_repaint();
            }
            if p.landed && ti == self.active {
                // Первые попадания запроса: стартуем с первого видимого/ниже.
                let off = nav::metrics(ctx, tab.id).map_or(0.0, |m| m.offset);
                tab.find.current = find::initial_hit(&tab.find.hits, &tab.layout, off);
                if let Some(hit) = tab.find.cur_hit() {
                    let (tab_id, layout) = (tab.id, &tab.layout);
                    let doc = state_doc(&tab.state).expect("searching tab has doc");
                    request_scroll_to_hit(ctx, tab_id, doc, layout, &typo, hit);
                }
            }
        }
    }

    /// Видимость оглавления по режиму (Авто — при ≥3 заголовках).
    fn toc_visible(&self) -> bool {
        self.active_doc().is_some_and(|(doc, _)| match self.settings.toc_mode {
            PanelMode::Off => false,
            PanelMode::On => !doc.headings.is_empty(),
            PanelMode::Auto => doc.headings.len() >= 3,
        })
    }

    /// Видимость миникарты по режиму (Авто — документ длиннее ~2,5 экранов;
    /// метрики прокрутки — с прошлого кадра).
    fn minimap_visible(&self, ctx: &Context) -> bool {
        match self.settings.minimap_mode {
            PanelMode::Off => false,
            PanelMode::On => self.active_doc().is_some_and(|(doc, _)| !doc.blocks.is_empty()),
            PanelMode::Auto => self
                .active_doc()
                .and_then(|(_, id)| nav::metrics(ctx, id))
                .is_some_and(|m| m.content_h > m.viewport_h * 2.5),
        }
    }

    /// Быстрый тумблер из статус-бара: видно → Выкл, скрыто → Вкл (режим
    /// «Авто» заменяется явным выбором).
    fn toggle_toc(&mut self) {
        self.settings.toc_mode = if self.toc_visible() {
            PanelMode::Off
        } else {
            PanelMode::On
        };
    }

    fn toggle_minimap(&mut self, ctx: &Context) {
        self.settings.minimap_mode = if self.minimap_visible(ctx) {
            PanelMode::Off
        } else {
            PanelMode::On
        };
    }

    /// Открывает файл новой вкладкой (или заменяет единственную welcome-вкладку).
    fn open(&mut self, path: PathBuf) {
        let replace_welcome = self.tabs.len() == 1 && self.tabs[0].path.is_none();
        self.settings.session.push_recent(&path);
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
            // Якорь внутри документа: скролл к заголовку с таким slug-ом.
            if let Some((doc, tab_id)) = self.active_doc()
                && let Some(block_idx) = anchors::resolve(doc, url)
            {
                nav::request_scroll(ctx, tab_id, nav::ScrollRequest::Block(block_idx));
            }
        } else {
            ctx.open_url(egui::OpenUrl::new_tab(url));
        }
    }

    /// Копирование выделения активной вкладки (plain text, из фона).
    fn copy_selection(&mut self) {
        if let Some(tab) = self.tabs.get(self.active)
            && let Some(doc) = state_doc(&tab.state)
            && let Some(text) = selection::copy_text(doc, &tab.selection)
        {
            crate::clipboard::copy_in_background(text);
        }
    }

    /// Контекст-меню документа (правый клик): Копировать адрес (на ссылке) /
    /// Копировать / Выделить всё. Плоское, поверх всего; закрывается кликом
    /// мимо, Esc или действием.
    fn show_context_menu(&mut self, ctx: &Context) {
        let Some(menu) = self.ctx_menu.take() else { return };
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            return; // закрыть, не возвращая
        }

        let pal = self.pal();
        let lang = self.lang();
        let em = self.settings.ui_font_size as f32;
        let has_selection = self
            .tabs
            .get(self.active)
            .is_some_and(|t| !t.selection.is_empty());

        // Элементы: подпись, активность, действие.
        let mut items: Vec<(&str, bool, CtxAction)> = Vec::new();
        if let Some(url) = &menu.link {
            items.push((t(lang, "cm_copy_url"), true, CtxAction::CopyUrl(url.clone())));
        }
        items.push((t(lang, "copy"), has_selection, CtxAction::Copy));
        items.push((t(lang, "cm_select_all"), true, CtxAction::SelectAll));

        // Не вылезать за края экрана.
        let screen = ctx.viewport_rect();
        let w = 240.0 * self.ui_scale();
        let h = items.len() as f32 * (em * 1.7) + 10.0;
        let pos = pos2(
            menu.pos.x.min(screen.max.x - w - 6.0).max(screen.min.x + 4.0),
            menu.pos.y.min(screen.max.y - h - 6.0).max(screen.min.y + 4.0),
        );

        let mut clicked: Option<usize> = None;
        let resp = eframe::egui::Area::new(eframe::egui::Id::unique("ctx_menu"))
            .order(eframe::egui::Order::Foreground)
            .fixed_pos(pos)
            .interactable(true)
            .show(ctx, |ui| {
                let frame = eframe::egui::Frame::NONE
                    .fill(pal.code_bg)
                    .stroke(egui::Stroke::new(1.0, pal.border))
                    .inner_margin(egui::Margin::same(4));
                frame.show(ui, |ui| {
                    ui.set_min_width(w - 8.0);
                    ui.vertical(|ui| {
                        for (i, (label, enabled, _)) in items.iter().enumerate() {
                            if ctx_menu_item(ui, ("ctx_item", i), label, *enabled, em, pal)
                                && clicked.is_none()
                            {
                                clicked = Some(i);
                            }
                        }
                    });
                });
            })
            .response;

        if let Some(i) = clicked {
            match items.swap_remove(i).2 {
                CtxAction::CopyUrl(url) => crate::clipboard::copy_in_background(url),
                CtxAction::Copy => self.copy_selection(),
                CtxAction::SelectAll => {
                    if let Some(tab) = self.tabs.get_mut(self.active)
                        && let Some(doc) = state_doc(&tab.state)
                    {
                        tab.selection.select_all(doc);
                        tab.selection.recompute_stats_if_due(doc);
                    }
                }
            }
            return; // действие закрывает меню
        }
        if resp.clicked_elsewhere() {
            return; // клик мимо — закрыть
        }
        self.ctx_menu = Some(menu); // живём дальше
    }

    /// Контекст-меню вкладки (правый клик по имени файла в таб-баре):
    /// «Показать в проводнике» / «Сохранить как…» (копия в другое место).
    fn show_tab_menu(&mut self, ctx: &Context) {
        let Some(menu) = self.tab_menu.take() else { return };
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            return; // закрыть, не возвращая
        }

        let pal = self.pal();
        let lang = self.lang();
        let em = self.settings.ui_font_size as f32;

        // Вкладка могла закрыться, пока меню открыто.
        let Some(tab) = self.tabs.get(menu.idx) else { return };
        let Some(path) = tab.path.clone() else { return };
        let file_ok = path.is_file();
        let dir_ok = path.parent().is_some_and(|d| d.is_dir());
        let can_save = file_ok || state_doc(&tab.state).is_some();
        // заимствование tab заканчивается здесь (NLL)

        let mut items: Vec<(&str, bool, TabCtxAction)> = vec![
            (t(lang, "tm_explorer"), file_ok || dir_ok, TabCtxAction::Explorer),
            (t(lang, "tm_save_as"), can_save, TabCtxAction::SaveAs),
        ];

        // Не вылезать за края экрана (как у меню документа).
        let screen = ctx.viewport_rect();
        let w = 240.0 * self.ui_scale();
        let h = items.len() as f32 * (em * 1.7) + 10.0;
        let pos = pos2(
            menu.pos.x.min(screen.max.x - w - 6.0).max(screen.min.x + 4.0),
            menu.pos.y.min(screen.max.y - h - 6.0).max(screen.min.y + 4.0),
        );

        let mut clicked: Option<usize> = None;
        let resp = eframe::egui::Area::new(eframe::egui::Id::unique("tab_menu"))
            .order(eframe::egui::Order::Foreground)
            .fixed_pos(pos)
            .interactable(true)
            .show(ctx, |ui| {
                let frame = eframe::egui::Frame::NONE
                    .fill(pal.code_bg)
                    .stroke(egui::Stroke::new(1.0, pal.border))
                    .inner_margin(egui::Margin::same(4));
                frame.show(ui, |ui| {
                    ui.set_min_width(w - 8.0);
                    ui.vertical(|ui| {
                        for (i, (label, enabled, _)) in items.iter().enumerate() {
                            if ctx_menu_item(ui, ("tab_menu_item", i), label, *enabled, em, pal)
                                && clicked.is_none()
                            {
                                clicked = Some(i);
                            }
                        }
                    });
                });
            })
            .response;

        if let Some(i) = clicked {
            match items.swap_remove(i).2 {
                TabCtxAction::Explorer => show_in_explorer(&path),
                TabCtxAction::SaveAs => self.save_copy_as(menu.idx),
            }
            return; // действие закрывает меню
        }
        if resp.clicked_elsewhere() {
            return; // клик мимо — закрыть
        }
        self.tab_menu = Some(menu); // живём дальше
    }

    /// «Сохранить как…»: точная побайтовая копия файла; файла уже нет —
    /// пишем последний прочитанный текст вкладки.
    fn save_copy_as(&self, idx: usize) {
        let Some(tab) = self.tabs.get(idx) else { return };
        let Some(path) = tab.path.as_ref() else { return };
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "document.md".to_string());
        let Some(dst) = rfd::FileDialog::new()
            .add_filter("Markdown", &["md", "markdown", "mdown"])
            .set_file_name(&name)
            .save_file()
        else {
            return;
        };
        if path.is_file() {
            if let Err(e) = std::fs::copy(path, &dst) {
                log::warn!("копия не записана: {e}");
            }
        } else if let Some(doc) = state_doc(&tab.state) {
            if let Err(e) = std::fs::write(&dst, doc.source.as_ref()) {
                log::warn!("копия не записана: {e}");
            }
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

        // Ctrl+Tab / Ctrl+Shift+Tab — переключение вкладок. Shift-вариант
        // проверяем ПЕРВЫМ: consume_key матчит лишний Shift (matches_logically).
        if ctx.input_mut(|i| {
            i.consume_key(Modifiers::CTRL | Modifiers::SHIFT, Key::Tab)
        }) {
            self.active = next_tab(self.active, self.tabs.len(), true);
        } else if ctx.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::Tab)) {
            self.active = next_tab(self.active, self.tabs.len(), false);
        }

        // Esc — закрыть панель настроек, иначе поиск, иначе — снять выделение
        // и контекст-меню.
        if self.show_settings {
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
                self.show_settings = false;
            }
        } else if let Some(tab) = self.tabs.get_mut(self.active)
            && tab.find.open
            && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape))
        {
            tab.find.close();
        } else if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            self.ctx_menu = None;
            self.tab_menu = None;
            if let Some(tab) = self.tabs.get_mut(self.active) {
                tab.selection.clear();
            }
        }

        // Ctrl+F — открыть поиск активной вкладки (только с документом).
        if ctx.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::F)) {
            if let Some(tab) = self.tabs.get_mut(self.active)
                && state_doc(&tab.state).is_some()
            {
                tab.find.open = true;
            }
        }

        // Поле поиска открыто: печать/вставка и правки уходят в запрос
        // (поле своё — TextEdit egui не используем, ввод ловим из очереди).
        if let Some(tab) = self.tabs.get_mut(self.active)
            && tab.find.open
        {
            // Event::Text — печать; Event::Paste — Ctrl+V (egui-winit
            // превращает его в синтетическое событие, как Copy).
            let mut insert = String::new();
            ctx.input_mut(|i| {
                i.events.retain(|e| match e {
                    egui::Event::Text(s) | egui::Event::Paste(s) => {
                        insert.push_str(s);
                        false
                    }
                    _ => true,
                });
            });
            if !insert.is_empty() {
                tab.find.insert_text(&insert);
            }
            // Backspace/Delete (Ctrl — по словам), стрелки, Home/End.
            if ctx.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::Backspace)) {
                tab.find.backspace(true);
            } else if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Backspace)) {
                tab.find.backspace(false);
            }
            if ctx.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::Delete)) {
                tab.find.delete_ahead(true);
            } else if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Delete)) {
                tab.find.delete_ahead(false);
            }
            if ctx.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::ArrowLeft)) {
                tab.find.caret_left(true);
            } else if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowLeft)) {
                tab.find.caret_left(false);
            }
            if ctx.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::ArrowRight)) {
                tab.find.caret_right(true);
            } else if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowRight)) {
                tab.find.caret_right(false);
            }
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Home)) {
                tab.find.caret_home();
            } else if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::End)) {
                tab.find.caret_end();
            }
        }

        // F3 / Shift+F3 — следующее/предыдущее попадание (открывает панель);
        // Enter / Shift+Enter — то же при открытой панели.
        if ctx.input_mut(|i| i.consume_key(Modifiers::SHIFT, Key::F3)) {
            self.find_step(&ctx, true, true);
        } else if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::F3)) {
            self.find_step(&ctx, false, true);
        }
        if self.tabs.get(self.active).is_some_and(|t| t.find.open) {
            if ctx.input_mut(|i| i.consume_key(Modifiers::SHIFT, Key::Enter)) {
                self.find_step(&ctx, true, false);
            } else if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
                self.find_step(&ctx, false, false);
            }
        }

        // Ctrl+A — при открытом поиске выделить запрос, иначе весь документ.
        let find_open = self.tabs.get(self.active).is_some_and(|t| t.find.open);
        if find_open {
            if ctx.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::A))
                && let Some(tab) = self.tabs.get_mut(self.active)
            {
                tab.find.select_all_field();
            }
        } else if ctx.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::A)) {
            if let Some(tab) = self.tabs.get_mut(self.active)
                && let Some(doc) = state_doc(&tab.state)
            {
                tab.selection.select_all(doc);
                tab.selection.recompute_stats_if_due(doc);
            }
        }

        // Ctrl+C — выделение как plain text (клипборд — только из фона).
        // egui-winit перехватывает Ctrl+C/X ДО Key-события (is_copy_command →
        // синтетический Event::Copy/Cut, lib.rs egui-winit) — consume_key их не
        // видит; забираем события из очереди сами. При открытом поиске копируем
        // (Ctrl+X — вырезаем) запрос.
        let mut copy_evt = false;
        let mut cut_evt = false;
        ctx.input_mut(|i| {
            let before = i.events.len();
            i.events.retain(|e| match e {
                egui::Event::Copy => {
                    copy_evt = true;
                    false
                }
                egui::Event::Cut => {
                    cut_evt = true;
                    false
                }
                _ => true,
            });
            before != i.events.len()
        });
        if copy_evt || cut_evt {
            if find_open {
                let tab = &mut self.tabs[self.active];
                if !tab.find.query.is_empty() {
                    crate::clipboard::copy_in_background(tab.find.query.clone());
                }
                if cut_evt {
                    tab.find.query.clear();
                    tab.find.caret = 0;
                    tab.find.reset_results();
                }
            } else {
                self.copy_selection();
            }
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

/// Документ из состояния вкладки (готовый или стримящийся чанками).
fn state_doc(state: &TabState) -> Option<&Doc> {
    match state {
        TabState::Ready(arc) => Some(arc.as_ref()),
        TabState::Loading { doc, .. } => Some(doc),
        _ => None,
    }
}

/// Просьба скролла к попаданию поиска (обрабатывает doc_view ближайшим
/// кадром): попадание на ~35% высоты вьюпорта сверху.
fn request_scroll_to_hit(
    ctx: &Context,
    tab_id: u64,
    doc: &Doc,
    layout: &LayoutState,
    typo: &Typo,
    hit: find::Hit,
) {
    let y = find::hit_y(doc, layout, typo, hit);
    let vh = nav::metrics(ctx, tab_id).map_or(400.0, |m| m.viewport_h);
    nav::request_scroll(ctx, tab_id, nav::ScrollRequest::Y((y - vh * 0.35).max(0.0)));
}

/// Пути указывают на один файл (Windows: без учёта регистра).
fn paths_eq(a: Option<&Path>, b: Option<&Path>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => path_str_eq(a, b),
        _ => false,
    }
}

#[cfg(windows)]
fn path_str_eq(a: &Path, b: &Path) -> bool {
    a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
}

#[cfg(not(windows))]
fn path_str_eq(a: &Path, b: &Path) -> bool {
    a == b
}

/// Полоса «файл недоступен» над документом (файл удалён/переименован):
/// содержимое остаётся читаемым; при появлении файла watcher перечитает.
fn missing_banner(ui: &mut Ui, path: &Option<PathBuf>, pal: &themes::Palette, lang: Lang, em: f32) {
    let h = (em * 2.1).max(30.0);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    let painter = ui.painter().clone();
    painter.rect_filled(r, egui::CornerRadius::same(0), pal.code_bg);
    painter.line_segment(
        [pos2(r.min.x, r.max.y - 0.5), pos2(r.max.x, r.max.y - 0.5)],
        egui::Stroke::new(1.0, pal.border),
    );
    painter.rect_filled(
        Rect::from_min_size(r.min, vec2(3.0, r.height())),
        egui::CornerRadius::same(0),
        pal.alert_caution,
    );
    let msg = title_galley(ui, t(lang, "err_file_gone"), em, pal.alert_caution, true);
    let path = path
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let pg = if path.is_empty() {
        None
    } else {
        Some(title_galley(ui, &path, em * 0.9, pal.muted, false))
    };
    let cy = r.center().y;
    let mut x = r.min.x + 14.0;
    painter.galley(pos2(x, cy - msg.size().y * 0.5), msg.clone(), pal.alert_caution);
    x += msg.size().x + 12.0;
    if let Some(pg) = pg {
        painter.galley_with_override_text_color(pos2(x, cy - pg.size().y * 0.5), pg, pal.muted);
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

/// Проводник: выделить файл (файла нет — открыть его каталог).
fn show_in_explorer(path: &Path) {
    let spawned = if path.is_file() {
        std::process::Command::new("explorer")
            .arg("/select,")
            .arg(path)
            .spawn()
            .is_ok()
    } else if let Some(dir) = path.parent().filter(|d| d.is_dir()) {
        std::process::Command::new("explorer").arg(dir).spawn().is_ok()
    } else {
        false
    };
    if !spawned {
        log::warn!("проводник не открылся: {}", path.display());
    }
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
        self.pump_find(&ctx);
        self.pump_watcher(&ctx);

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
            let mut restart = false;
            egui::Panel::right("settings_panel")
                .resizable(false)
                .exact_size(w)
                .frame(Frame::NONE.fill(pal.code_bg))
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("settings_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            if settings_panel::show(ui, &mut self.settings, pal, lang) {
                                restart = true;
                            }
                        });
                    // Плоская граница слева (сам рисуем — стили egui не используем).
                    let r = ui.min_rect();
                    ui.painter().line_segment(
                        [pos2(r.min.x, r.min.y), pos2(r.min.x, r.max.y)],
                        egui::Stroke::new(1.0, pal.border),
                    );
                });
            if restart {
                self.restart();
            }
        }

        // Панели навигации: оглавление (сторона — из настроек) и миникарта.
        let show_toc = self.toc_visible();
        let show_mm = self.minimap_visible(&ctx);

        // Ширины панелей — наши (settings.json), растяжение — своей ручкой:
        // механизм PanelState у egui не сохранял размер между кадрами.
        let pal = self.pal();
        let mut toc_w = self.settings.toc_width;
        let mut mm_w = self.settings.minimap_width;
        let left = self.settings.toc_side == Side::Left;

        if show_toc
            && let Some(tab) = self.tabs.get_mut(self.active)
            && let Some(doc) = state_doc(&tab.state)
        {
            let (tab_id, layout, toc) = (tab.id, &tab.layout, &mut tab.toc);
            let em = self.settings.ui_font_size as f32;
            let max_level = self.settings.toc_max_level;
            let panel = if left {
                egui::Panel::left("outline")
            } else {
                egui::Panel::right("outline")
            };
            panel
                .resizable(false)
                .exact_size(toc_w)
                .show_separator_line(false)
                .frame(Frame::NONE.fill(pal.bg))
                .show(ui, |ui| {
                    outline::show(ui, doc, layout, pal, tab_id, max_level, em, toc);
                    resize_handle(ui, "toc_resize", left, ui.max_rect(), pal, &mut toc_w, model::TOC_WIDTH_RANGE);
                });
        }

        if show_mm
            && let Some(tab) = self.tabs.get_mut(self.active)
            && let Some(doc) = state_doc(&tab.state)
        {
            let (tab_id, layout) = (tab.id, &tab.layout);
            let dark = self.dark;
            egui::Panel::right("minimap")
                .resizable(false)
                .exact_size(mm_w)
                .show_separator_line(false)
                .frame(Frame::NONE.fill(pal.bg))
                .show(ui, |ui| {
                    minimap::show(ui, doc, layout, pal, tab_id, dark);
                    resize_handle(ui, "mm_resize", false, ui.max_rect(), pal, &mut mm_w, model::MM_WIDTH_RANGE);
                });
        }

        if toc_w != self.settings.toc_width {
            self.settings.toc_width = toc_w;
        }
        if mm_w != self.settings.minimap_width {
            self.settings.minimap_width = mm_w;
        }

        // Статус-бар: статистика документа + выделение + тумблеры панелей.
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
                    status_bar(ui, self);
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
            table_scroll: self.settings.table_scroll,
            code_line_numbers: self.settings.code_line_numbers,
            code_highlight: self.settings.code_highlight,
        };

        let mut doc_events: Vec<doc_view::DocEvent> = Vec::new();
        let mut welcome_action: Option<welcome::Action> = None;
        // Последние файлы для welcome-экрана (существующие, до 8 строк).
        let recent: Vec<PathBuf> = if self
            .tabs
            .get(self.active)
            .is_some_and(|t| matches!(t.state, TabState::Welcome))
        {
            self.settings
                .session
                .recent
                .iter()
                .map(PathBuf::from)
                .filter(|p| p.is_file())
                .take(8)
                .collect()
        } else {
            Vec::new()
        };
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
                    missing,
                    selection,
                    find,
                    ..
                } = tab;
                match state {
                    TabState::Error(err) => {
                        show_error(ui, path, err, env.lang, ui_em);
                    }
                    TabState::Welcome => {
                        welcome_action = welcome::show(ui, env.lang, pal, ui_em, &recent);
                    }
                    TabState::Loading { doc, .. } => {
                        if *missing {
                            missing_banner(ui, path, pal, env.lang, ui_em);
                        }
                        if doc.blocks.is_empty() {
                            show_loading(ui, env.lang, ui_em);
                        } else {
                            doc_events.extend(doc_view::show(ui, doc, layout, &env, *id, selection, find));
                        }
                    }
                    TabState::Ready(arc) => {
                        if *missing {
                            missing_banner(ui, path, pal, env.lang, ui_em);
                        }
                        doc_events.extend(doc_view::show(ui, arc, layout, &env, *id, selection, find));
                    }
                }
            });
        for ev in doc_events {
            match ev {
                doc_view::DocEvent::OpenLink(url) => self.open_link(&url, &ctx),
                doc_view::DocEvent::ContextMenu { pos, link } => {
                    self.tab_menu = None;
                    self.ctx_menu = Some(CtxMenu { pos, link });
                    ctx.request_repaint();
                }
            }
        }
        match welcome_action {
            Some(welcome::Action::Dialog) => {
                if let Some(path) = pick_md_file() {
                    self.open(path);
                }
            }
            Some(welcome::Action::Recent(path)) => self.open(path),
            None => {}
        }

        // Контекст-меню — поверх всего, после центральной панели.
        self.show_context_menu(&ctx);

        // Меню вкладки (правый клик по имени файла в таб-баре).
        self.show_tab_menu(&ctx);

        // Попап поиска активной вкладки (Ctrl+F) — плавающий, поверх документа.
        if self.tabs.get(self.active).is_some_and(|t| {
            t.find.open && state_doc(&t.state).is_some()
        }) {
            let pal = *self.pal();
            let lang = self.lang();
            let em = self.settings.ui_font_size as f32;
            let tab_id = self.tabs[self.active].id;
            let action =
                find_bar::show_popup(&ctx, tab_id, &mut self.tabs[self.active].find, &pal, lang, em);
            match action {
                Some(find_bar::Action::Next) => self.find_step(&ctx, false, false),
                Some(find_bar::Action::Prev) => self.find_step(&ctx, true, false),
                Some(find_bar::Action::Close) => {
                    self.tabs[self.active].find.close();
                }
                None => {}
            }
        }

        // Сессия (вкладки/позиции чтения/геометрия окна) — тем же дебаунсом,
        // что и настройки: снимок каждого кадра, запись при изменении.
        let snap = self.session_snapshot();
        if snap != self.settings.session {
            self.settings.session = snap;
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

    /// Выход из приложения: сессия обязана попасть на диск синхронно —
    /// дебаунс сохранения мог не успеть сработать до закрытия окна.
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.persist_session();
    }
}

/// Действия из таб-бара (применяются после отрисовки, чтобы не мучиться с заимствованиями).
enum TabAction {
    Select(usize),
    Close(usize),
    NewTab,
    ToggleSettings,
    /// Правый клик по вкладке: контекст-меню в позиции курсора.
    TabMenu { pos: Pos2, idx: usize },
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
            // Правый клик по вкладке — контекст-меню файла (проводник/копия).
            if (resp.secondary_clicked() || x_resp.secondary_clicked())
                && let Some(p) = ui.input(|i| i.pointer.interact_pos())
            {
                actions.push(TabAction::TabMenu { pos: p, idx: i });
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
            TabAction::TabMenu { pos, idx } => {
                app.ctx_menu = None; // открыто одно меню
                app.tab_menu = Some(TabMenu { pos, idx });
            }
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

/// Статус-бар: слева — символы/строки/объём файла + живая статистика
/// выделения, справа — тумблеры панелей и «по ширине окна».
fn status_bar(ui: &mut Ui, app: &mut App) {
    let pal = *app.pal();
    let lang = app.lang();
    let em = app.settings.ui_font_size as f32 * 0.85;
    let h = 26.0;
    let sel: Option<SelStats> = app
        .tabs
        .get(app.active)
        .and_then(|t| t.selection.stats);

    // Статистика активной вкладки (для загрузки — «чтение…»).
    let stats = match app.tabs.get(app.active).map(|t| &t.state) {
        Some(TabState::Ready(doc)) => Some(format!(
            "{} {} · {} · {} {} · {} {} · {}",
            fmt_num(doc.word_count),
            t(lang, "sb_words"),
            reading_time(doc.word_count, lang),
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

        // Тумблеры справа: оглавление · миникарта · «по ширине окна».
        // Состояние — текстом («вкл/выкл»), без смены цвета плашки.
        // Правый край — с запасом от угла ресайза окна.
        const GAP: f32 = 8.0;
        let states = [
            app.toc_visible(),
            app.minimap_visible(ui.ctx()),
            app.settings.fit_width,
        ];
        let labels: Vec<String> = [
            t(lang, "st_outline"),
            t(lang, "st_minimap"),
            t(lang, "st_fit_width"),
        ]
        .iter()
        .zip(states)
        .map(|(l, on)| format!("{l} {}", t(lang, if on { "sb_on" } else { "sb_off" })))
        .collect();
        let widths: Vec<f32> = labels
            .iter()
            .map(|l| crate::view::widgets::plain_galley(ui, l, em, pal.fg).size().x + 18.0)
            .collect();
        let total: f32 = widths.iter().sum::<f32>() + GAP * 2.0;
        let lead = (ui.available_width() - total - 25.0).max(0.0);
        ui.add_space(lead);

        let mut clicked: Option<usize> = None;
        for (i, label) in labels.iter().enumerate() {
            if i > 0 {
                ui.add_space(GAP);
            }
            if status_toggle(ui, i, label, em, &pal, h) {
                clicked = Some(i);
            }
        }
        match clicked {
            Some(0) => app.toggle_toc(),
            Some(1) => app.toggle_minimap(ui.ctx()),
            Some(2) => app.settings.fit_width = !app.settings.fit_width,
            _ => {}
        }
    });

    // Верхняя граница бара.
    let r = ui.min_rect();
    ui.painter().line_segment(
        [pos2(r.min.x, r.min.y), pos2(r.max.x, r.min.y)],
        egui::Stroke::new(1.0, pal.border),
    );
}

/// Плоская ручка растяжения панели навигации: линия на краю, обращённом к
/// документу; перетаскивание меняет ширину (хранится в settings.json).
fn resize_handle(
    ui: &mut Ui,
    salt: &'static str,
    // Панель прижата к левому краю окна — тянем её правую границу.
    panel_on_left: bool,
    bounds: Rect,
    pal: &themes::Palette,
    width: &mut f32,
    range: (f32, f32),
) {
    let edge = if panel_on_left { bounds.max.x } else { bounds.min.x };
    // Хит-зона 10px вокруг линии — заходит и на документ, чтобы легко схватить.
    let hit = Rect::from_min_max(
        pos2(edge - 5.0, bounds.min.y),
        pos2(edge + 5.0, bounds.max.y),
    );
    let resp = ui.interact(hit, ui.scope_id().with(salt), Sense::click_and_drag());
    if resp.dragged() {
        let dx = resp.drag_delta().x;
        let d = if panel_on_left { dx } else { -dx };
        *width = (*width + d).clamp(range.0, range.1);
        ui.ctx().request_repaint();
    }
    let active = resp.hovered() || resp.dragged();
    // Линию — на полпикселя ВНУТРИ панели: ровно на границе её срезал клип.
    let line_x = if panel_on_left { edge - 0.5 } else { edge + 0.5 };
    ui.painter().line_segment(
        [pos2(line_x, bounds.min.y), pos2(line_x, bounds.max.y)],
        egui::Stroke::new(1.0, if active { pal.muted } else { pal.border }),
    );
    if active {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
}

/// Плоский тумблер статус-бара: рамка без заливки, состояние — текстом
/// («вкл»/«выкл» в подписи). Возвращает true в кадр клика.
fn status_toggle(
    ui: &mut Ui,
    salt: usize,
    label: &str,
    em: f32,
    pal: &themes::Palette,
    row_h: f32,
) -> bool {
    let g = crate::view::widgets::plain_galley(ui, label, em, pal.fg);
    let w = g.size().x + 18.0;
    let (r, _) = ui.allocate_exact_size(vec2(w, row_h - 8.0), Sense::hover());
    let resp = ui.interact(r, ui.scope_id().with(("sb_toggle", salt)), Sense::click());
    let hovered = resp.hovered();
    let painter = ui.painter().clone();
    let border = if hovered { pal.muted } else { pal.border };
    painter.rect(
        r,
        egui::CornerRadius::same(0),
        egui::Color32::TRANSPARENT,
        egui::Stroke::new(1.0, border),
        egui::StrokeKind::Middle,
    );
    let color = if hovered { pal.fg } else { pal.muted };
    // painter.galley игнорирует цвет для «цветных» галлеев — только override.
    painter.galley_with_override_text_color(r.center() - g.size() * 0.5, g, color);
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.clicked()
}

/// Плоский пункт контекст-меню: строка с hover-подсветкой; неактивный —
/// приглушён и не кликается. Возвращает true в кадр клика.
fn ctx_menu_item(
    ui: &mut Ui,
    salt: (&'static str, usize),
    label: &str,
    enabled: bool,
    em: f32,
    pal: &themes::Palette,
) -> bool {
    let g = crate::view::widgets::plain_galley(ui, label, em, pal.fg);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), em * 1.7), Sense::hover());
    let resp = ui.interact(
        rect,
        ui.scope_id().with(salt),
        if enabled { Sense::click() } else { Sense::hover() },
    );
    let painter = ui.painter().clone();
    let hovered = enabled && resp.hovered();
    if hovered {
        painter.rect_filled(rect, egui::CornerRadius::same(0), pal.inline_code_bg);
    }
    let color = if !enabled {
        pal.faint
    } else if hovered {
        pal.fg
    } else {
        pal.muted
    };
    painter.galley_with_override_text_color(
        pos2(rect.min.x + 10.0, rect.center().y - g.size().y * 0.5),
        g,
        color,
    );
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    enabled && resp.clicked()
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

/// Оценка времени чтения: слова / 220 сл-мин («чтение: N мин»).
fn reading_time(words: usize, lang: Lang) -> String {
    let mins = ((words as f64) / READ_WPM as f64).ceil() as usize;
    t(lang, "read_min").replace("{}", &fmt_num(mins))
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
