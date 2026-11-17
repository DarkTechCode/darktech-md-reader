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
    ("welcome_recent", "Последние файлы"),
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
    ("settings_title", "Настройки чтения"),
    ("st_theme", "Тема"),
    ("st_light", "Светлая"),
    ("st_dark", "Тёмная"),
    ("st_auto", "Авто"),
    ("st_lang", "Язык"),
    ("st_lang_ru", "Русский"),
    ("st_lang_en", "English"),
    ("st_font", "Шрифт"),
    ("st_font_size", "Размер шрифта"),
    ("st_line_height", "Межстрочный"),
    ("st_text_width", "Ширина текста"),
    ("st_narrow", "Узкая"),
    ("st_medium", "Средняя"),
    ("st_wide", "Широкая"),
    ("st_letter_spacing", "Межбуквенный"),
    ("st_ui_font", "Шрифт интерфейса"),
    ("st_reset", "Сбросить настройки"),
    ("st_restart", "Перезапуск"),
    ("st_site", "Сайт"),
    ("st_fit_width", "По ширине окна"),
    ("st_table_align", "Ячейки таблиц"),
    ("st_cell_top", "По верху"),
    ("st_cell_center", "По центру"),
    ("st_table_scroll", "Скролл в таблицах"),
    ("cm_select_all", "Выделить всё"),
    ("find_placeholder", "Поиск"),
    ("cm_copy_url", "Копировать адрес"),
    ("tm_explorer", "Показать в проводнике"),
    ("tm_save_as", "Сохранить как…"),
    ("st_line_nums", "Нумерация строк"),
    ("st_highlight", "Подсветка кода"),
    ("st_on", "Вкл"),
    ("st_off", "Выкл"),
    ("st_open", "Открыть"),
    ("st_outline", "Оглавление"),
    ("st_outline_side", "Сторона оглавления"),
    ("st_side_left", "Слева"),
    ("st_side_right", "Справа"),
    ("st_outline_level", "Уровень оглавления"),
    ("st_minimap", "Миникарта"),
    ("sb_symbols", "симв."),
    ("sb_lines", "строк"),
    ("sb_words", "слов"),
    ("sb_selected", "выделено"),
    ("sb_on", "вкл"),
    ("sb_off", "выкл"),
    ("st_author", "Автор"),
    ("sb_b", "Б"),
    ("sb_kb", "КБ"),
    ("sb_mb", "МБ"),
    ("sb_gb", "ГБ"),
];

const EN: &[(&str, &str)] = &[
    ("app_name", "DarkTechMD"),
    ("welcome_title", "DarkTechMD"),
    ("welcome_hint", "Drop a .md file into the window or pick one manually"),
    ("open_file", "Open file…"),
    ("welcome_recent", "Recent files"),
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
    ("settings_title", "Reading settings"),
    ("st_theme", "Theme"),
    ("st_light", "Light"),
    ("st_dark", "Dark"),
    ("st_auto", "Auto"),
    ("st_lang", "Language"),
    ("st_lang_ru", "Русский"),
    ("st_lang_en", "English"),
    ("st_font", "Font"),
    ("st_font_size", "Font size"),
    ("st_line_height", "Line spacing"),
    ("st_text_width", "Text width"),
    ("st_narrow", "Narrow"),
    ("st_medium", "Medium"),
    ("st_wide", "Wide"),
    ("st_letter_spacing", "Letter spacing"),
    ("st_ui_font", "UI font"),
    ("st_reset", "Reset settings"),
    ("st_restart", "Restart"),
    ("st_site", "Site"),
    ("st_fit_width", "Fit width"),
    ("st_table_align", "Table cells"),
    ("st_cell_top", "Top"),
    ("st_cell_center", "Center"),
    ("st_table_scroll", "Table scrolling"),
    ("cm_select_all", "Select all"),
    ("find_placeholder", "Find"),
    ("cm_copy_url", "Copy link address"),
    ("tm_explorer", "Show in Explorer"),
    ("tm_save_as", "Save as…"),
    ("st_line_nums", "Line numbers"),
    ("st_highlight", "Syntax highlight"),
    ("st_on", "On"),
    ("st_off", "Off"),
    ("st_open", "Open"),
    ("st_outline", "Outline"),
    ("st_outline_side", "Outline side"),
    ("st_side_left", "Left"),
    ("st_side_right", "Right"),
    ("st_outline_level", "Outline depth"),
    ("st_minimap", "Minimap"),
    ("sb_symbols", "chars"),
    ("sb_lines", "lines"),
    ("sb_words", "words"),
    ("sb_selected", "selected"),
    ("sb_on", "on"),
    ("sb_off", "off"),
    ("st_author", "Author"),
    ("sb_b", "B"),
    ("sb_kb", "KB"),
    ("sb_mb", "MB"),
    ("sb_gb", "GB"),
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
