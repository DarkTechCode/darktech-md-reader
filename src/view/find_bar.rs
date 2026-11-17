//! Попап поиска (Ctrl+F): маленькое плавающее окно сверху справа — поле,
//! счётчик «3/17», кнопки ↑/↓/✕. Двигается за ручку-грип, закрывается Esc/✕.
//! Order::Foreground поверх документа — место не ест. Поле ввода СВОЁ:
//! TextEdit egui не используется — его кэшированный галлей «растворяется»
//! после пересборки атласа глифов (кэш галлеев не чистится, грабля egui
//! 0.37), поэтому текст/каретку рисуем сами, а ввод (печать, Backspace,
//! стрелки, вставка) обрабатывает app.rs.

use std::time::Duration;

use eframe::egui::text::CCursor;
use eframe::egui::{
    self, pos2, vec2, Context, CornerRadius, Rect, Sense, Stroke, StrokeKind, Ui, Vec2,
};

use crate::i18n::{t, Lang};

use super::find::{char_to_byte, FindState};
use super::themes::Palette;

pub enum Action {
    Next,
    Prev,
    Close,
}

const FIELD_W: f32 = 320.0;
const FIELD_H: f32 = 30.0;
const COUNTER_W: f32 = 64.0;
const BTN: f32 = 26.0;
const GRIP_W: f32 = 20.0;
/// Мигание каретки: сколько секунд из периода она видна.
const BLINK_ON: f64 = 1.2;
const BLINK_PERIOD: f64 = 2.0;

/// Показ попапа; возвращает действие кнопок. Позиция живёт в FindState
/// (перетаскивание своё — у egui fixed_pos перебивает сохранённое).
pub fn show_popup(
    ctx: &Context,
    tab_id: u64,
    find: &mut FindState,
    pal: &Palette,
    lang: Lang,
    em: f32,
) -> Option<Action> {
    let screen = ctx.viewport_rect();
    // Шрифт поля — как заголовки вкладок (16 × масштаб UI-шрифта).
    let field_font = 16.0 * em / 14.0;
    // Оценка ширины — для позиции по умолчанию (правый верх, под таб-баром).
    let est_w = GRIP_W + FIELD_W + COUNTER_W + BTN * 3.0 + 6.0 * 4.0 + 12.0;
    let default_pos = pos2(screen.max.x - est_w - 14.0, screen.min.y + 46.0);
    let mut pos = find.pos.unwrap_or(default_pos);

    let mut action: Option<Action> = None;
    let mut grip_delta = Vec2::ZERO;

    let out = egui::Area::new(egui::Id::unique(("find_popup", tab_id)))
        .order(egui::Order::Foreground)
        .interactable(true)
        .fixed_pos(pos)
        .show(ctx, |ui| {
            egui::Frame::NONE
                .fill(pal.code_bg)
                .stroke(Stroke::new(1.0, pal.border))
                .inner_margin(egui::Margin::symmetric(5, 4))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        grip_delta = grip_button(ui, pal);
                        action = field_and_controls(ui, find, pal, lang, field_font);
                    });
                });
        });
    // Прямоугольник попапа — документ глушит выделение под ним.
    find.popup_rect = Some(out.response.rect);

    // Двигаем за грип; держим целиком на экране (размер — по оценке).
    pos += grip_delta;
    let w = est_w.max(0.0);
    pos.x = pos.x.clamp(screen.min.x + 2.0, (screen.max.x - w - 2.0).max(screen.min.x + 2.0));
    pos.y = pos.y.clamp(screen.min.y + 2.0, (screen.max.y - 42.0).max(screen.min.y + 2.0));
    find.pos = Some(pos);

    action
}

