//! Ассоциация расширений (`.md`, `.txt`) с DarkTechMD — только HKCU, без
//! прав администратора. Чекбоксы в настройках; прежний обработчик
//! расширения запоминается и восстанавливается при отключении.

/// Ассоциируемые расширения (без точки) — чекбоксы в настройках.
pub const EXTENSIONS: [&str; 2] = ["md", "txt"];

/// Отображаемое имя типа документа для расширения.
fn doc_name(ext: &str) -> String {
    match ext {
        "md" => "Документ Markdown (DarkTechMD)".to_string(),
        "txt" => "Текстовый документ (DarkTechMD)".to_string(),
        other => format!("{other} (DarkTechMD)"),
    }
}

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::Foundation::{ERROR_SUCCESS, WIN32_ERROR};
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegDeleteValueW, RegOpenKeyExW,
        RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE,
        REG_OPTION_NON_VOLATILE, REG_SZ,
    };
    use windows_sys::Win32::UI::Shell::{
        SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_FLUSH, SHCNF_IDLIST,
    };

    const CLASSES: &str = "Software\\Classes";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn check(code: WIN32_ERROR, what: &str) -> Result<(), String> {
        if code == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(format!("{what}: код {code}"))
        }
    }

    fn prog_id(ext: &str) -> String {
        format!("DarkTechMD.{ext}")
    }

    /// Открыть (или создать) подраздел HKCU\Software\Classes\<sub>.
    fn open_key(sub: &str, create: bool) -> Result<HKEY, String> {
        open_hkcu(&format!("{CLASSES}\\{sub}"), create)
    }

    /// Открыть (или создать) раздел по полному пути от HKCU.
    fn open_hkcu(path: &str, create: bool) -> Result<HKEY, String> {
        let path = wide(path);
        let mut hk = std::ptr::null_mut();
        let code = unsafe {
            if create {
                RegCreateKeyExW(
                    HKEY_CURRENT_USER,
                    path.as_ptr(),
                    0,
                    std::ptr::null(),
                    REG_OPTION_NON_VOLATILE,
                    KEY_QUERY_VALUE | KEY_SET_VALUE,
                    std::ptr::null(),
                    &mut hk,
                    std::ptr::null_mut(),
                )
            } else {
                RegOpenKeyExW(HKEY_CURRENT_USER, path.as_ptr(), 0, KEY_QUERY_VALUE, &mut hk)
            }
        };
        check(code, "открытие раздела реестра")?;
        Ok(hk)
    }

    fn set_str(hk: HKEY, name: Option<&str>, value: &str) -> Result<(), String> {
        let name = wide(name.unwrap_or("")); // пустое имя = значение по умолчанию
        let data = wide(value);
        let bytes = (data.len() * 2) as u32;
        check(
            unsafe { RegSetValueExW(hk, name.as_ptr(), 0, REG_SZ, data.as_ptr().cast(), bytes) },
            "запись значения",
        )
    }

    /// Прочитать REG_SZ-значение (имя None — значение по умолчанию).
    fn get_str(hk: HKEY, name: Option<&str>) -> Option<String> {
        let name = wide(name.unwrap_or(""));
        let mut len = 0u32;
        let code = unsafe {
            RegQueryValueExW(
                hk,
                name.as_ptr(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut len,
            )
        };
        if code != ERROR_SUCCESS || len == 0 {
            return None;
        }
        let mut buf = vec![0u8; len as usize];
        let code = unsafe {
            RegQueryValueExW(
                hk,
                name.as_ptr(),
                std::ptr::null(),
                std::ptr::null_mut(),
                buf.as_mut_ptr(),
                &mut len,
            )
        };
        if code != ERROR_SUCCESS {
            return None;
        }
        let units: Vec<u16> = buf[..len as usize]
            .chunks_exact(2)
            .map(|c| u16::from_ne_bytes([c[0], c[1]]))
            .collect();
        Some(String::from_utf16_lossy(&units))
            .filter(|s| !s.is_empty())
            .map(|s| s.trim_end_matches('\0').to_string())
    }

    fn delete_value(hk: HKEY, name: Option<&str>) -> Result<(), String> {
        let name = wide(name.unwrap_or(""));
        check(unsafe { RegDeleteValueW(hk, name.as_ptr()) }, "удаление значения")
    }

    /// Ожидаемая команда открытия текущим exe.
    fn expected_command() -> String {
        let exe = std::env::current_exe().unwrap_or_default();
        format!("\"{}\" \"%1\"", exe.display())
    }

    fn notify_shell() {
        unsafe {
            SHChangeNotify(
                SHCNE_ASSOCCHANGED as i32,
                SHCNF_IDLIST | SHCNF_FLUSH,
                std::ptr::null(),
                std::ptr::null(),
            );
        }
    }

    /// Ассоциировано ли расширение с этим exe (наш ProgID и наша команда).
    pub fn is_associated(ext: &str) -> bool {
        let Ok(hk) = open_key(&format!("{}\\shell\\open\\command", prog_id(ext)), false) else {
            return false;
        };
        let cmd = get_str(hk, None);
        unsafe { RegCloseKey(hk) };
        cmd.as_deref() == Some(expected_command().as_str())
    }

    /// Назначенный Windows обработчик расширения (UserChoice в FileExts),
    /// если это НЕ DarkTechMD: тогда проводник открывает его, игнорируя нашу
    /// ассоциацию. UserChoice защищён подписью — сменить можно только в
    /// системных настройках («Приложения по умолчанию»).
    pub fn foreign_default(ext: &str) -> Option<String> {
        let path = format!(
            "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\FileExts\\.{ext}\\UserChoice"
        );
        let hk = open_hkcu(&path, false).ok()?;
        let prog = get_str(hk, Some("ProgId"));
        unsafe { RegCloseKey(hk) };
        prog.filter(|p| p != &prog_id(ext))
    }

    /// Сделать DarkTechMD обработчиком расширения (HKCU). Прежний обработчик
    /// запоминается в значении PrevProgId и вернётся при отключении.
    pub fn associate(ext: &str) -> Result<(), String> {
        let prog = prog_id(ext);
        let hk_ext = open_key(&format!(".{ext}"), true)?;
        let prev = get_str(hk_ext, None).filter(|p| p != &prog);
        let hk_ours = open_key(&prog, true)?;
        let icon = {
            let exe = std::env::current_exe().unwrap_or_default();
            format!("{},0", exe.display())
        };
        let result = (|| {
            if let Some(p) = &prev {
                set_str(hk_ours, Some("PrevProgId"), p)?;
            } else {
                let _ = delete_value(hk_ours, Some("PrevProgId"));
            }
            set_str(hk_ours, None, &super::doc_name(ext))?;
            set_str(hk_ours, Some("DefaultIcon"), &icon)?;
            let hk_cmd = open_key(&format!("{prog}\\shell\\open\\command"), true)?;
            let r = set_str(hk_cmd, None, &expected_command());
            unsafe { RegCloseKey(hk_cmd) };
            r?;
            set_str(hk_ext, None, &prog)
        })();
        unsafe {
            RegCloseKey(hk_ours);
            RegCloseKey(hk_ext);
        }
        result?;
        notify_shell();
        Ok(())
    }

    /// Убрать ассоциацию расширения: вернуть прежний обработчик (или удалить
    /// наш), удалить наш ProgID-ключ.
    pub fn remove(ext: &str) -> Result<(), String> {
        let prog = prog_id(ext);
        // Прежний обработчик — до удаления ключа.
        let prev = open_key(&prog, false)
            .ok()
            .and_then(|hk| {
                let p = get_str(hk, Some("PrevProgId"));
                unsafe { RegCloseKey(hk) };
                p
            });
        check(
            unsafe {
                RegDeleteTreeW(
                    HKEY_CURRENT_USER,
                    wide(&format!("{CLASSES}\\{prog}")).as_ptr(),
                )
            },
            "удаление раздела ассоциации",
        )?;
        // Default-значение .<ext> указывает на наш ProgID?
        let ours = open_key(&format!(".{ext}"), false)
            .ok()
            .and_then(|hk| {
                let v = get_str(hk, None);
                unsafe { RegCloseKey(hk) };
                v
            })
            .is_some_and(|v| v == prog);
        if ours {
            let hk_ext = open_key(&format!(".{ext}"), true)?;
            let r = match prev {
                Some(p) => set_str(hk_ext, None, &p),
                None => delete_value(hk_ext, None),
            };
            unsafe { RegCloseKey(hk_ext) };
            r?;
        }
        notify_shell();
        Ok(())
    }
}

#[cfg(windows)]
pub use imp::{associate, foreign_default, is_associated, remove};

#[cfg(not(windows))]
mod imp {
    pub fn is_associated(_ext: &str) -> bool {
        false
    }
    pub fn foreign_default(_ext: &str) -> Option<String> {
        None
    }
    pub fn associate(_ext: &str) -> Result<(), String> {
        Err("ассоциация файлов поддерживается только на Windows".into())
    }
    pub fn remove(_ext: &str) -> Result<(), String> {
        Err("ассоциация файлов поддерживается только на Windows".into())
    }
}
