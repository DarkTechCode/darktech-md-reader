//! Панель настроек чтения: строки «подпись + сегментированные плашки /
//! слайдер» (композиция MarkMello, стили — плоские, без скруглений).
//! Правки применяются сразу; App замечает изменение SettingsData и
//! перезаписывает settings.json с дебаунсом.

use std::sync::Arc;

use eframe::egui::text::Galley;
use eframe::egui::{pos2, vec2, CornerRadius, Rect, Sense, Stroke, Ui};

use crate::i18n::{t, Lang};
use crate::settings::model::{
    SettingsData, LangMode, PanelMode, Side, TextAlign, ThemeMode, FONT_MAX, FONT_MIN, LETTER_MAX,
    LETTER_MIN, LETTER_STEP, LINE_HEIGHT_MAX, LINE_HEIGHT_MIN, LINE_HEIGHT_STEP, TOC_LEVEL_MAX,
    TOC_LEVEL_MIN, UI_FONT_MAX, UI_FONT_MIN, WIDTH_MAX, WIDTH_MIN, WIDTH_PRESETS,
};
use crate::view::themes::{lighten, FontChoice, Palette};
use crate::view::widgets::{plain_galley, wrapped_galley};

/// Горизонтальный отступ содержимого от краёв панели.
const PAD: f32 = 16.0;
/// Шаг произвольной ширины текста (слайдер), px.
const WIDTH_SLIDER_STEP: f32 = 20.0;

