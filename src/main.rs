// В release — GUI-приложение без окна консоли (в debug консоль оставляем для логов).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;

use darktech_md::monitor;
use darktech_md::settings::model::SettingsData;
use darktech_md::settings::store;

fn main() -> eframe::Result {
    env_logger::init();

    // Настройки читаем ДО создания окна: геометрия (позиция/размер/
    // развёрнутость) восстанавливается из сессии в settings.json.
    let settings = store::load();

    // Аргументы CLI: пути к .md файлам, каждый — отдельная вкладка.
    // С явными аргументами сессия не восстанавливается (явное намерение).
    let paths: Vec<PathBuf> = std::env::args_os()
        .skip(1)
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .collect();

    let options = eframe::NativeOptions {
        viewport: viewport_builder(&settings),
        ..Default::default()
    };

    eframe::run_native(
        "DarkTechMD",
        options,
        Box::new(move |cc| Ok(Box::new(darktech_md::app::App::new(cc, paths, settings)))),
    )
}

/// Окно из сессии: позиция — только если попадает на существующий монитор
/// (после смены мониторов окно не должно оказаться за экраном), размер —
/// нормальный (не развёрнутый). Координаты — поинты egui. Окно создаётся
/// НЕВИДИМЫМ: первый кадр успевает применить тему титульной строки — окно
/// показывается сразу с правильным заголовком (без светлого мигания).
fn viewport_builder(s: &SettingsData) -> eframe::egui::ViewportBuilder {
    let ses = &s.session;
    let mut vb = eframe::egui::ViewportBuilder::default()
        .with_title("DarkTechMD")
        .with_min_inner_size([560.0, 380.0])
        .with_visible(false);
    if ses.win_maximized {
        vb = vb.with_maximized(true);
    }
    if let Some([w, h]) = ses.win_size
        && (560.0..=16384.0).contains(&w)
        && (380.0..=16384.0).contains(&h)
    {
        vb = vb.with_inner_size([w, h]);
    }
    if let Some([x, y]) = ses.win_pos
        && monitor::point_visible(x, y)
    {
        vb = vb.with_position([x as f32, y as f32]);
    }
    vb
}
