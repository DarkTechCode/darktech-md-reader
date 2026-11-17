//! Область документа: виртуальный скролл. Рендерится только видимое окно
//! блоков ± overscan; высоты — измеренные и оценённые (префикс-суммы в
//! LayoutState), скроллбар уточняется по мере измерения.

use eframe::egui::{
    self, pos2, scroll_area, Color32, Id, Key, PointerButton, Pos2, Rect, ScrollArea, Sense,
    Stroke, Ui, Vec2,
};

use crate::layout::{measure, LayoutKey, LayoutState};
use crate::md::blocks::Doc;

use super::find::{self, FindState};
use super::nav;
use super::render;
use super::selection::{self, SelFrame, SelectionState};
use super::themes::{self, lighten};
use super::Env;

/// Событие из документа (клик по ссылке и т.п.).
#[derive(Clone, Debug)]
pub enum DocEvent {
    OpenLink(String),
    /// Правый клик: показать контекст-меню в позиции (возможно, на ссылке).
    ContextMenu {
        pos: Pos2,
        link: Option<String>,
    },
}

/// Запас рендера сверху/снизу от вьюпорта, px.
const OVERSCAN: f32 = 600.0;
/// Поля над/под документом в координатах контента (нужно панелям навигации).
pub const TOP_PAD: f32 = 20.0;
pub const BOTTOM_PAD: f32 = 40.0;

const FAB_SIZE: f32 = 44.0;
const FAB_MARGIN_BOTTOM: f32 = 20.0;
/// Справа — с учётом ширины скроллбара, чтобы зазор был как снизу.
const FAB_MARGIN_RIGHT: f32 = FAB_MARGIN_BOTTOM + 26.0;
const FAB_ACCENT: Color32 = Color32::from_rgb(64, 140, 255);

/// Автоскролл средней кнопкой: мёртвая зона и разгон (px/s на px дистанции).
const AUTOSCROLL_DEAD: f32 = 16.0;
const AUTOSCROLL_GAIN: f32 = 6.0;
const AUTOSCROLL_MAX: f32 = 2400.0;

