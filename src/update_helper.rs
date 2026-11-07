#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod platform {
    use sha2::{Digest, Sha256};
    use std::os::windows::io::FromRawHandle;
    use std::os::windows::process::CommandExt;
    use std::path::{Path, PathBuf};
    use std::{fs, io::Read, process::Command};

    fn wait_for_process(id: u32) -> Result<(), String> {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn OpenProcess(access: u32, inherit: i32, id: u32) -> *mut std::ffi::c_void;
            fn WaitForSingleObject(handle: *mut std::ffi::c_void, timeout: u32) -> u32;
        }
        let handle = unsafe { OpenProcess(0x100000, 0, id) };
        if handle.is_null() {
            let error = std::io::Error::last_os_error();
            return if error.raw_os_error() == Some(87) {
                Ok(())
            } else {
                Err(format!("Не удалось дождаться закрытия приложения: {error}"))
            };
        }
        let _guard = unsafe { std::os::windows::io::OwnedHandle::from_raw_handle(handle) };
        if unsafe { WaitForSingleObject(handle, 120_000) } != 0 {
            return Err("Приложение ещё работает. Закройте его и повторите обновление.".into());
        }
        Ok(())
    }

    fn check_directory(directory: &Path) -> Result<(), String> {
        let temp = std::env::temp_dir()
            .canonicalize()
            .map_err(|error| error.to_string())?;
        let actual = directory
            .canonicalize()
            .map_err(|error| error.to_string())?;
        if actual.parent() != Some(temp.as_path())
            || !actual
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("school-day-update-"))
        {
            return Err("Не найдена папка загруженного обновления".into());
        }
        Ok(())
    }

    fn cleanup(directory: &Path) -> Result<(), String> {
        check_directory(directory)?;
        for name in ["setup.exe", "helper.exe"] {
            let file = directory.join(name);
            match fs::remove_file(file) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.to_string()),
            }
        }
        fs::remove_dir(directory).map_err(|error| error.to_string())
    }

    fn install(
        directory: &Path,
        application: &Path,
        size: u64,
        expected_hash: &str,
    ) -> Result<(), String> {
        use std::os::windows::fs::OpenOptionsExt;
        check_directory(directory)?;
        let installer = directory.join("setup.exe");
        let mut file = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&installer)
            .map_err(|error| error.to_string())?;
        if file.metadata().map_err(|error| error.to_string())?.len() != size {
            return Err("Размер установщика изменился. Повторите загрузку.".into());
        }
        let mut hash = Sha256::new();
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
            if count == 0 {
                break;
            }
            hash.update(&buffer[..count]);
        }
        if format!("{:x}", hash.finalize()) != expected_hash {
            return Err("Контрольная сумма установщика изменилась. Повторите загрузку.".into());
        }
        let result = Command::new(&installer)
            .args(["/S", "/UPDATE"])
            .creation_flags(0x0800_0000)
            .status()
            .map_err(|error| format!("Не удалось запустить установщик: {error}"))?;
        drop(file);
        if !result.success() {
            return Err(
                "Не удалось установить обновление. Откройте «Школьный день» и повторите попытку."
                    .into(),
            );
        }
        Command::new(application)
            .creation_flags(0x0800_0000)
            .spawn()
            .map_err(|error| {
                format!("Обновление установлено. Не удалось открыть приложение: {error}")
            })?;
        Ok(())
    }

    fn launch_cleanup(directory: &Path, application: &Path) {
        let helper = application.with_file_name("school-day-updater.exe");
        let _ = Command::new(helper)
            .arg("cleanup")
            .arg(std::process::id().to_string())
            .arg(directory)
            .creation_flags(0x0800_0000)
            .spawn();
    }

    fn show_error(message: &str) {
        #[link(name = "user32")]
        unsafe extern "system" {
            fn MessageBoxW(
                window: *mut std::ffi::c_void,
                text: *const u16,
                title: *const u16,
                flags: u32,
            ) -> i32;
        }
        let message: Vec<_> = message.encode_utf16().chain(Some(0)).collect();
        let title: Vec<_> = "Обновление «Школьного дня»"
            .encode_utf16()
            .chain(Some(0))
            .collect();
        unsafe {
            MessageBoxW(std::ptr::null_mut(), message.as_ptr(), title.as_ptr(), 0x10);
        }
    }

    pub fn run() -> Result<(), String> {
        let args: Vec<_> = std::env::args_os().skip(1).collect();
        let mode = args
            .first()
            .and_then(|arg| arg.to_str())
            .ok_or("Не задан режим обновления")?;
        if mode == "cleanup" && args.len() == 3 {
            let pid = args[1]
                .to_str()
                .and_then(|pid| pid.parse().ok())
                .ok_or("Не задан процесс обновления")?;
            wait_for_process(pid)?;
            return cleanup(Path::new(&args[2]));
        }
        if mode != "install" || args.len() != 6 {
            return Err("Неверные параметры обновления".into());
        }
        let pid = args[1]
            .to_str()
            .and_then(|pid| pid.parse().ok())
            .ok_or("Не задан процесс приложения")?;
        let directory = PathBuf::from(&args[2]);
        let application = PathBuf::from(&args[3]);
        let size = args[4]
            .to_str()
            .and_then(|size| size.parse().ok())
            .ok_or("Не задан размер обновления")?;
        let hash = args[5].to_str().ok_or("Не задана контрольная сумма")?;
        let result =
            wait_for_process(pid).and_then(|()| install(&directory, &application, size, hash));
        if let Err(error) = &result {
            show_error(error);
        }
        launch_cleanup(&directory, &application);
        result
    }
}

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    match platform::run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(_) => std::process::ExitCode::FAILURE,
    }
}

#[cfg(not(windows))]
fn main() {}