/// Ручка-грип «⠿»: перетаскивание двигает попап. Возвращает сдвиг за кадр.
fn grip_button(ui: &mut Ui, pal: &Palette) -> Vec2 {
    let (rect, _) = ui.allocate_exact_size(vec2(GRIP_W, 26.0), Sense::hover());
    let resp = ui.interact(rect, ui.scope_id().with("find_grip"), Sense::drag());
    let hovered = resp.hovered();
    let color = if resp.dragged() {
        pal.link
    } else if hovered {
        pal.fg
    } else {
        pal.faint
    };
    let painter = ui.painter().clone();
    // Шесть точек в два столбца.
    for row in 0..3 {
        for col in 0..2 {
            let p = pos2(
                rect.min.x + 4.0 + col as f32 * 7.0,
                rect.min.y + 5.0 + row as f32 * 8.0,
            );
            painter.circle_filled(p, 1.6, color);
        }
    }
    if hovered || resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    if resp.dragged() {
        resp.drag_delta()
    } else {
        Vec2::ZERO
    }
}

/// Интервал двойного/тройного клика по полю.
const DOUBLE_CLICK_MS: u64 = 500;
/// Смещение клика, до которого серия «в том же месте», px.
const CLICK_SAME_PX: f32 = 6.0;

/// Своё поле ввода: текст/подсказка/выделение/каретка — наши; клик ставит
/// каретку, драг и двойной/тройной клик выделяют (слово/всё), счётчик и
/// кнопки ↑/↓/✕ рядом.
fn field_and_controls(
    ui: &mut Ui,
    find: &mut FindState,
    pal: &Palette,
    lang: Lang,
    font_size: f32,
) -> Option<Action> {
    // Поле: рамка/фон/текст — наши; galley — для хит-теста каретки.
    let empty = find.query.is_empty();
    let text_color = if empty { pal.faint } else { pal.fg };
    let shown: &str = if empty {
        t(lang, "find_placeholder")
    } else {
        find.query.as_str()
    };
    let g = super::widgets::plain_galley(ui, shown, font_size, text_color);
    let (rect, _) = ui.allocate_exact_size(vec2(FIELD_W, FIELD_H), Sense::hover());
    let resp = ui.interact(
        rect,
        ui.scope_id().with("find_field"),
        Sense::click_and_drag(),
    );
    let painter = ui.painter().clone();
    painter.rect(
        rect,
        CornerRadius::same(0),
        pal.bg,
        Stroke::new(1.0, pal.border),
        StrokeKind::Middle,
    );
    let text_pos = pos2(rect.min.x + 8.0, rect.center().y - g.size().y * 0.5);

    // Позиция символа под курсором (в пустое поле — 0).
    let hit_ch = |p: egui::Pos2| -> usize {
        g.cursor_from_pos(p - text_pos)
            .index
            .0
            .min(find.query.chars().count())
    };

    // Клик (без движения, срабатывает на отпускании): каретка и снятие
    // выделения; серия 2/3 — слово/весь запрос. Драг — отдельно: он не
    // порождает clicked(), а начался с якоря.
    if resp.clicked()
        && let Some(p) = resp.interact_pointer_pos()
    {
        let count = match find.clicks {
            Some((t, cp, n))
                if t.elapsed().as_millis() as u64 <= DOUBLE_CLICK_MS
                    && cp.distance(p) <= CLICK_SAME_PX
                    && n < 3 =>
            {
                n + 1
            }
            _ => 1,
        };
        find.clicks = Some((std::time::Instant::now(), p, count));
        let ch = hit_ch(p);
        match count {
            2 => {
                let (a, b) = field_word_range(&find.query, ch);
                find.sel_anchor = a;
                find.caret = b;
            }
            3 => {
                find.sel_anchor = 0;
                find.caret = find.query.chars().count();
            }
            _ => {
                find.sel_anchor = ch;
                find.caret = ch;
            }
        }
    }
    if resp.drag_started()
        && let Some(p) = resp.interact_pointer_pos()
    {
        find.clicks = None; // драг прерывает серию кликов
        let ch = hit_ch(p);
        find.sel_anchor = ch;
        find.caret = ch;
    } else if resp.dragged()
        && let Some(p) = resp.interact_pointer_pos()
    {
        find.caret = hit_ch(p);
    }

    // Выделение поля — под текстом.
    let (sa, sb) = find.field_sel_range();
    if sa < sb {
        let ra = g
            .pos_from_cursor(CCursor::new(sa))
            .translate(text_pos.to_vec2());
        let rb = g
            .pos_from_cursor(CCursor::new(sb))
            .translate(text_pos.to_vec2());
        painter.rect_filled(ra.union(rb), CornerRadius::same(0), pal.selection);
    }
    painter.galley(text_pos, g.clone(), text_color);

    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
    }

    // Каретка: мигает, рисуется внутри поля (клики/печать обновляют её).
    let now = ui.ctx().time();
    let on = (now % BLINK_PERIOD) < BLINK_ON;
    if on {
        let cr = g
            .pos_from_cursor(CCursor::new(find.caret))
            .translate(text_pos.to_vec2());
        painter.line_segment(
            [pos2(cr.min.x, rect.min.y + 5.0), pos2(cr.min.x, rect.max.y - 5.0)],
            Stroke::new(1.5, pal.fg),
        );
    }
    let left = (BLINK_PERIOD - (now % BLINK_PERIOD)).max(0.05);
    ui.ctx().request_repaint_after(Duration::from_secs_f64(left));

    // Счётчик «3/17» («…» — поиск ещё идёт, «+» — хранимые срезаны капом).
    let counter = counter_text(find);
    let color = if find.hits.is_empty() {
        pal.faint
    } else {
        pal.muted
    };
    let g = super::widgets::plain_galley(ui, &counter, font_size * 0.85, color);
    let (r, _) = ui.allocate_exact_size(vec2(COUNTER_W, g.size().y), Sense::hover());
    ui.painter()
        .galley(pos2(r.min.x, r.center().y - g.size().y * 0.5), g, color);

    let enabled = !find.hits.is_empty();
    let mut action: Option<Action> = None;
    if icon_button(ui, "find_prev", pal, enabled, |p, rect, c| {
        chevron(p, rect, c, true)
    }) {
        action = Some(Action::Prev);
    }
    if icon_button(ui, "find_next", pal, enabled, |p, rect, c| {
        chevron(p, rect, c, false)
    }) {
        action = Some(Action::Next);
    }
    if icon_button(ui, "find_close", pal, true, |p, rect, c| {
        let s = 5.0;
        let c0 = rect.center();
        p.line_segment(
            [pos2(c0.x - s, c0.y - s), pos2(c0.x + s, c0.y + s)],
            Stroke::new(1.8, c),
        );
        p.line_segment(
            [pos2(c0.x - s, c0.y + s), pos2(c0.x + s, c0.y - s)],
            Stroke::new(1.8, c),
        );
    }) {
        action = Some(Action::Close);
    }

    action
}

