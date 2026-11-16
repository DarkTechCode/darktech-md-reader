//! Состояние приложения: вкладки, ввод (CLI/drag-drop/Ctrl+O), панели.

use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui::{
    self, pos2, vec2, Context, FontFamily, FontId, Frame, Key, Modifiers, Rect, Sense, Ui,
    ViewportCommand,
};
use eframe::egui::epaint::text::VariationCoords;
use eframe::egui::text::{ByteIndex, Galley, LayoutJob, LayoutSection, TextFormat};

use crate::i18n::{system_lang, t, Lang};
use crate::md::blocks::Doc;
use crate::md::parser;
use crate::view::widgets::flat_text_button;
use crate::view::{doc_view, themes, welcome, Env};
use crate::fonts;

pub struct App {
    tabs: Vec<Tab>,
    active: usize,
    lang: Lang,
    typo: themes::Typo,
    dark: bool,
    /// Последний выставленный заголовок окна (чтобы не спамить командой).
    title_sent: String,
}

pub struct Tab {
    pub path: Option<PathBuf>,
    pub doc: Option<Arc<Doc>>,
    pub error: Option<String>,
}

impl Tab {
    fn welcome() -> Self {
        Self {
            path: None,
            doc: None,
            error: None,
        }
    }

    fn load(path: PathBuf) -> Self {
        match read_text(&path) {
            Ok(text) => Self {
                doc: Some(Arc::new(parser::parse(Arc::from(text.as_str())))),
                path: Some(path),
                error: None,
            },
            Err(e) => Self {
                doc: None,
                path: Some(path),
                error: Some(e),
            },
        }
    }

    fn title(&self, lang: Lang) -> String {
        match &self.path {
            Some(p) => p
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.to_string_lossy().into_owned()),
            None => t(lang, "welcome_title").to_string(),
        }
    }
}

/// Читает файл как UTF-8 (с BOM); CRLF/LF не важны.
fn read_text(path: &std::path::Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let mut text = String::from_utf8(bytes).map_err(|_| format!("{}: не UTF-8", path.display()))?;
    if text.starts_with('\u{FEFF}') {
        text.replace_range(0..3, "");
    }
    Ok(text)
}

impl App {
    pub fn new(cc: &eframe::CreationContext, paths: Vec<PathBuf>) -> Self {
        fonts::install(&cc.egui_ctx);
        let lang = system_lang();
        let dark = true;
        themes::apply(
            &cc.egui_ctx,
            if dark { &themes::DARK } else { &themes::LIGHT },
            dark,
        );

        let mut tabs: Vec<Tab> = paths.into_iter().map(Tab::load).collect();
        if tabs.is_empty() {
            tabs.push(Tab::welcome());
        }

        Self {
            tabs,
            active: 0,
            lang,
            typo: themes::Typo::default(),
            dark,
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

    /// Открывает файл новой вкладкой (или заменяет единственную welcome-вкладку).
    fn open(&mut self, path: PathBuf) {
        let replace_welcome = self.tabs.len() == 1 && self.tabs[0].path.is_none();
        let tab = Tab::load(path);
        let has_content = tab.doc.is_some() || tab.error.is_some();
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
                self.tabs.push(Tab::welcome());
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
        self.handle_input(&ctx);

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
        egui::CentralPanel::default()
            .frame(Frame::NONE.fill(pal.bg))
            .show(ui, |ui| {
                let Some(tab) = self.tabs.get(self.active) else {
                    return;
                };
                if let Some(err) = &tab.error {
                    show_error(ui, tab, err, env.lang);
                } else if let Some(doc) = &tab.doc {
                    for ev in doc_view::show(ui, doc, &env) {
                        match ev {
                            doc_view::DocEvent::OpenLink(url) => link_events.push(url),
                        }
                    }
                } else if welcome::show(ui, env.lang) {
                    if let Some(path) = pick_md_file() {
                        self.open(path);
                    }
                }
            });
        for url in link_events {
            self.open_link(&url, &ctx);
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
fn tab_strip(ui: &mut Ui, app: &mut App) {
    let pal = *app.pal();
    let h = 46.0;
    let mut actions: Vec<TabAction> = Vec::new();

    ui.horizontal(|ui| {
        ui.add_space(4.0);
        let titles: Vec<String> = app.tabs.iter().map(|tab| tab.title(app.lang)).collect();
        let active = app.active;
        for (i, title) in titles.iter().enumerate() {
            let is_active = i == active;
            let g = title_galley(ui, title, is_active, &pal, 24.0);
            let close_w = 32.0;
            let w = g.size().x + 14.0 * 2.0 + close_w;
            let (rect, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click());
            let painter = ui.painter();
            if is_active {
                painter.rect_filled(rect, egui::CornerRadius::same(0), pal.bg);
            } else if resp.hovered() {
                painter.rect_filled(
                    rect,
                    egui::CornerRadius::same(0),
                    themes::lighten(pal.code_bg, 0.35),
                );
            }
            painter.galley(
                pos2(rect.min.x + 14.0, rect.center().y - g.size().y * 0.5),
                g,
                if is_active { pal.fg } else { pal.muted },
            );

            // Крестик закрытия — отдельная зона клика.
            let xg = title_galley(ui, "×", false, &pal, 22.0);
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
        if flat_text_button(ui, "+", 24.0, pal.muted, pal.fg) {
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
) -> std::sync::Arc<Galley> {
    let mut job = LayoutJob::default();
    let mut fmt = TextFormat {
        font_id: FontId::new(size, FontFamily::Proportional),
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

fn show_error(ui: &mut Ui, tab: &Tab, err: &str, lang: Lang) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() * 0.3);
        ui.label(egui::RichText::new(t(lang, "err_read")).size(20.0).strong());
        ui.add_space(8.0);
        let path = tab
            .path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        ui.label(egui::RichText::new(path).weak());
        ui.label(egui::RichText::new(err).weak().color(egui::Color32::LIGHT_RED));
    });
}