pub fn show(
    ui: &mut Ui,
    doc: &Doc,
    layout: &mut LayoutState,
    env: &Env,
    tab_key: u64,
    sel: &mut SelectionState,
    find: &FindState,
) -> Vec<DocEvent> {
    let mut events = Vec::new();
    // Тот же id, что у ScrollArea (make_persistent_id = scope_id.with(salt)).
    let scroll_id = ui.scope_id().with(("doc_scroll", tab_key));
    let fab_id = ui.scope_id().with(("fab_top", tab_key));

    // Принудительный оффсет этого кадра: восстановление сохранённой позиции
    // (сессия/перезагрузка файла — приоритетнее прочих просьб кадра),
    // «наверх», навигация (оглавление/миникарта/якорь), автоскролл или
    // компенсация «прилипания» (высоты выше вьюпорта изменились).
    let mut forced: Option<f32> = None;
    let to_top = ui.ctx().data_mut(|d| {
        let v = d.get_temp::<bool>(fab_id).unwrap_or(false);
        if v {
            d.insert_temp(fab_id, false);
        }
        v
    });
    let nav_req = nav::take_request(ui.ctx(), tab_key);
    let as_off = nav::take_autoscroll_offset(ui.ctx(), tab_key);
    // Восстановление применяем, только когда раскладка готова (первый ensure
    // дал оценки высот): в кадре ожидания документ не рисуем (см. ниже) —
    // иначе мелькнул бы «топ» перед прыжком на сохранённую позицию.
    let restoring = nav::peek_restore(ui.ctx(), tab_key);
    if let Some(r) = restoring.filter(|_| layout.is_ready()) {
        let block = r.block.min(layout.blocks_len().saturating_sub(1));
        forced = Some((TOP_PAD + layout.offset(block) + (r.y - r.block_top)).max(0.0));
        nav::clear_restore(ui.ctx(), tab_key);
    } else if to_top {
        forced = Some(0.0);
        nav::stop_autoscroll(ui.ctx(), tab_key);
    } else if let Some(req) = nav_req {
        let y = match req {
            nav::ScrollRequest::Block(i) => TOP_PAD + layout.offset(i),
            nav::ScrollRequest::Y(y) => y,
        };
        forced = Some(y.max(0.0));
        nav::stop_autoscroll(ui.ctx(), tab_key);
    } else if let Some(off) = as_off {
        forced = Some(off);
    } else if let Some(delta) = layout.take_pending_scroll() {
        let cur = scroll_area::State::load(ui.ctx(), scroll_id)
            .map(|s| s.clamped_offset().y)
            .unwrap_or(0.0);
        forced = Some((cur + delta).max(0.0));
    }

    let mut sa = ScrollArea::vertical()
        .id_salt(("doc_scroll", tab_key))
        .auto_shrink([false, false]);
    if let Some(off) = forced {
        sa = sa.vertical_scroll_offset(off);
    }

    let out = sa.show_viewport(ui, |ui, viewport| {
        let avail = ui.available_width();
        // «По ширине окна»: колонка на всё, но с полями 20px, чтобы текст
        // не прилипал к краям (и не мешал скроллбару). Ось — целые пиксели:
        // дробная ширина дрожала бы раскладкой при каждом пикселе ресайза.
        // В обычном режиме узкое окно тоже не прилипает к краям: минимум
        // 10px с каждой стороны.
        const FIT_PAD: f32 = 20.0;
        const SIDE_PAD: f32 = 10.0;
        let col = if env.fit_width {
            avail - FIT_PAD * 2.0
        } else {
            env.typo.width.min(avail - SIDE_PAD * 2.0)
        }
        .floor()
        .max(120.0)
        .min(avail);
        layout.ensure(
            doc,
            LayoutKey {
                typo: *env.typo,
                width: col,
                dark: env.pal.bg == themes::DARK.bg,
                lang: env.lang,
                code_nums: env.code_line_numbers,
                table_scroll: env.table_scroll,
                text_align: env.text_align,
            },
        );

        // origin = экранная позиция контента (0,0); viewport — координаты контента.
        let origin = ui.cursor().min;
        ui.set_min_height(TOP_PAD + layout.total_height() + BOTTOM_PAD);

        if restoring.is_some() && !layout.is_ready() {
            // Кадр-ожидание восстановления: ensure только что обеспечил
            // раскладку, позицию применит следующий кадр (forced). Контент
            // не рисуем — так восстановление происходит без видимого прыжка.
            ui.ctx().request_repaint();
            return;
        }

        let side = ((avail - col) / 2.0).max(0.0).floor();
        let x0 = origin.x + side;
        let vis = viewport.translate(origin.to_vec2());
        let vr = layout.visible_range(viewport.min.y - OVERSCAN, viewport.max.y + OVERSCAN);
        let i0 = vr.start;

        // Попадания поиска — только видимых блоков (список отсортирован).
        let empty: &[find::Hit] = &[];
        let (find_hits, find_current) = if find.open && !find.hits.is_empty() {
            let a = find.hits.partition_point(|h| h.block < vr.start);
            let b = find.hits.partition_point(|h| h.block < vr.end);
            (&find.hits[a..b], find.cur_hit())
        } else {
            (empty, None)
        };

        // Собственное выделение: области регистрируются при отрисовке,
        // мышь обрабатывается после (драг над областью — прямо в place_text).
        let mut frame = SelFrame {
            sel,
            regions: Vec::new(),
            pointer: ui.input(|i| i.pointer.interact_pos()),
            link_hover: false,
            find_hits,
            find_current,
            // Попап поиска (прошлый кадр) глушит выделение под собой.
            blocked: find.open.then_some(find.popup_rect).flatten(),
        };

        let mut y = origin.y + TOP_PAD + layout.offset(i0);
        for i in vr {
            let block = &doc.blocks[i];
            if layout.prepared(i).is_none() {
                let (p, h) = measure::measure_block(ui, env, col, block, &doc.source);
                layout.set_measured(i, h);
                layout.put_prepared(i, p);
            }
            let h = layout.height(i);
            let rect = Rect::from_min_size(pos2(x0, y), Vec2::new(col, h));
            let prep = layout.prepared_mut(i).expect("prepared только что создан");
            let (ev, new_h) = render::render_block(
                ui, rect, vis, tab_key, i, block, prep, env, &mut frame, &doc.source,
            );
            if let Some(h) = new_h {
                layout.set_measured(i, h);
            }
            events.extend(ev);
            y += layout.height(i);
        }

        selection::end_frame(
            ui,
            doc,
            &frame.regions,
            // Область мыши документа — видимый ВЬЮПОРТ (vis), не max_rect:
            // контент скролла выше/ниже вьюпорта накрыл бы таб-бар и
            // статус-бар (I-beam и клики по ним у документа).
            vis,
            frame.sel,
            frame.link_hover,
            &mut events,
            frame.blocked,
        );
        // Живая статистика выделения (гигантские диапазоны — по отпусканию).
        frame.sel.recompute_stats_if_due(doc);

        if layout.finish_frame(i0) {
            ui.ctx().request_repaint();
        }
    });

    // Прогресс прокрутки: насколько далеко от начала документа.
    let offset = out.state.clamped_offset().y;
    let scrollable = (out.content_size.y - out.inner_rect.height()).max(1.0);
    let progress = (offset / scrollable).clamp(0.0, 1.0);
    if progress > 0.001 {
        show_fab(ui, progress, fab_id, env);
    }

    // Метрики для панелей навигации (оглавление, миникарта).
    nav::publish_metrics(
        ui.ctx(),
        tab_key,
        nav::Metrics {
            offset,
            viewport_h: out.inner_rect.height(),
            content_h: out.content_size.y,
        },
    );

    autoscroll(ui, &out, tab_key, env);
    events
}

