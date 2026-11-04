use std::os::windows::process::CommandExt;

// Установщик проверяет этот объект перед заменой или удалением файлов программы.
pub fn running_guard() -> Result<std::os::windows::io::OwnedHandle, String> {
    use std::os::windows::io::FromRawHandle;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateMutexW(
            attributes: *mut std::ffi::c_void,
            owner: i32,
            name: *const u16,
        ) -> *mut std::ffi::c_void;
    }
    let name: Vec<u16> = "Local\\io.github.sqeezy_53.school-day"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    // Имя завершается нулём; дескриптор закрывается при завершении приложения.
    let handle = unsafe { CreateMutexW(std::ptr::null_mut(), 0, name.as_ptr()) };
    if handle.is_null() {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(unsafe { std::os::windows::io::OwnedHandle::from_raw_handle(handle) })
}

// SVG-модулю нужны пути текущей установки, в том числе с пробелами и кириллицей.
pub fn prepare_runtime() -> Result<(), String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let prefix = executable
        .parent()
        .and_then(|path| path.parent())
        .ok_or("Не найдена папка приложения")?;
    let query = prefix.join("bin/gdk-pixbuf-query-loaders.exe");
    // При запуске из Cargo библиотеки настраивает среда разработчика.
    if !query.is_file() {
        return Ok(());
    }
    let modules = prefix.join("lib/gdk-pixbuf-2.0/2.10.0");
    let mut loaders = std::fs::read_dir(modules.join("loaders"))
        .map_err(|error| format!("Не найдены модули изображений: {error}"))?
        .filter_map(Result::ok)
        .map(|entry| std::path::PathBuf::from("loaders").join(entry.file_name()))
        .filter(|path| path.extension().is_some_and(|extension| extension == "dll"))
        .collect::<Vec<_>>();
    loaders.sort();
    let output = std::process::Command::new(query)
        // У C-утилиты argv зависит от кодовой страницы. Имена DLL содержат ASCII,
        // а полный путь рабочей папки Windows получает через Unicode API.
        .current_dir(&modules)
        .args(&loaders)
        // Отладочные сообщения GLib идут в stdout и не должны попадать в кэш.
        .env_remove("G_MESSAGES_DEBUG")
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .output()
        .map_err(|error| format!("Не удалось подключить значки: {error}"))?;
    let cache = String::from_utf8_lossy(&output.stdout);
    if !output.status.success()
        || !cache.starts_with("# GdkPixbuf Image Loader Modules file")
        || !cache.contains("\"svg\" ")
        || !cache.contains("\"png\" ")
    {
        return Err("Не удалось загрузить модули изображений. Переустановите приложение.".into());
    }
    std::fs::write(modules.join("loaders.cache"), output.stdout)
        .map_err(|error| format!("Не удалось сохранить настройки значков: {error}"))?;
    gtk::gdk_pixbuf::Pixbuf::init_modules(&modules.to_string_lossy())
        .map_err(|error| format!("Не удалось подключить значки: {error}"))
}

pub fn show_error(message: &str) {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn MessageBoxW(
            window: *mut std::ffi::c_void,
            text: *const u16,
            title: *const u16,
            flags: u32,
        ) -> i32;
    }
    let message: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
    let title: Vec<u16> = "Школьный день".encode_utf16().chain(Some(0)).collect();
    // Обе строки завершаются нулём и живут до возврата синхронного MessageBoxW.
    unsafe {
        MessageBoxW(std::ptr::null_mut(), message.as_ptr(), title.as_ptr(), 0x10);
    }
}
