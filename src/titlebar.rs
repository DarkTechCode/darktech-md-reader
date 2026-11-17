//! Тема титульной строки Windows: DWMWA_USE_IMMERSIVE_DARK_MODE напрямую
//! по HWND. Зачем не команда egui `ViewportCommand::SetTheme`:
//! 1) eframe показывает окно сам (post_rendering после первой покраски), а
//!    команды вьюпорта обрабатываются УЖЕ ПОСЛЕ показа — заголовок на миг
//!    остаётся системно-светлым при тёмной программе;
//! 2) winit 0.30 красит заголовок через устаревший недокументированный
//!    SetWindowCompositionAttribute(WCA_USEDARKMODECOLORS) — на новых
//!    билдах Windows он игнорируется, официальный атрибут DWM — наш путь.
//! HWND берём в App::new из CreationContext: окно уже создано, но ещё
//! СКРЫТО (eframe прячет до первой покраски) — атрибут применяется до
//! первого показа, мигания нет вовсе.

use std::sync::OnceLock;

use windows_sys::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE,
};

/// HWND главного окна (isize-обёртка — чтобы жил в статике).
static HWND: OnceLock<isize> = OnceLock::new();

/// Запомнить HWND главного окна (вызывается один раз из App::new).
pub fn set_hwnd(hwnd: isize) {
    if hwnd != 0 {
        let _ = HWND.set(hwnd);
    }
}

/// Тёмная (true) / светлая (false) титульная строка. Применяется мгновенно,
/// в том числе до показа окна. Атрибут 19 — фолбэк старых билдов Win10
/// (до 20H1 у DWMWA_USE_IMMERSIVE_DARK_MODE был другой номер).
pub fn set_dark(dark: bool) {
    let Some(&h) = HWND.get() else { return };
    let hwnd = h as windows_sys::Win32::Foundation::HWND;
    let v: i32 = dark as i32;
    let attr = |a: u32| unsafe {
        DwmSetWindowAttribute(
            hwnd,
            a,
            &v as *const i32 as *const core::ffi::c_void,
            std::mem::size_of::<i32>() as u32,
        ) == 0 // S_OK
    };
    if !attr(DWMWA_USE_IMMERSIVE_DARK_MODE as u32) {
        attr(19);
    }
}