/// Рисует панель; возвращает true, если запрошен перезапуск приложения.
pub fn show(ui: &mut Ui, s: &mut SettingsData, pal: &Palette, lang: Lang) -> bool {
    let em = s.ui_font_size as f32;

    // Заголовок.
    ui.add_space(14.0);
    let title = plain_galley(ui, t(lang, "settings_title"), em * 1.12, pal.fg);
    alloc_row(ui, em * 1.7, |ui, row| {
        ui.painter().galley(
            pos2(row.min.x + PAD, row.center().y - title.size().y * 0.5),
            title,
            pal.fg,
        );
    });
    ui.add_space(6.0);
    hline(ui, pal);
    ui.add_space(12.0);

    pills_row(
        ui,
        "theme",
        t(lang, "st_theme"),
        em,
        pal,
        &[
            (t(lang, "st_light"), ThemeMode::Light),
            (t(lang, "st_dark"), ThemeMode::Dark),
            (t(lang, "st_auto"), ThemeMode::System),
        ],
        &mut s.theme,
    );

    pills_row(
        ui,
        "lang",
        t(lang, "st_lang"),
        em,
        pal,
        &[
            (t(lang, "st_lang_ru"), LangMode::Ru),
            (t(lang, "st_lang_en"), LangMode::En),
            (t(lang, "st_auto"), LangMode::System),
        ],
        &mut s.language,
    );

    pills_row(
        ui,
        "font",
        t(lang, "st_font"),
        em,
        pal,
        &[
            ("Serif", FontChoice::Serif),
            ("Sans", FontChoice::Sans),
            ("Mono", FontChoice::Mono),
        ],
        &mut s.font,
    );

    let mut v = s.font_size as f32;
    slider_row(
        ui,
        "font_size",
        t(lang, "st_font_size"),
        em,
        pal,
        FONT_MIN as f32,
        FONT_MAX as f32,
        1.0,
        &mut v,
        0,
    );
    s.font_size = v as u8;

    let mut v = s.line_height;
    slider_row(
        ui,
        "line_height",
        t(lang, "st_line_height"),
        em,
        pal,
        LINE_HEIGHT_MIN,
        LINE_HEIGHT_MAX,
        LINE_HEIGHT_STEP,
        &mut v,
        2,
    );
    s.line_height = v;

    // Ширина: пресеты-плашки + произвольная (слайдер ниже). Пока включено
    // «по ширине окна», ни один пресет не активен (NAN не совпадает ни с чем);
    // ручной выбор ширины выключает растягивание.
    let before_w = s.text_width;
    let mut w = s.text_width as f32;
    let labels = [t(lang, "st_narrow"), t(lang, "st_medium"), t(lang, "st_wide")];
    let items: [(&str, f32); 3] = [
        (labels[0], WIDTH_PRESETS[0] as f32),
        (labels[1], WIDTH_PRESETS[1] as f32),
        (labels[2], WIDTH_PRESETS[2] as f32),
    ];
    let mut cur = if s.fit_width { f32::NAN } else { w };
    pills_row(ui, "width", t(lang, "st_text_width"), em, pal, &items, &mut cur);
    if !cur.is_nan() {
        w = cur;
    }
    slider_row(
        ui,
        "width",
        "",
        em,
        pal,
        WIDTH_MIN as f32,
        WIDTH_MAX as f32,
        WIDTH_SLIDER_STEP,
        &mut w,
        0,
    );
    s.text_width = w as u16;
    if s.text_width != before_w {
        s.fit_width = false;
    }

    // Выравнивание текста (абзацы/заголовки/цитаты/списки).
    pills_row(
        ui,
        "text_align",
        t(lang, "st_align"),
        em,
        pal,
        &[
            (t(lang, "st_align_left"), TextAlign::Left),
            (t(lang, "st_align_center"), TextAlign::Center),
            (t(lang, "st_align_justify"), TextAlign::Justify),
        ],
        &mut s.text_align,
    );

    let mut v = s.letter_spacing;
    slider_row(
        ui,
        "letter",
        t(lang, "st_letter_spacing"),
        em,
        pal,
        LETTER_MIN,
        LETTER_MAX,
        LETTER_STEP,
        &mut v,
        1,
    );
    s.letter_spacing = v;

    let mut v = s.ui_font_size as f32;
    slider_row(
        ui,
        "ui_font",
        t(lang, "st_ui_font"),
        em,
        pal,
        UI_FONT_MIN as f32,
        UI_FONT_MAX as f32,
        1.0,
        &mut v,
        0,
    );
    s.ui_font_size = v as u8;

    // Выравнивание ячеек таблиц: по центру / по верхнему краю.
    let mut top = s.table_top;
    pills_row(
        ui,
        "table_align",
        t(lang, "st_table_align"),
        em,
        pal,
        &[
            (t(lang, "st_cell_center"), false),
            (t(lang, "st_cell_top"), true),
        ],
        &mut top,
    );
    s.table_top = top;

    // Скролл в таблицах: выкл — широкие таблицы сжимаются в ширину колонки
    // текста, ячейки переносятся по словам (без горизонтальной прокрутки).
    let mut tscroll = s.table_scroll;
    pills_row(
        ui,
        "table_scroll",
        t(lang, "st_table_scroll"),
        em,
        pal,
        &[(t(lang, "st_off"), false), (t(lang, "st_on"), true)],
        &mut tscroll,
    );
    s.table_scroll = tscroll;

    // Нумерация строк в блоках кода.
    let mut nums = s.code_line_numbers;
    pills_row(
        ui,
        "code_nums",
        t(lang, "st_line_nums"),
        em,
        pal,
        &[(t(lang, "st_off"), false), (t(lang, "st_on"), true)],
        &mut nums,
    );
    s.code_line_numbers = nums;

    // Подсветка синтаксиса.
    let mut hl = s.code_highlight;
    pills_row(
        ui,
        "code_hl",
        t(lang, "st_highlight"),
        em,
        pal,
        &[(t(lang, "st_off"), false), (t(lang, "st_on"), true)],
        &mut hl,
    );
    s.code_highlight = hl;

    // Навигация: миникарта и оглавление (сторона/максимальный уровень).
    let mode_items = [
        (t(lang, "st_auto"), PanelMode::Auto),
        (t(lang, "st_on"), PanelMode::On),
        (t(lang, "st_off"), PanelMode::Off),
    ];
    pills_row(
        ui,
        "minimap",
        t(lang, "st_minimap"),
        em,
        pal,
        &mode_items,
        &mut s.minimap_mode,
    );

    pills_row(
        ui,
        "outline",
        t(lang, "st_outline"),
        em,
        pal,
        &mode_items,
        &mut s.toc_mode,
    );

    pills_row(
        ui,
        "outline_side",
        t(lang, "st_outline_side"),
        em,
        pal,
        &[
            (t(lang, "st_side_left"), Side::Left),
            (t(lang, "st_side_right"), Side::Right),
        ],
        &mut s.toc_side,
    );

    // H1..H6 — компактные плашки уровней.
    let levels: [(&str, u8); 6] = [
        ("H1", TOC_LEVEL_MIN),
        ("H2", 2),
        ("H3", 3),
        ("H4", 4),
        ("H5", 5),
        ("H6", TOC_LEVEL_MAX),
    ];
    pills_row(
        ui,
        "outline_level",
        t(lang, "st_outline_level"),
        em,
        pal,
        &levels,
        &mut s.toc_max_level,
    );

    // Ассоциация файлов (HKCU, без установщика — PLAN §9): состояние — в
    // реестре (читается каждый кадр, дёшево), клик по чекбоксу действует сразу.
    ui.add_space(8.0);
    assoc_row(ui, t(lang, "st_assoc"), em, pal, lang);

    // Сброс настроек + перезапуск приложения (владелец тестирует: новый
    // процесс открывает те же вкладки).
    ui.add_space(14.0);
    hline(ui, pal);
    ui.add_space(12.0);
    let mut restart = false;
    ui.horizontal(|ui| {
        ui.add_space(PAD - 6.0);
        if flat_bordered_button(ui, t(lang, "st_reset"), em, pal) {
            // Сбрасываем опции чтения; сессия (вкладки/последние файлы/
            // геометрия окна) — не настройка чтения, её сохраняем.
            let session = std::mem::take(&mut s.session);
            *s = SettingsData::default();
            s.session = session;
        }
        ui.add_space(10.0);
        if flat_bordered_button(ui, t(lang, "st_restart"), em, pal) {
            restart = true;
        }
    });

    // Подпись автора: сайт и GitHub открываются в браузере.
    ui.add_space(20.0);
    ui.add_space(8.0);
    let author = plain_galley(ui, &format!("{}: Dark Wizard", t(lang, "st_author")), em, pal.muted);
    alloc_row(ui, em * 1.5, |ui, row| {
        ui.painter().galley(
            pos2(row.min.x + PAD, row.center().y - author.size().y * 0.5),
            author,
            pal.muted,
        );
    });
    // Подпись-ссылки: метка («Email:», «Сайт:», «GitHub:») — обычный текст,
    // значение — ссылка (открывается в браузере).
    let links = [
        ("Email", "md@darktech.ru", "mailto:md@darktech.ru"),
        (t(lang, "st_site"), "https://darktech.ru", "https://darktech.ru"),
        (
            "GitHub",
            "https://github.com/DarkTechCode/darktech-md-reader/",
            "https://github.com/DarkTechCode/darktech-md-reader/",
        ),
    ];
    for (label, value, url) in links {
        let lg = plain_galley(ui, &format!("{label}: "), em, pal.muted);
        let vg = plain_galley(ui, value, em, pal.link);
        let label_w = lg.size().x;
        alloc_row(ui, em * 1.5, |ui, row| {
            let cy = row.center().y;
            ui.painter().galley(
                pos2(row.min.x + PAD, cy - lg.size().y * 0.5),
                lg,
                pal.muted,
            );
            let rect = Rect::from_min_size(
                pos2(row.min.x + PAD + label_w, cy - vg.size().y * 0.5),
                vg.size(),
            );
            let resp = ui.interact(
                rect,
                ui.scope_id().with(("author_link", url)),
                Sense::click(),
            );
            let hovered = resp.hovered();
            // painter.galley игнорирует цвет для «цветных» галлеев — только override.
            ui.painter().galley_with_override_text_color(
                rect.min,
                vg,
                if hovered { lighten(pal.link, 0.25) } else { pal.link },
            );
            if hovered {
                ui.ctx().set_cursor_icon(eframe::egui::CursorIcon::PointingHand);
            }
            if resp.clicked() {
                ui.ctx().open_url(eframe::egui::OpenUrl::new_tab(url));
            }
        });
    }
    // Нижний отступ: последняя строка не прилипает к краю окна при скролле.
    ui.add_space(20.0);
    restart
}

