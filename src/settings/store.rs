//! Хранение настроек: %APPDATA%\DarkTechMD\settings.json.
//! Запись атомарная (tmp + rename); повреждённый файл → дефолты, молча.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::model::SettingsData;

/// Путь к settings.json; без %APPDATA% настройки живут только в памяти.
pub fn settings_path() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|base| {
        PathBuf::from(base)
            .join("DarkTechMD")
            .join("settings.json")
    })
}

pub fn load() -> SettingsData {
    settings_path().map(|p| load_from(&p)).unwrap_or_default()
}

pub fn save(data: &SettingsData) -> io::Result<()> {
    match settings_path() {
        Some(p) => save_to(&p, data),
        None => Ok(()),
    }
}

/// Читает настройки; нет файла или повреждён — дефолты (без паники и логов).
pub fn load_from(path: &Path) -> SettingsData {
    let Ok(text) = fs::read_to_string(path) else {
        return SettingsData::default();
    };
    match serde_json::from_str::<SettingsData>(&text) {
        Ok(mut d) => {
            d.normalize();
            d
        }
        Err(_) => SettingsData::default(),
    }
}

/// Атомарная запись: сначала tmp, потом rename (на Windows заменяет файл).
pub fn save_to(path: &Path, data: &SettingsData) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(data).map_err(io::Error::from)?)?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::model::{LangMode, ThemeMode};

    fn temp_file(name: &str) -> PathBuf {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("temp")
            .join(name);
        let _ = fs::remove_file(&p);
        p
    }

    #[test]
    fn roundtrip_and_corrupted_gives_defaults() {
        let path = temp_file("test-settings.json");
        let d = SettingsData {
            theme: ThemeMode::Dark,
            language: LangMode::En,
            font_size: 21,
            ..Default::default()
        };
        save_to(&path, &d).unwrap();
        assert_eq!(load_from(&path), d);

        // Повреждённый файл — молча дефолты.
        fs::write(&path, "{ не json").unwrap();
        assert_eq!(load_from(&path), SettingsData::default());

        let _ = fs::remove_file(&path);
    }
}
