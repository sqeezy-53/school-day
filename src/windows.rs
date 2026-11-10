use std::os::windows::process::CommandExt;
use std::os::windows::prelude::OwnedHandle;

pub fn initialize() -> Result<OwnedHandle, String> {
    let guard = running_guard()?;
    prepare_runtime()?;
    Ok(guard)
    
}

pub fn running_guard() -> Result<OwnedHandle, String> {
    use std::os::windows::io::FromRawHandle;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateMutexW(
            attributes: *mut std::ffi::c_void,
            owner: i32,
            name: *const u16,
        ) -> *mut std::ffi::c_void;
        fn OpenMutexW(access: u32, inherit: i32, name: *const u16) -> *mut std::ffi::c_void;
    }
    let installing: Vec<u16> = "Local\\io.github.sqeezy_53.school-day.installing"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let check_installing = || {
        let handle = unsafe { OpenMutexW(0x100000, 0, installing.as_ptr()) };
        if handle.is_null() {
            let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(2) {
            return Ok(());
        }
        Err(format!("Не удалось проверить, выполняется ли установка: {}", error))
        } else {
            let _guard = unsafe { OwnedHandle::from_raw_handle(handle) };
            Err("Выполняется установка «Школьного дня». Дождитесь её завершения и откройте приложение снова.".to_string())
        }
    };
    check_installing()?;
    let name: Vec<u16> = "Local\\io.github.sqeezy_53.school-day"
        .encode_utf16()
        .chain(Some(0))
        .collect();

    let handle = unsafe { CreateMutexW(std::ptr::null_mut(), 0, name.as_ptr()) };

    let os_error = std::io::Error::last_os_error();
    if handle.is_null() {
        return Err(os_error.to_string());
    }
    let guard = unsafe { OwnedHandle::from_raw_handle(handle) };

    if os_error.raw_os_error() == Some(183) {
        return Err("Школьный день уже запущен".to_string());
    }
    check_installing()?;
    Ok(guard)
}

pub fn prepare_runtime() -> Result<(), String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let prefix = executable
        .parent()
        .and_then(|path| path.parent())
        .ok_or("Не найдена папка приложения")?;
    let query = prefix.join("bin/gdk-pixbuf-query-loaders.exe");

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
        .current_dir(&modules)
        .args(&loaders)
        .env_remove("G_MESSAGES_DEBUG")
        .creation_flags(0x0800_0000)
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
    unsafe {
        MessageBoxW(std::ptr::null_mut(), message.as_ptr(), title.as_ptr(), 0x10);
    }
}