/// Кнопка с рамкой и паддингами («Сбросить настройки», «Перезапуск»):
/// при наведении — заливка.
fn flat_bordered_button(ui: &mut Ui, text: &str, em: f32, pal: &Palette) -> bool {
    let g = plain_galley(ui, text, em, pal.muted);
    let size = g.size() + vec2(em * 1.6, em * 0.9);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter().clone();
    let hovered = resp.hovered();
    let color = if hovered { pal.fg } else { pal.muted };
    let border = if hovered { pal.muted } else { pal.border };
    let fill = if hovered {
        lighten(pal.bg, 0.10)
    } else {
        eframe::egui::Color32::TRANSPARENT
    };
    painter.rect(
        rect,
        eframe::egui::CornerRadius::same(0),
        fill,
        Stroke::new(1.0, border),
        eframe::egui::StrokeKind::Middle,
    );
    painter.galley_with_override_text_color(rect.center() - g.size() * 0.5, g, color);
    if hovered {
        ui.ctx()
            .set_cursor_icon(eframe::egui::CursorIcon::PointingHand);
    }
    resp.clicked()
}

/// Строка «подпись слева, сегментированные плашки справа».
/// Активная плашка — инверсия (фон pal.fg, текст pal.bg); значение не из
/// списка — ни одна не подсвечена.
fn pills_row<T: Copy + PartialEq>(
    ui: &mut Ui,
    salt: &str,
    label: &str,
    em: f32,
    pal: &Palette,
    items: &[(&str, T)],
    value: &mut T,
) {
    let row_h = em + 14.0;
    let lg = plain_galley(ui, label, em, pal.muted);
    let gs: Vec<Arc<Galley>> = items
        .iter()
        .map(|(txt, _)| plain_galley(ui, txt, em, pal.fg))
        .collect();
    let pills_w: f32 = gs.iter().map(|g| g.size().x + 22.0).sum();
    // Плашки прижаты к правому краю строки; подпись — слева.

    alloc_row(ui, row_h, |ui, row| {
        let painter = ui.painter().clone();
        painter.galley(
            pos2(row.min.x + PAD, row.center().y - lg.size().y * 0.5),
            lg,
            pal.muted,
        );
        let mut x = row.max.x - PAD - pills_w;
        for (i, g) in gs.iter().enumerate() {
            let w = g.size().x + 22.0;
            let rect = Rect::from_min_size(pos2(x, row.min.y), vec2(w, row_h));
            x += w;
            let resp = ui.interact(
                rect,
                ui.scope_id().with(("pill", salt, i)),
                Sense::click(),
            );
            let active = items[i].1 == *value;
            let hovered = resp.hovered();
            if active {
                painter.rect_filled(rect, CornerRadius::same(0), pal.fg);
            } else if hovered {
                // inline_code_bg контрастен к code_bg в обеих темах.
                painter.rect_filled(rect, CornerRadius::same(0), pal.inline_code_bg);
            }
            let color = if active {
                pal.bg
            } else if hovered {
                pal.fg
            } else {
                pal.muted
            };
            // painter.galley игнорирует цвет для «цветных» галлеев — только override.
            painter.galley_with_override_text_color(rect.center() - g.size() * 0.5, g.clone(), color);
            if hovered && !active {
                ui.ctx().set_cursor_icon(eframe::egui::CursorIcon::PointingHand);
            }
            if resp.clicked() {
                *value = items[i].1;
            }
        }
    });
    ui.add_space(6.0);
}

