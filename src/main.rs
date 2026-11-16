// В release — GUI-приложение без окна консоли (в debug консоль оставляем для логов).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;

fn main() -> eframe::Result {
    env_logger::init();

    // Аргументы CLI: пути к .md файлам, каждый — отдельная вкладка.
    let paths: Vec<PathBuf> = std::env::args_os()
        .skip(1)
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .collect();

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("DarkTechMD")
            .with_inner_size([1100.0, 760.0])
            .with_min_inner_size([560.0, 380.0]),
        ..Default::default()
    };

    eframe::run_native(
        "DarkTechMD",
        options,
        Box::new(move |cc| Ok(Box::new(darktech_md::app::App::new(cc, paths)))),
    )
}
