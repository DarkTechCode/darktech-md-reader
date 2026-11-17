//! Проверка сохранённой геометрии окна: позиция должна попадать на
//! существующий монитор (после смены мониторов/DPI окно не должно
//! оказываться за экраном). Координаты храним в поинтах egui.

/// Попадает ли точка (поинты, координаты монитора) на какой-либо монитор.
#[cfg(windows)]
pub fn point_visible(x: i32, y: i32) -> bool {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::{MonitorFromPoint, MONITOR_DEFAULTTONULL};
    use windows_sys::Win32::UI::HiDpi::GetDpiForSystem;

    // Поинты → физические пиксели по системному DPI (per-monitor может
    // отличаться — это эвристика против полностью невидимого окна).
    let scale = unsafe { GetDpiForSystem() } as f32 / 96.0;
    let pt = POINT {
        x: (x as f32 * scale).round() as i32,
        y: (y as f32 * scale).round() as i32,
    };
    let monitor = unsafe { MonitorFromPoint(pt, MONITOR_DEFAULTTONULL) };
    !monitor.is_null()
}

#[cfg(not(windows))]
pub fn point_visible(_x: i32, _y: i32) -> bool {
    true
}