/// Строка «подпись слева, чекбоксы .md/.txt справа»: ассоциация расширений
/// с DarkTechMD (HKCU). Состояние — системное (реестр), не в settings.json.
/// Если Windows назначила расширение другому приложению (UserChoice —
/// защищён, программно не сменить), под чекбоксами — подсказка и кнопка
/// открытия системных настроек.
fn assoc_row(ui: &mut Ui, label: &str, em: f32, pal: &Palette, lang: Lang) {
    let row_h = em + 14.0;
    let lg = plain_galley(ui, label, em, pal.muted);
    let labels: Vec<_> = crate::associate::EXTENSIONS
        .iter()
        .map(|e| plain_galley(ui, &format!(".{e}"), em, pal.fg))
        .collect();
    let box_s = em * 0.95;
    let item_w = |g: &Arc<Galley>| box_s + 6.0 + g.size().x;
    let gap = 18.0;
    let total: f32 = labels.iter().map(|g| item_w(g) + gap).sum::<f32>() - gap;

    alloc_row(ui, row_h, |ui, row| {
        let painter = ui.painter().clone();
        painter.galley(
            pos2(row.min.x + PAD, row.center().y - lg.size().y * 0.5),
            lg,
            pal.muted,
        );
        let mut x = row.max.x - PAD - total;
        for (i, g) in labels.iter().enumerate() {
            let ext = crate::associate::EXTENSIONS[i];
            let checked = crate::associate::is_associated(ext);
            let hit = Rect::from_min_size(pos2(x, row.min.y), vec2(item_w(g), row_h));
            let resp = ui.interact(
                hit,
                ui.scope_id().with(("assoc", ext)),
                Sense::click(),
            );
            let hovered = resp.hovered();

            // Чекбокс (плоский, как task list в документе) + подпись.
            let box_rect =
                Rect::from_min_size(pos2(x, row.center().y - box_s * 0.5), vec2(box_s, box_s));
            let stroke_color = if checked || hovered {
                pal.link
            } else {
                pal.muted
            };
            painter.rect(
                box_rect,
                CornerRadius::same(0),
                if checked { pal.link } else { eframe::egui::Color32::TRANSPARENT },
                Stroke::new(1.5, stroke_color),
                eframe::egui::StrokeKind::Middle,
            );
            if checked {
                let c = box_rect.center();
                let s = box_s * 0.28;
                painter.line_segment(
                    [pos2(c.x - s, c.y + s * 0.2), pos2(c.x - s * 0.25, c.y + s * 0.8)],
                    Stroke::new(2.0, eframe::egui::Color32::WHITE),
                );
                painter.line_segment(
                    [pos2(c.x - s * 0.25, c.y + s * 0.8), pos2(c.x + s, c.y - s * 0.6)],
                    Stroke::new(2.0, eframe::egui::Color32::WHITE),
                );
            }
            painter.galley_with_override_text_color(
                pos2(x + box_s + 6.0, row.center().y - g.size().y * 0.5),
                g.clone(),
                if hovered { pal.fg } else { pal.muted },
            );

            if hovered {
                ui.ctx()
                    .set_cursor_icon(eframe::egui::CursorIcon::PointingHand);
            }
            if resp.clicked() {
                let res = if checked {
                    crate::associate::remove(ext)
                } else {
                    crate::associate::associate(ext)
                };
                if let Err(e) = res {
                    log::warn!("ассоциация .{ext}: {e}");
                }
                ui.ctx().request_repaint();
            }
            x += item_w(g) + gap;
        }
    });

    // Включённые расширения, назначенные системой другим приложениям:
    // наша ассоциация записана, но двойной клик открывает «их». Подсказка
    // ОДНА на все такие расширения (текст и кнопка почти не различаются):
    // «.md», «.txt» → «.md и .txt».
    let hijacked: Vec<&str> = crate::associate::EXTENSIONS
        .iter()
        .copied()
        .filter(|ext| crate::associate::is_associated(ext))
        .filter(|ext| crate::associate::foreign_default(ext).is_some())
        .collect();
    if !hijacked.is_empty() {
        ui.add_space(2.0);
        let names: Vec<String> = hijacked.iter().map(|e| format!(".{e}")).collect();
        let exts = match names.as_slice() {
            [] => String::new(), // недостижимо: hijacked непуст
            [one] => one.clone(),
            [init @ .., last] => {
                // Два — без запятой («.md и .txt»), больше — через запятую.
                let head = if init.len() == 1 {
                    init[0].clone()
                } else {
                    init.join(", ")
                };
                format!("{head} {} {}", t(lang, "st_and"), last)
            }
        };
        let key = if hijacked.len() == 1 {
            "st_assoc_hijack"
        } else {
            "st_assoc_hijack_multi"
        };
        let text = t(lang, key).replace("{ext}", &exts);
        let size = em * 0.85;
        let btn_g = plain_galley(ui, t(lang, "st_assoc_open"), size, pal.muted);
        let btn_size = btn_g.size() + vec2(em * 1.2, em * 0.6);
        // Подсказка бывает длиннее панели: не влезает с кнопкой в строку —
        // перенос по словам на всю ширину, кнопка уходит строкой ниже.
        let avail = (ui.available_width() - PAD * 2.0).max(80.0);
        let mut note = plain_galley(ui, &text, size, pal.muted);
        let inline = note.size().x + 10.0 + btn_size.x <= avail;
        if !inline {
            note = wrapped_galley(ui, &text, size, pal.muted, avail);
        }
        let note_h = if inline {
            note.size().y.max(btn_size.y).max(em * 0.9)
        } else {
            note.size().y
        };
        let note_w = note.size().x;
        alloc_row(ui, note_h, |ui, row| {
            let painter = ui.painter().clone();
            painter.galley(
                pos2(row.min.x + PAD, row.center().y - note.size().y * 0.5),
                note,
                pal.muted,
            );
            if inline {
                assoc_open_btn(
                    ui,
                    Rect::from_min_size(
                        pos2(
                            row.min.x + PAD + note_w + 10.0,
                            row.center().y - btn_size.y * 0.5,
                        ),
                        btn_size,
                    ),
                    &btn_g,
                    pal,
                    &exts,
                );
            }
        });
        if !inline {
            ui.add_space(4.0);
            alloc_row(ui, btn_size.y.max(em * 0.9), |ui, row| {
                assoc_open_btn(
                    ui,
                    Rect::from_min_size(
                        pos2(row.min.x + PAD, row.center().y - btn_size.y * 0.5),
                        btn_size,
                    ),
                    &btn_g,
                    pal,
                    &exts,
                );
            });
        }
    }
    ui.add_space(6.0);
}

