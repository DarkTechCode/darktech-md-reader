//! Состояние приложения: вкладки, ввод (CLI/drag-drop/Ctrl+O), панели.
//! Большие файлы грузятся фоном: UI показывает блоки по мере разбора.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::Receiver;

use eframe::egui::{
    self, pos2, vec2, Context, FontFamily, Frame, Key, Modifiers, Rect, Sense, Ui,
    ViewportCommand,
};
use eframe::egui::epaint::text::VariationCoords;
use eframe::egui::text::{ByteIndex, Galley, LayoutJob, LayoutSection, TextFormat};

use crate::i18n::{system_lang, t, Lang};
use crate::layout::LayoutState;
use crate::md::blocks::Doc;
use crate::md::parser::{self, ParseMsg};
use crate::view::widgets::flat_text_button;
use crate::view::{doc_view, themes, welcome, Env};
use crate::fonts;

pub struct App {
    tabs: Vec<Tab>,
    active: usize,
    lang: Lang,
    typo: themes::Typo,
    dark: bool,
    /// Счётчик id вкладок (стабильные id скроллов/блоков на переключение вкладок).
    next_tab_id: u64,
    /// Последний выставленный заголовок окна (чтобы не спамить командой).
    title_sent: String,
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
            ParseMsg::Done { word_count, source } => {
                res = Pumped::Done { word_count, source }
            }
            ParseMsg::Failed(e) => res = Pumped::Failed(e),
        }
    }
    res
}

impl App {
    pub fn new(cc: &eframe::CreationContext, paths: Vec<PathBuf>) -> Self {
        fonts::install(&cc.egui_ctx);
        let lang = system_lang();
        let dark = true;
        // Фиксируем тёмную тему egui: без этого при системной светлой теме
        // наш стиль попадает в Dark, а рендерится дефолтный Light.
        cc.egui_ctx.set_theme(egui::ThemePreference::Dark);
        themes::apply(
            &cc.egui_ctx,
            if dark { &themes::DARK } else { &themes::LIGHT },
            dark,
        );

        let mut next_id = 0;
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
            lang,
            typo: themes::Typo::default(),
            dark,
            next_tab_id: next_id + 1,
            title_sent: String::new(),
        }
    }

    fn pal(&self) -> &'static themes::Palette {
        if self.dark {
            &themes::DARK
        } else {
            &themes::LIGHT
        }
    }

    /// Прокачивает фоновые загрузки всех вкладок; true — пока есть живые.
    fn pump_tabs(&mut self, ctx: &Context) -> bool {
        let mut loading = false;
        for tab in &mut self.tabs {
            let mut finished: Option<TabState> = None;
            if let TabState::Loading { doc, rx } = &mut tab.state {
                match pump_rx(rx, doc) {
                    Pumped::Still => loading = true,
                    Pumped::Done { word_count, source } => {
                        doc.word_count = word_count;
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
            Some(tab) if tab.path.is_some() => format!("{} — DarkTechMD", tab.title(self.lang)),
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

fn pick_md_file() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter("Markdown", &["md", "markdown", "mdown"])
        .pick_file()
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // Стиль темы может сбрасываться (смена темы системой/eframe) —
        // тогда egui вернёт свои дефолты (серый скроллбар и пр.). Возвращаем наш.
        if ui.visuals().panel_fill != self.pal().bg {
            themes::apply(&ctx, self.pal(), self.dark);
        }

        self.handle_input(&ctx);
        self.pump_tabs(&ctx);

        egui::Panel::top("tab_strip")
            .resizable(false)
            .frame(Frame::NONE.fill(self.pal().code_bg))
            .show(ui, |ui| {
                tab_strip(ui, self);
            });

        let pal = self.pal();
        let typo = self.typo;
        let env = Env {
            pal,
            typo: &typo,
            lang: self.lang,
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
                        show_error(ui, path, err, env.lang);
                    }
                    TabState::Welcome => {
                        welcome_clicked = welcome::show(ui, env.lang);
                    }
                    TabState::Loading { doc, .. } => {
                        if doc.blocks.is_empty() {
                            show_loading(ui, env.lang);
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

        self.sync_title(&ctx);
    }
}

/// Действия из таб-бара (применяются после отрисовки, чтобы не мучиться с заимствованиями).
enum TabAction {
    Select(usize),
    Close(usize),
    NewTab,
}

/// Плоские вкладки без скруглений: активная подсвечена заливкой фона читалки.
/// Текст — размера ссылок в документе, имя файла с расширением.
fn tab_strip(ui: &mut Ui, app: &mut App) {
    let pal = *app.pal();
    let h = 40.0;
    let text_size = 18.0;
    let mut actions: Vec<TabAction> = Vec::new();

    ui.horizontal(|ui| {
        ui.add_space(4.0);
        let titles: Vec<String> = app.tabs.iter().map(|tab| tab.title(app.lang)).collect();
        let active = app.active;
        for (i, title) in titles.iter().enumerate() {
            let is_active = i == active;
            let g = title_galley(ui, title, is_active, &pal, text_size);
            let close_w = 28.0;
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
                painter.rect_filled(
                    rect,
                    egui::CornerRadius::same(0),
                    themes::lighten(pal.code_bg, 0.35),
                );
            }
            painter.galley(
                pos2(rect.min.x + 12.0, rect.center().y - g.size().y * 0.5),
                g,
                if is_active { pal.fg } else { pal.muted },
            );

            // Крестик закрытия — отдельная зона клика.
            let xg = title_galley(ui, "×", false, &pal, 16.0);
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
            painter.galley(x_min, xg, if x_resp.hovered() { pal.fg } else { pal.faint });
            if resp.hovered() || x_resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }

            if x_resp.clicked() {
                actions.push(TabAction::Close(i));
            } else if resp.clicked() {
                actions.push(TabAction::Select(i));
            }
        }
        ui.add_space(4.0);
        if flat_text_button(ui, "+", text_size, pal.muted, pal.fg) {
            actions.push(TabAction::NewTab);
        }
    });

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
        }
    }
}

fn title_galley(
    ui: &Ui,
    text: &str,
    bold: bool,
    pal: &themes::Palette,
    size: f32,
) -> Arc<Galley> {
    let mut job = LayoutJob::default();
    let mut fmt = TextFormat {
        font_id: eframe::egui::FontId::new(size, FontFamily::Proportional),
        color: if bold { pal.fg } else { pal.muted },
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

fn show_loading(ui: &mut Ui, lang: Lang) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() * 0.3);
        ui.label(egui::RichText::new(t(lang, "loading")).size(20.0).weak());
    });
}

fn show_error(ui: &mut Ui, path: &Option<PathBuf>, err: &str, lang: Lang) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() * 0.3);
        ui.label(egui::RichText::new(t(lang, "err_read")).size(20.0).strong());
        ui.add_space(8.0);
        let path = path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        ui.label(egui::RichText::new(path).weak());
        ui.label(egui::RichText::new(err).weak().color(egui::Color32::LIGHT_RED));
    });
}
