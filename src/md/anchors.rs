//! Якоря заголовков (`#slug`): генерация slug-ов в стиле GitHub и поиск
//! заголовка по якорю ссылки. Slug хранится в `Doc::headings` с этапа парсинга.

use std::collections::HashSet;

use super::blocks::Doc;

/// Slug заголовка: буквы/цифры (unicode, к нижнему регистру) остаются,
/// пробелы и `-`/`_` становятся `-`, остальное выбрасывается; повторы
/// различаются суффиксами `-1`, `-2`, … (как на GitHub).
pub fn slugify(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut prev_dash = false;
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            for lower in ch.to_lowercase() {
                out.push(lower);
            }
            prev_dash = false;
        } else if ch == ' ' || ch == '-' || ch == '_' {
            if !prev_dash && !out.is_empty() {
                out.push('-');
                prev_dash = true;
            }
        }
        // Прочие символы (пунктуация, разметка) выбрасываем.
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

/// Дедупликация slug-а в рамках документа: первая реплика — как есть,
/// дальше `-1`, `-2`, … Набор занятых — на стороне вызывающего.
pub fn unique_slug(text: &str, taken: &mut HashSet<String>) -> String {
    let base = slugify(text);
    if taken.insert(base.clone()) {
        return base;
    }
    for n in 1u32.. {
        let cand = format!("{base}-{n}");
        if taken.insert(cand.clone()) {
            return cand;
        }
    }
    unreachable!()
}

/// Поиск заголовка по якорю из ссылки (`#…`): точное совпадение slug-а,
/// с fallback-ами на исходный текст и percent-декодирование (GitHub
/// кодирует кириллицу в ссылках как `%D0%B0…`). Возвращает индекс блока.
pub fn resolve(doc: &Doc, anchor: &str) -> Option<usize> {
    let anchor = anchor.trim_start_matches('#');
    if anchor.is_empty() {
        return None;
    }
    let decoded = percent_decode(anchor);
    let needle = decoded.trim().to_lowercase();
    doc.headings
        .iter()
        .find(|h| h.slug == needle || h.text.trim().to_lowercase() == needle)
        .map(|h| h.block_idx)
}

/// Декодирование `%XX` в байты (невалидные последовательности — как есть).
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2]))
        {
            out.push((h * 16 + l) as u8);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_github_style() {
        assert_eq!(slugify("Hello, World!"), "hello-world");
        assert_eq!(slugify("Заголовок с пробелами"), "заголовок-с-пробелами");
        assert_eq!(slugify("  ---Foo_Bar---  "), "foo-bar");
        assert_eq!(slugify("!!!"), "");
        assert_eq!(slugify("C++ и C#"), "c-и-c");
    }

    #[test]
    fn unique_slug_appends_counters() {
        let mut taken = HashSet::new();
        assert_eq!(unique_slug("Intro", &mut taken), "intro");
        assert_eq!(unique_slug("Intro", &mut taken), "intro-1");
        assert_eq!(unique_slug("intro", &mut taken), "intro-2");
        assert_eq!(unique_slug("Other", &mut taken), "other");
    }

    #[test]
    fn percent_decode_basics() {
        assert_eq!(percent_decode("a%20b"), "a b");
        assert_eq!(percent_decode("%D0%B0"), "а");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
    }
}
