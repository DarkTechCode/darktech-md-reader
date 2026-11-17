//! Ассоциация `.md` с DarkTechMD (только HKCU — без прав администратора).
//! Кнопка в настройках, off по умолчанию (PLAN §9). Прежний обработчик `.md`
//! запоминается и восстанавливается при отключении.

/// ProgID нашей ассоциации в HKCU\Software\Classes.
pub const PROG_ID: &str = "DarkTechMD.md";

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::Foundation::{ERROR_SUCCESS, WIN32_ERROR};
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegDeleteValueW, RegOpenKeyExW,
        RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE,
        REG_OPTION_NON_VOLATILE, REG_SZ,
    };
    use windows_sys::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_FLUSH, SHCNF_IDLIST};

    use super::PROG_ID;

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

    /// Открыть (или создать) подраздел HKCU\Software\Classes\<sub>.
    fn open_key(sub: &str, create: bool) -> Result<HKEY, String> {
        let path = wide(&format!("{CLASSES}\\{sub}"));
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
            unsafe {
                RegSetValueExW(hk, name.as_ptr(), 0, REG_SZ, data.as_ptr().cast(), bytes)
            },
            "запись значения",
        )
    }

    /// Прочитать REG_SZ-значение (имя None — значение по умолчанию).
    fn get_str(hk: HKEY, name: Option<&str>) -> Option<String> {
        let name = wide(name.unwrap_or(""));
        let mut len = 0u32;
        let code = unsafe {
            RegQueryValueExW(hk, name.as_ptr(), std::ptr::null(), std::ptr::null_mut(), std::ptr::null_mut(), &mut len)
        };
        if code != ERROR_SUCCESS || len == 0 {
            return None;
        }
        let mut buf = vec![0u8; len as usize];
        let code = unsafe {
            RegQueryValueExW(hk, name.as_ptr(), std::ptr::null(), std::ptr::null_mut(), buf.as_mut_ptr(), &mut len)
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
            SHChangeNotify(SHCNE_ASSOCCHANGED as i32, SHCNF_IDLIST | SHCNF_FLUSH, std::ptr::null(), std::ptr::null());
        }
    }

    /// Ассоциирован ли `.md` с этим exe (наш ProgID и наша команда).
    pub fn is_associated() -> bool {
        let Ok(hk) = open_key(&format!("{PROG_ID}\\shell\\open\\command"), false) else {
            return false;
        };
        let cmd = get_str(hk, None);
        unsafe { RegCloseKey(hk) };
        cmd.as_deref() == Some(expected_command().as_str())
    }

    /// Сделать DarkTechMD обработчиком `.md` (HKCU). Прежний обработчик
    /// запоминается в значении PrevProgId и вернётся при отключении.
    pub fn associate() -> Result<(), String> {
        let hk_md = open_key(".md", true)?;
        let prev = get_str(hk_md, None).filter(|p| p != PROG_ID);
        let hk_ours = open_key(PROG_ID, true)?;
        let result = (|| {
            if let Some(p) = &prev {
                set_str(hk_ours, Some("PrevProgId"), p)?;
            } else {
                let _ = delete_value(hk_ours, Some("PrevProgId"));
            }
            set_str(hk_ours, None, "Документ Markdown (DarkTechMD)")?;
            set_str(hk_ours, Some("DefaultIcon"), &icon_value())?;
            let hk_cmd = open_key(&format!("{PROG_ID}\\shell\\open\\command"), true)?;
            let r = set_str(hk_cmd, None, &expected_command());
            unsafe { RegCloseKey(hk_cmd) };
            r?;
            set_str(hk_md, None, PROG_ID)
        })();
        unsafe {
            RegCloseKey(hk_ours);
            RegCloseKey(hk_md);
        }
        result?;
        notify_shell();
        Ok(())
    }

    /// Значение DefaultIcon: «путь,0» — иконка из ресурсов exe (winres).
    fn icon_value() -> String {
        let exe = std::env::current_exe().unwrap_or_default();
        format!("{},0", exe.display())
    }

    /// Убрать ассоциацию: вернуть прежний обработчик `.md` (или удалить
    /// наш), удалить наш ProgID-ключ.
    pub fn remove() -> Result<(), String> {
        // Прежний обработчик — до удаления ключа.
        let prev = open_key(PROG_ID, false)
            .ok()
            .and_then(|hk| {
                let p = get_str(hk, Some("PrevProgId"));
                unsafe { RegCloseKey(hk) };
                p
            });
        check(
            unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, wide(&format!("{CLASSES}\\{PROG_ID}")).as_ptr()) },
            "удаление раздела ассоциации",
        )?;
        if is_ours_md_default() {
            let hk_md = open_key(".md", true)?;
            let r = match prev {
                Some(p) => set_str(hk_md, None, &p),
                None => delete_value(hk_md, None),
            };
            unsafe { RegCloseKey(hk_md) };
            r?;
        }
        notify_shell();
        Ok(())
    }

    /// Default-значение `.md` указывает на наш ProgID.
    fn is_ours_md_default() -> bool {
        open_key(".md", false)
            .ok()
            .and_then(|hk| {
                let v = get_str(hk, None);
                unsafe { RegCloseKey(hk) };
                v
            })
            .as_deref()
            == Some(PROG_ID)
    }
}

#[cfg(windows)]
pub use imp::{associate, is_associated, remove};

#[cfg(not(windows))]
mod imp {
    pub fn is_associated() -> bool {
        false
    }
    pub fn associate() -> Result<(), String> {
        Err("ассоциация файлов поддерживается только на Windows".into())
    }
    pub fn remove() -> Result<(), String> {
        Err("ассоциация файлов поддерживается только на Windows".into())
    }
}