/// Кнопка «Открыть настройки Windows» под подсказкой ассоциации: плоская
/// рамка, заливка при наведении; клик — системные «Приложения по умолчанию»
/// (UserChoice защищён подписью — программно сменить нельзя).
fn assoc_open_btn(ui: &mut Ui, rect: Rect, g: &Arc<Galley>, pal: &Palette, ext: &str) {
    let resp = ui.interact(rect, ui.scope_id().with(("assoc_open", ext)), Sense::click());
    let hovered = resp.hovered();
    let painter = ui.painter().clone();
    painter.rect(
        rect,
        CornerRadius::same(0),
        if hovered {
            lighten(pal.bg, 0.10)
        } else {
            eframe::egui::Color32::TRANSPARENT
        },
        Stroke::new(1.0, if hovered { pal.muted } else { pal.border }),
        eframe::egui::StrokeKind::Middle,
    );
    painter.galley_with_override_text_color(
        rect.center() - g.size() * 0.5,
        g.clone(),
        if hovered { pal.fg } else { pal.muted },
    );
    if hovered {
        ui.ctx()
            .set_cursor_icon(eframe::egui::CursorIcon::PointingHand);
    }
    if resp.clicked() {
        let _ = std::process::Command::new("explorer")
            .arg("ms-settings:defaultapps")
            .spawn();
    }
}