/// Текст счётчика: «3/17», «3/17…» (идёт), «3/100 000+» (кап хранимых),
/// «…» — результатов ещё нет, «0/0» — ничего не найдено.
fn counter_text(find: &FindState) -> String {
    if find.hits.is_empty() {
        return if find.searching {
            "…".to_string()
        } else {
            "0/0".to_string()
        };
    }
    let known = if find.searching {
        find.hits.len()
    } else if find.total > find.hits.len() {
        find.hits.len() // кап хранимых; «+» скажет, что есть ещё
    } else {
        find.total
    };
    let suffix = if find.searching {
        "…"
    } else if find.total > find.hits.len() {
        "+"
    } else {
        ""
    };
    format!("{}/{}{}", find.current + 1, known, suffix)
}

/// Плоская квадратная кнопка 26×26 с рисунком-замыканием; hover — заливка,
/// PointingHand; `enabled=false` — приглушена и не кликается.
fn icon_button(
    ui: &mut Ui,
    salt: &'static str,
    pal: &Palette,
    enabled: bool,
    draw: impl FnOnce(&egui::Painter, Rect, egui::Color32),
) -> bool {
    let (rect, _) = ui.allocate_exact_size(vec2(BTN, BTN), Sense::hover());
    let resp = ui.interact(
        rect,
        ui.scope_id().with(salt),
        if enabled { Sense::click() } else { Sense::hover() },
    );
    let hovered = enabled && resp.hovered();
    if hovered {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(0), pal.inline_code_bg);
    }
    let color = if !enabled {
        pal.faint
    } else if hovered {
        pal.fg
    } else {
        pal.muted
    };
    let painter = ui.painter().clone();
    draw(&painter, rect, color);
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    enabled && resp.clicked()
}

