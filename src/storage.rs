use serde::{Serialize, de::DeserializeOwned};
use std::fs::{File, OpenOptions, TryLockError};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

const MAX_JSON_BYTES: u64 = 32 * 1024 * 1024;

pub fn data_dir() -> PathBuf {
    gtk::glib::user_data_dir().join("school-day")
}

pub fn lock_data() -> Result<File, String> {
    lock_directory(&data_dir())
}

fn lock_directory(directory: &Path) -> Result<File, String> {
    std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let mut options = local_file_options();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options
        .open(directory.join(".school-day.lock"))
        .map_err(|error| format!("Ошибка доступа к данным: {error}"))?;
    require_regular_file(&file.metadata().map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(TryLockError::WouldBlock) => {
            Err("Приложение уже запущено. Закройте другое окно и попробуйте снова.".into())
        }
        Err(TryLockError::Error(error)) => Err(format!("Ошибка блокировки данных: {error}")),
    }
}

pub fn load_json<T>(
    name: &str,
    validate: impl Fn(&mut T) -> Result<(), String>,
) -> Result<Option<T>, String>
where
    T: DeserializeOwned + Serialize,
{
    load_json_inner(name, validate, true)
}

pub fn read_json<T>(
    name: &str,
    validate: impl Fn(&mut T) -> Result<(), String>,
) -> Result<Option<T>, String>
where
    T: DeserializeOwned + Serialize,
{
    load_json_inner(name, validate, false)
}

fn load_json_inner<T>(
    name: &str,
    validate: impl Fn(&mut T) -> Result<(), String>,
    migrate: bool,
) -> Result<Option<T>, String>
where
    T: DeserializeOwned + Serialize,
{
    let target = data_dir().join(name);
    match read_json_file(&target) {
        Ok(json) => return decode(&json, &validate).map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("{}: {error}", target.display())),
    }

    if std::env::var_os("FLATPAK_ID").is_some() {
        return Ok(None);
    }
    if let Ok(executable) = std::env::current_exe()
        && let Some(directory) = executable.parent()
    {
        let source = directory.join("data").join(name);
        let json = match read_json_file(&source) {
            Ok(json) => json,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("{}: {error}", source.display())),
        };
        let value = decode(&json, &validate)?;
        if migrate {
            save_json(name, &value)?;
        }
        return Ok(Some(value));
    }
    Ok(None)
}

fn local_file_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    options
}

fn require_regular_file(metadata: &std::fs::Metadata) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Ожидался обычный файл",
            ));
        }
    }
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Ожидался обычный файл",
        ));
    }
    Ok(())
}

fn read_json_file(path: &Path) -> io::Result<String> {
    require_regular_file(&std::fs::symlink_metadata(path)?)?;
    let file = local_file_options().read(true).open(path)?;
    let metadata = file.metadata()?;
    require_regular_file(&metadata)?;
    if metadata.len() > MAX_JSON_BYTES {
        return Err(json_too_large());
    }
    let mut json = String::new();
    file.take(MAX_JSON_BYTES + 1).read_to_string(&mut json)?;
    if json.len() as u64 > MAX_JSON_BYTES {
        return Err(json_too_large());
    }
    Ok(json)
}

fn json_too_large() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Размер файла данных превышает 32 МиБ",
    )
}

fn decode<T: DeserializeOwned>(
    json: &str,
    validate: &impl Fn(&mut T) -> Result<(), String>,
) -> Result<T, String> {
    let mut value = serde_json::from_str(json).map_err(|error| error.to_string())?;
    validate(&mut value)?;
    Ok(value)
}

pub fn save_json<T: Serialize + ?Sized>(name: &str, value: &T) -> Result<(), String> {
    let json = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    write_json(&data_dir(), name, &json).map_err(|error| error.to_string())
}

pub fn reset_json<T: Serialize>(name: &str, value: &T) -> Result<Option<PathBuf>, String> {
    let directory = data_dir();
    let target = directory.join(name);
    let backup = match std::fs::symlink_metadata(&target) {
        Ok(metadata) if !metadata.is_dir() => {
            let backup = directory.join(format!("{name}.{}.bak", gtk::glib::uuid_string_random()));
            std::fs::rename(&target, &backup).map_err(|error| error.to_string())?;
            Some(backup)
        }
        Ok(_) => return Err(format!("{} — каталог", target.display())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.to_string()),
    };
    if let Err(error) = save_json(name, value) {
        if let Some(backup) = &backup
            && let Err(restore) = std::fs::rename(backup, &target)
        {
            return Err(format!(
                "{error}. Исходный файл сохранён в {}. Ошибка восстановления: {restore}",
                backup.display()
            ));
        }
        return Err(error);
    }
    Ok(backup)
}