/// Строка «подпись слева, слайдер + значение справа». Слайдер тянется на всё
/// свободное место; значения притягиваются к сетке `step`.
#[allow(clippy::too_many_arguments)]
fn slider_row(
    ui: &mut Ui,
    salt: &str,
    label: &str,
    em: f32,
    pal: &Palette,
    min: f32,
    max: f32,
    step: f32,
    value: &mut f32,
    digits: usize,
) {
    let row_h = em + 14.0;
    let lg = if label.is_empty() {
        None
    } else {
        Some(plain_galley(ui, label, em, pal.muted))
    };
    let val = format!("{value:.digits$}");
    let vg = plain_galley(ui, &val, em, pal.muted);
    let val_w = vg.size().x + 10.0;
    let label_w = lg.as_ref().map_or(0.0, |g| g.size().x);
    let avail = ui.available_width() - PAD * 2.0;
    let track_w = (avail - label_w - val_w - 10.0).max(70.0);

    alloc_row(ui, row_h, |ui, row| {
        let painter = ui.painter().clone();
        let mut x = row.min.x + PAD;
        if let Some(g) = &lg {
            painter.galley(
                pos2(x, row.center().y - g.size().y * 0.5),
                g.clone(),
                pal.muted,
            );
            x += label_w + 10.0;
        }
        let rect = Rect::from_min_size(pos2(x, row.min.y), vec2(track_w, row_h));
        let resp = ui.interact(
            rect,
            ui.scope_id().with(("slider", salt)),
            Sense::click_and_drag(),
        );
        if let Some(p) = resp.interact_pointer_pos() {
            if resp.is_pointer_button_down_on() {
                let t = ((p.x - rect.min.x) / rect.width()).clamp(0.0, 1.0);
                let v = min + t * (max - min);
                *value = (((v - min) / step).round() * step + min).clamp(min, max);
            }
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(eframe::egui::CursorIcon::PointingHand);
        }

        let t = ((*value - min) / (max - min)).clamp(0.0, 1.0);
        let cy = rect.center().y;
        let track = Rect::from_min_size(pos2(rect.min.x, cy - 1.5), vec2(rect.width(), 3.0));
        painter.rect_filled(track, CornerRadius::same(0), pal.border);
        if t > 0.0 {
            painter.rect_filled(
                Rect::from_min_size(track.min, vec2(track.width() * t, 3.0)),
                CornerRadius::same(0),
                pal.link,
            );
        }
        painter.circle(
            pos2(track.min.x + track.width() * t, cy),
            5.0,
            pal.fg,
            Stroke::NONE,
        );

        let vr = Rect::from_min_size(pos2(rect.max.x + 10.0, row.min.y), vec2(val_w, row_h));
        painter.galley(
            pos2(vr.min.x, vr.center().y - vg.size().y * 0.5),
            vg,
            pal.muted,
        );
    });
    ui.add_space(6.0);
}

/// Строка фиксированной высоты на всю ширину; рисование — внутри замыкания.
fn alloc_row(ui: &mut Ui, h: f32, paint: impl FnOnce(&mut Ui, Rect)) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    paint(ui, rect);
}

fn hline(ui: &mut Ui, pal: &Palette) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter()
        .rect_filled(r, CornerRadius::same(0), pal.border);
}