/// Шеврон ↑/↓ (глифы не во всех шрифтах — рисуем линиями).
fn chevron(p: &egui::Painter, rect: Rect, color: egui::Color32, up: bool) {
    let c = rect.center();
    let s = 5.0;
    let (tip_y, side_y) = if up { (c.y - s * 0.5, c.y + s * 0.5) } else { (c.y + s * 0.5, c.y - s * 0.5) };
    p.line_segment(
        [pos2(c.x - s, side_y), pos2(c.x, tip_y)],
        Stroke::new(1.8, color),
    );
    p.line_segment(
        [pos2(c.x + s, side_y), pos2(c.x, tip_y)],
        Stroke::new(1.8, color),
    );
}

// ---------------------------------------------------------------------------
// Редактирование запроса (вызывает app.rs при открытом попапе).
// ---------------------------------------------------------------------------

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Куда прыгнет каретка по Ctrl+стрелке/Backspace (граница слова).
fn word_jump(text: &str, caret: usize, left: bool) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut i = caret.min(n);
    if left {
        while i > 0 && !is_word(chars[i - 1]) {
            i -= 1;
        }
        while i > 0 && is_word(chars[i - 1]) {
            i -= 1;
        }
    } else {
        while i < n && !is_word(chars[i]) {
            i += 1;
        }
        while i < n && is_word(chars[i]) {
            i += 1;
        }
    }
    i
}

/// Слово вокруг позиции для двойного клика: прогоны одного класса
/// (пробелы / буквенно-цифровые с «_» / прочие знаки) — как в браузерах.
fn field_word_range(text: &str, pos: usize) -> (usize, usize) {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    if n == 0 {
        return (0, 0);
    }
    let cls = |i: usize| -> u8 {
        if chars[i].is_whitespace() {
            0
        } else if is_word(chars[i]) {
            1
        } else {
            2
        }
    };
    let p = pos.min(n);
    let class = cls(p.min(n - 1));
    let mut s = p;
    let mut e = p;
    while e < n && cls(e) == class {
        e += 1;
    }
    while s > 0 && cls(s - 1) == class {
        s -= 1;
    }
    (s, e.max(s))
}

impl FindState {
    /// Выделение поля по порядку (равны — выделения нет).
    pub fn field_sel_range(&self) -> (usize, usize) {
        (self.sel_anchor.min(self.caret), self.sel_anchor.max(self.caret))
    }

    /// Ctrl+A — выделить весь запрос.
    pub fn select_all_field(&mut self) {
        self.sel_anchor = 0;
        self.caret = self.query.chars().count();
    }

    /// Удалить выделение (если есть); caret — в начало бывшего выделения.
    fn delete_selection(&mut self) {
        let (a, b) = self.field_sel_range();
        if a < b {
            let b0 = char_to_byte(&self.query, a);
            let b1 = char_to_byte(&self.query, b);
            self.query.replace_range(b0..b1, "");
            self.caret = a;
            self.sel_anchor = a;
        }
    }

    /// Вставка в позицию каретки (печать, Ctrl+V); выделение заменяется.
    pub fn insert_text(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        self.delete_selection();
        let byte = char_to_byte(&self.query, self.caret);
        self.query.insert_str(byte, s);
        self.caret += s.chars().count();
        self.sel_anchor = self.caret;
    }

    /// Backspace (word — Ctrl+Backspace); выделение удаляется целиком.
    pub fn backspace(&mut self, word: bool) {
        if self.field_sel_range().0 < self.field_sel_range().1 {
            self.delete_selection();
            return;
        }
        if self.caret == 0 {
            return;
        }
        let to = if word {
            word_jump(&self.query, self.caret, true)
        } else {
            self.caret - 1
        };
        let b0 = char_to_byte(&self.query, to);
        let b1 = char_to_byte(&self.query, self.caret);
        self.query.replace_range(b0..b1, "");
        self.caret = to;
        self.sel_anchor = to;
    }