/// Плавный автоскролл средней кнопкой (как в браузерах): клик по документу —
/// якорь; чем дальше курсор от якоря, тем быстрее прокрутка. Повторный клик,
/// колесо или Esc выключают. Следующий оффсет прокидывается через temp-data
/// и применяется в начале следующего кадра (как «наверх» / компенсация).
fn autoscroll(
    ui: &mut Ui,
    out: &egui::containers::scroll_area::ScrollAreaOutput<()>,
    tab_key: u64,
    env: &Env,
) {
    // Якорь и оффсет — на глобальных ключах nav: миникарта выключает
    // автоскролл при драге (два механизма не борются за оффсет).
    let mut anchor = nav::autoscroll_anchor(ui.ctx(), tab_key);

    // Клик средней кнопкой по области документа — вкл/выкл.
    let mid_pressed = ui.input(|i| i.pointer.button_pressed(PointerButton::Middle));
    if mid_pressed {
        let pos = ui.input(|i| i.pointer.interact_pos());
        anchor = match (anchor, pos) {
            (Some(_), _) => None,
            (None, Some(p)) if out.inner_rect.contains(p) => Some(p),
            _ => None,
        };
    }
    // Колесо или Esc — выключить.
    let wheeled =
        ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::MouseWheel { .. })));
    if anchor.is_some() && (wheeled || ui.input(|i| i.key_pressed(Key::Escape))) {
        anchor = None;
    }
    nav::set_autoscroll_anchor(ui.ctx(), tab_key, anchor);
    let Some(anchor) = anchor else { return };

    let dir = if let Some(p) = ui.input(|i| i.pointer.interact_pos()) {
        let dy = p.y - anchor.y;
        if dy.abs() <= AUTOSCROLL_DEAD {
            0.0
        } else {
            let v = ((dy.abs() - AUTOSCROLL_DEAD) * AUTOSCROLL_GAIN).min(AUTOSCROLL_MAX);
            dy.signum() * v
        }
    } else {
        0.0
    };

    if dir != 0.0 {
        let dt = ui.input(|i| i.unstable_dt).min(0.1);
        let max = (out.content_size.y - out.inner_rect.height()).max(0.0);
        let cur = out.state.clamped_offset().y;
        let next = (cur + dir * dt).clamp(0.0, max);
        if (next - cur).abs() > 0.01 {
            nav::set_autoscroll_offset(ui.ctx(), tab_key, next);
        }
    }

    // Индикатор-«якорь»: кружок с шевронами; активное направление — акцентом.
    let painter = ui.painter().clone();
    let r = 14.0;
    painter.circle(anchor, r, env.pal.bg, Stroke::new(1.5, env.pal.border));
    // Шеврон остриём вверх или вниз (направление прокрутки).
    let chevron = |cy: f32, s: f32, up: bool, color: Color32| {
        let (tip_y, side_y) = if up { (cy - s * 0.28, cy + s * 0.22) } else { (cy + s * 0.28, cy - s * 0.22) };
        painter.line_segment(
            [pos2(anchor.x - s * 0.5, side_y), pos2(anchor.x, tip_y)],
            Stroke::new(1.8, color),
        );
        painter.line_segment(
            [pos2(anchor.x + s * 0.5, side_y), pos2(anchor.x, tip_y)],
            Stroke::new(1.8, color),
        );
    };
    chevron(anchor.y - 6.0, 7.0, true, if dir < 0.0 { env.pal.link } else { env.pal.faint });
    chevron(anchor.y + 6.0, 7.0, false, if dir > 0.0 { env.pal.link } else { env.pal.faint });

    ui.ctx().request_repaint();
}

