//! Запись в клипборд из фонового потока.
//!
//! На Windows `SetClipboardData` рассылает `SendMessage(HWND_BROADCAST)`
//! всем окнам системы; если хоть одно окно не отвечает (менеджеры буфера,
//! зависшие приложения), синхронный вызов блокируется НАВСЕГДА. eframe пишет
//! в клипборд в конце кадра — главный поток завис бы вместе с окном.
//! Поэтому пишем из отдельного потока и не даём накопиться очереди.

use std::sync::atomic::{AtomicBool, Ordering};

static BUSY: AtomicBool = AtomicBool::new(false);

/// Копирует текст в клипборд в фоне. Если прошлая запись ещё не завершилась —
/// молча пропускаем (поток-писатель всё ещё держит более свежий текст).
pub fn copy_in_background(text: String) {
    if BUSY.swap(true, Ordering::AcqRel) {
        return; // предыдущая запись ещё идёт
    }
    std::thread::spawn(move || {
        if let Ok(mut clip) = arboard::Clipboard::new() {
            let _ = clip.set_text(text);
        }
        BUSY.store(false, Ordering::Release);
    });
}

#[cfg(test)]
mod tests {
    /// Проверка реального обмена с системным клипбордом (трогает буфер
    /// пользователя — запускать вручную: cargo test -- --ignored).
    #[test]
    #[ignore]
    fn clipboard_roundtrip() {
        super::copy_in_background("darktech-md-clipboard-test".into());
        std::thread::sleep(std::time::Duration::from_millis(500));
        let mut clip = arboard::Clipboard::new().expect("клипборд доступен");
        let text = clip.get_text().expect("чтение из клипборда");
        assert_eq!(text, "darktech-md-clipboard-test");
    }
}