    /// Delete вперёд (word — Ctrl+Delete); выделение удаляется целиком.
    pub fn delete_ahead(&mut self, word: bool) {
        if self.field_sel_range().0 < self.field_sel_range().1 {
            self.delete_selection();
            return;
        }
        let n = self.query.chars().count();
        if self.caret >= n {
            return;
        }
        let to = if word {
            word_jump(&self.query, self.caret, false)
        } else {
            self.caret + 1
        };
        let b0 = char_to_byte(&self.query, self.caret);
        let b1 = char_to_byte(&self.query, to);
        self.query.replace_range(b0..b1, "");
        self.sel_anchor = self.caret;
    }

    /// Влево: с выделением — схлопнуть к началу, иначе на символ/слово.
    pub fn caret_left(&mut self, word: bool) {
        let (a, b) = self.field_sel_range();
        if a < b {
            self.caret = a;
            self.sel_anchor = a;
            return;
        }
        self.caret = if word {
            word_jump(&self.query, self.caret, true)
        } else {
            self.caret.saturating_sub(1)
        };
        self.sel_anchor = self.caret;
    }

    pub fn caret_right(&mut self, word: bool) {
        let (a, b) = self.field_sel_range();
        if a < b {
            self.caret = b;
            self.sel_anchor = b;
            return;
        }
        let n = self.query.chars().count();
        self.caret = if word {
            word_jump(&self.query, self.caret, false)
        } else {
            (self.caret + 1).min(n)
        };
        self.sel_anchor = self.caret;
    }

    pub fn caret_home(&mut self) {
        self.caret = 0;
        self.sel_anchor = 0;
    }

    pub fn caret_end(&mut self) {
        self.caret = self.query.chars().count();
        self.sel_anchor = self.caret;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_backspace_and_caret_moves() {
        let mut f = FindState::default();
        f.insert_text("при");
        f.insert_text("вет");
        assert_eq!(f.query, "привет");
        assert_eq!(f.caret, 6);
        f.caret_home();
        f.caret_right(false);
        f.caret_right(true); // до конца слова
        assert_eq!(f.caret, 6);
        f.backspace(true); // Ctrl+Backspace — всё слово
        assert_eq!(f.query, "");
        assert_eq!(f.caret, 0);
    }

    #[test]
    fn insert_in_middle_and_delete_ahead() {
        let mut f = FindState::default();
        f.insert_text("ab cd");
        f.caret_home();
        f.insert_text("X"); // Xab cd
        assert_eq!(f.query, "Xab cd");
        f.caret_end();
        f.backspace(false); // последний символ
        assert_eq!(f.query, "Xab c");
        f.caret_home();
        f.delete_ahead(true); // слово целиком
        assert_eq!(f.query, " c");
    }

    #[test]
    fn word_jump_over_punct() {
        assert_eq!(word_jump("foo, bar", 8, true), 5); // к началу «bar»
        assert_eq!(word_jump("foo, bar", 3, false), 8); // за конец «bar»
        assert_eq!(word_jump("foo, бар!", 4, true), 0); // к началу «foo»
    }

    #[test]
    fn field_selection_editing() {
        let mut f = FindState::default();
        f.insert_text("абв где");
        f.select_all_field();
        assert_eq!(f.field_sel_range(), (0, 7));
        f.insert_text("x"); // выделение заменяется вводом
        assert_eq!(f.query, "x");
        f.insert_text(" y");
        assert_eq!(f.query, "x y");
        f.sel_anchor = 0;
        f.caret = 1; // выделен «x»
        f.backspace(false);
        assert_eq!(f.query, " y");
        assert_eq!(f.caret, 0);
        // двойной клик по середине слова
        assert_eq!(field_word_range("foo, bar", 6), (5, 8));
        assert_eq!(field_word_range("foo, bar", 3), (3, 4)); // знак
    }
}
