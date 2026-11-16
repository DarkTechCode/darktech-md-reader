//! Локализация UI: RU / EN. Все строки интерфейса — только через `t`.

use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Ru,
    En,
}

/// Язык «Системный»: определяется один раз при старте.
pub fn system_lang() -> Lang {
    static LANG: OnceLock<Lang> = OnceLock::new();
    *LANG.get_or_init(|| match sys_locale::get_locale().as_deref() {
        Some(l) if l.to_ascii_lowercase().starts_with("ru") => Lang::Ru,
        _ => Lang::En,
    })
}

const RU: &[(&str, &str)] = &[
    ("app_name", "DarkTechMD"),
    ("welcome_title", "DarkTechMD"),
    ("welcome_hint", "Перетащите файл .md в окно или выберите его вручную"),
    ("open_file", "Открыть файл…"),
    ("close_tab", "Закрыть вкладку"),
    ("copy", "Копировать"),
    ("copied", "Скопировано!"),
    ("err_read", "Не удалось прочитать файл"),
    ("err_file_gone", "Файл недоступен"),
    ("loading", "Чтение файла…"),
    ("diagram_label", "диаграмма"),
    ("html_label", "html"),
    ("words", "слов"),
    ("read_min", "чтение: {} мин"),
    ("alert_note", "Примечание"),
    ("alert_tip", "Совет"),
    ("alert_important", "Важно"),
    ("alert_warning", "Предупреждение"),
    ("alert_caution", "Осторожно"),
];

const EN: &[(&str, &str)] = &[
    ("app_name", "DarkTechMD"),
    ("welcome_title", "DarkTechMD"),
    ("welcome_hint", "Drop a .md file into the window or pick one manually"),
    ("open_file", "Open file…"),
    ("close_tab", "Close tab"),
    ("copy", "Copy"),
    ("copied", "Copied!"),
    ("err_read", "Failed to read file"),
    ("err_file_gone", "File is not available"),
    ("loading", "Reading file…"),
    ("diagram_label", "diagram"),
    ("html_label", "html"),
    ("words", "words"),
    ("read_min", "reading: {} min"),
    ("alert_note", "Note"),
    ("alert_tip", "Tip"),
    ("alert_important", "Important"),
    ("alert_warning", "Warning"),
    ("alert_caution", "Caution"),
];

/// Строка интерфейса по ключу. Отсутствующий ключ падает — значит, опечатка в коде.
pub fn t(lang: Lang, key: &str) -> &'static str {
    let table = match lang {
        Lang::Ru => RU,
        Lang::En => EN,
    };
    let found = table
        .iter()
        .find(|(k, _)| *k == key)
        .unwrap_or_else(|| panic!("i18n: нет ключа {key:?}"))
        .1;
    // fallback на английский, если строка оставлена пустой
    if found.is_empty() {
        EN.iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| *v)
            .unwrap_or_default()
    } else {
        found
    }
}