/// Круглая кнопка «наверх» с индикатором прогресса чтения по обводке.
fn show_fab(ui: &mut Ui, progress: f32, scroll_top_id: Id, env: &Env) {
    let bounds = ui.max_rect();
    let min = pos2(
        bounds.max.x - FAB_MARGIN_RIGHT - FAB_SIZE,
        bounds.max.y - FAB_MARGIN_BOTTOM - FAB_SIZE,
    );
    let rect = Rect::from_min_size(min, Vec2::splat(FAB_SIZE));
    let resp = ui.interact(rect, ui.scope_id().with("fab"), Sense::click());
    let painter = ui.painter().clone();
    let center = rect.center();
    let radius = FAB_SIZE * 0.5;

    let hovered = resp.hovered();
    let bg = if hovered {
        lighten(env.pal.fab_bg, 0.12)
    } else {
        env.pal.fab_bg
    };
    let chevron = if hovered {
        env.pal.fg
    } else {
        env.pal.fab_chevron
    };
    let border = env.pal.fab_border;
    let stroke = if border == Color32::TRANSPARENT {
        Stroke::NONE
    } else if hovered {
        Stroke::new(1.5, env.pal.muted)
    } else {
        Stroke::new(1.0, border)
    };
    painter.circle(center, radius, bg, stroke);

    // Обводка-прогресс: акцентная дуга, полный круг в самом низу.
    let a0 = -std::f32::consts::FRAC_PI_2;
    let sweep = std::f32::consts::TAU * progress.clamp(0.0, 1.0);
    let steps = ((progress * 64.0).ceil() as usize).max(2);
    let pts: Vec<egui::Pos2> = (0..=steps)
        .map(|i| {
            let a = a0 + sweep * (i as f32 / steps as f32);
            pos2(center.x + radius * a.cos(), center.y + radius * a.sin())
        })
        .collect();
    painter.add(egui::Shape::line(pts, Stroke::new(3.0, FAB_ACCENT)));

    // Шеврон вверх.
    let s = radius * 0.42;
    painter.line_segment(
        [
            pos2(center.x - s, center.y + s * 0.4),
            pos2(center.x, center.y - s * 0.6),
        ],
        Stroke::new(2.5, chevron),
    );
    painter.line_segment(
        [
            pos2(center.x, center.y - s * 0.6),
            pos2(center.x + s, center.y + s * 0.4),
        ],
        Stroke::new(2.5, chevron),
    );

    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if resp.clicked() {
        ui.ctx().data_mut(|d| d.insert_temp(scroll_top_id, true));
    }
}