fn write_json(directory: &Path, name: &str, json: &[u8]) -> std::io::Result<()> {
    if json.len() as u64 > MAX_JSON_BYTES {
        return Err(json_too_large());
    }
    std::fs::create_dir_all(directory)?;
    let target = directory.join(name);
    let temporary = directory.join(format!(".{name}.{}.tmp", gtk::glib::uuid_string_random()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let mut file = options.open(&temporary)?;
    let result = (|| {
        file.write_all(json)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, &target)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    #[cfg(unix)]
    if result.is_ok() {
        let _ = File::open(directory).and_then(|directory| directory.sync_all());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Sandbox(PathBuf);
    impl Sandbox {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "school-day-test-{}",
                gtk::glib::uuid_string_random()
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_second_writer_is_blocked_until_the_first_closes() {
        let sandbox = Sandbox::new();
        let first = lock_directory(&sandbox.0).unwrap();
        assert!(lock_directory(&sandbox.0).is_err());
        drop(first);
        assert!(lock_directory(&sandbox.0).is_ok());
    }

    #[test]
    fn settings_keep_hiding_preferences_after_atomic_save_and_reload()
    -> Result<(), Box<dyn std::error::Error>> {
        let sandbox = Sandbox::new();
        let original = crate::settings::Settings {
            class: "9Б".into(),
            hide_old_lessons: true,
            old_lesson_days: 180,
            ..crate::settings::Settings::default()
        };
        let path = sandbox.0.join("settings.json");
        write_json(&sandbox.0, "settings.json", &serde_json::to_vec(&original)?)?;
        let mut loaded: crate::settings::Settings = serde_json::from_str(&read_json_file(&path)?)?;
        assert!(loaded == original);
        loaded.class = "10Б".into();
        loaded.check_updates = true;
        write_json(&sandbox.0, "settings.json", &serde_json::to_vec(&loaded)?)?;
        let reloaded: crate::settings::Settings = serde_json::from_str(&read_json_file(&path)?)?;
        assert!(reloaded == loaded);
        assert!(reloaded.hide_old_lessons);
        assert_eq!(reloaded.old_lesson_days, 180);
        Ok(())
    }

    #[test]
    fn atomic_save_replaces_existing_contents_and_leaves_no_temp_files() {
        let sandbox = Sandbox::new();
        write_json(&sandbox.0, "lessons.json", b"[1]").unwrap();
        write_json(&sandbox.0, "lessons.json", b"[2,3]").unwrap();
        assert_eq!(
            std::fs::read(sandbox.0.join("lessons.json")).unwrap(),
            b"[2,3]"
        );
        assert_eq!(std::fs::read_dir(&sandbox.0).unwrap().count(), 1);
    }

    #[test]
    fn failed_replace_cleans_its_temporary_file() {
        let sandbox = Sandbox::new();
        std::fs::create_dir(sandbox.0.join("lessons.json")).unwrap();
        assert!(write_json(&sandbox.0, "lessons.json", b"[]").is_err());
        assert!(sandbox.0.join("lessons.json").is_dir());
        assert_eq!(std::fs::read_dir(&sandbox.0).unwrap().count(), 1);
    }

    #[test]
    fn read_rejects_data_above_limit_before_loading_it() {
        let sandbox = Sandbox::new();
        let path = sandbox.0.join("lessons.json");
        File::create(&path)
            .unwrap()
            .set_len(MAX_JSON_BYTES + 1)
            .unwrap();
        assert_eq!(
            read_json_file(&path).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[cfg(unix)]
    #[test]
    fn reading_data_and_locking_refuse_symbolic_links() {
        let sandbox = Sandbox::new();
        let original = sandbox.0.join("original");
        std::fs::write(&original, "[]").unwrap();
        let data = sandbox.0.join("lessons.json");
        std::os::unix::fs::symlink(&original, &data).unwrap();
        std::os::unix::fs::symlink(&original, sandbox.0.join(".school-day.lock")).unwrap();
        assert_eq!(
            read_json_file(&data).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert!(lock_directory(&sandbox.0).is_err());
        assert_eq!(std::fs::read_to_string(original).unwrap(), "[]");
    }

    #[cfg(unix)]
    #[test]
    fn precreated_temp_symlink_cannot_overwrite_an_unrelated_file() {
        let sandbox = Sandbox::new();
        let victim = sandbox.0.join("private-note.txt");
        std::fs::write(&victim, b"KEEP").unwrap();
        std::os::unix::fs::symlink(&victim, sandbox.0.join("lessons.json.tmp")).unwrap();
        write_json(&sandbox.0, "lessons.json", b"[]").unwrap();
        assert_eq!(std::fs::read(&victim).unwrap(), b"KEEP");
        assert!(!sandbox.0.join("lessons.json").is_symlink());
    }

    #[cfg(unix)]
    #[test]
    fn saved_personal_data_is_readable_only_by_its_owner() {
        use std::os::unix::fs::PermissionsExt;
        let sandbox = Sandbox::new();
        write_json(&sandbox.0, "lessons.json", b"[]").unwrap();
        let permissions = std::fs::metadata(sandbox.0.join("lessons.json"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(permissions & 0o777, 0o600);
    }
}
