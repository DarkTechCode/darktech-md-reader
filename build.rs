//! Ресурсы Windows-исполняемого файла: иконка, версия, метаданные (winres).
//! Иконка окна ОС берёт из ресурсов exe автоматически (winit не задаёт свою).

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "1.0.0".into());
    let mut res = winres::WindowsResource::new();
    res.set_icon("assets/icon.ico");
    res.set("FileDescription", "DarkTechMD — читалка Markdown");
    res.set("ProductName", "DarkTechMD");
    res.set("LegalCopyright", "© Dark Wizard · https://darktech.ru");
    res.set("FileVersion", &version);
    res.set("ProductVersion", &version);
    if let Err(e) = res.compile() {
        // Сборка без rc.exe (чистый хост без Windows SDK) не должна падать —
        // exe соберётся без ресурсов, предупреждение объяснит причину.
        println!("cargo:warning=winres: {e}");
    }
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=build.rs");
}
