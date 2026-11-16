//! Связка панелей навигации (оглавление, миникарта) со скроллом документа:
//! глобальные ключи temp-data (обе стороны строят их одинаково, независимо от
//! ui-ско́пов), метрики прокрутки, запросы прокрутки, сброс автоскролла.

use eframe::egui::{Context, Id, Pos2};

/// Глобальный ключ temp-data вкладки.
pub fn key(tab: u64, salt: &'static str) -> Id {
    Id::unique(("darktech_nav", tab, salt))
}

/// Метрики прокрутки документа — пишет doc_view каждый кадр, читают панели.
#[derive(Clone, Copy, Debug)]
pub struct Metrics {
    /// Текущий оффсет контента, px.
    pub offset: f32,
    /// Высота вьюпорта, px.
    pub viewport_h: f32,
    /// Полная высота контента (с полями), px.
    pub content_h: f32,
}

pub fn publish_metrics(ctx: &Context, tab: u64, m: Metrics) {
    ctx.data_mut(|d| d.insert_temp(key(tab, "metrics"), m));
}

pub fn metrics(ctx: &Context, tab: u64) -> Option<Metrics> {
    ctx.data(|d| d.get_temp(key(tab, "metrics")))
}

/// Запрос прокрутки документа (обрабатывает doc_view в ближайшем кадре).
#[derive(Clone, Copy, Debug)]
pub enum ScrollRequest {
    /// К блоку (заголовку): верх блока — к верху окна.
    Block(usize),
    /// Абсолютный оффсет контента, px.
    Y(f32),
}

pub fn request_scroll(ctx: &Context, tab: u64, req: ScrollRequest) {
    ctx.data_mut(|d| d.insert_temp(key(tab, "scroll_req"), req));
    ctx.request_repaint();
}

pub fn take_request(ctx: &Context, tab: u64) -> Option<ScrollRequest> {
    ctx.data_mut(|d| {
        let id = key(tab, "scroll_req");
        let v = d.get_temp(id);
        if v.is_some() {
            d.remove::<ScrollRequest>(id);
        }
        v
    })
}

/// Автоскролл: якорь хранится по глобальному ключу, чтобы миникарта могла
/// выключить его при драге (иначе два механизма боролись бы за оффсет).
pub fn set_autoscroll_anchor(ctx: &Context, tab: u64, anchor: Option<Pos2>) {
    ctx.data_mut(|d| d.insert_temp(key(tab, "as_anchor"), anchor));
}

pub fn autoscroll_anchor(ctx: &Context, tab: u64) -> Option<Pos2> {
    ctx.data(|d| d.get_temp(key(tab, "as_anchor"))).flatten()
}

/// Отложенный оффсет автоскролла (применяется следующим кадром).
pub fn set_autoscroll_offset(ctx: &Context, tab: u64, off: f32) {
    ctx.data_mut(|d| d.insert_temp(key(tab, "as_off"), off));
}

pub fn take_autoscroll_offset(ctx: &Context, tab: u64) -> Option<f32> {
    ctx.data_mut(|d| {
        let id = key(tab, "as_off");
        let v = d.get_temp(id);
        if v.is_some() {
            d.remove::<f32>(id);
        }
        v
    })
}

/// Выключить автоскролл (навигация перехватила управление прокруткой).
pub fn stop_autoscroll(ctx: &Context, tab: u64) {
    ctx.data_mut(|d| {
        d.insert_temp(key(tab, "as_anchor"), Option::<Pos2>::None);
        d.remove::<f32>(key(tab, "as_off"));
    });
}
