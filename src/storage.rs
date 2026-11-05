use serde::{Serialize, de::DeserializeOwned};
use std::fs::{File, OpenOptions, TryLockError};
use std::io::Write;
use std::path::{Path, PathBuf};

pub fn data_dir() -> PathBuf {
    gtk::glib::user_data_dir().join("school-day")
}

// GApplication объединяет запуски только внутри одного сеанса D-Bus.
// Блокировка каталога также защищает от второго сеанса и отдельной копии программы.
pub fn lock_data() -> Result<File, String> {
    lock_directory(&data_dir())
}

fn lock_directory(directory: &Path) -> Result<File, String> {
    std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options
        .open(directory.join(".school-day.lock"))
        .map_err(|error| format!("Не удалось открыть каталог данных: {error}"))?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(TryLockError::WouldBlock) => Err(
            "Эти данные уже открыты в другом экземпляре приложения. Закройте его и повторите запуск."
                .into(),
        ),
        Err(TryLockError::Error(error)) => {
            Err(format!("Не удалось защитить данные от одновременной записи: {error}"))
        }
    }
}

pub fn load_json<T>(
    name: &str,
    validate: impl Fn(&T) -> Result<(), String>,
) -> Result<Option<T>, String>
where
    T: DeserializeOwned + Serialize,
{
    let target = data_dir().join(name);
    match std::fs::read_to_string(&target) {
        Ok(json) => return decode(&json, &validate).map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("{}: {error}", target.display())),
    }

    // Старые версии хранили данные рядом с проектом. Исходные файлы оставляем.
    // В Flatpak данные из домашней папки не читаются: доступен только свой каталог.
    if std::env::var_os("FLATPAK_ID").is_some() {
        return Ok(None);
    }
    let mut directories = Vec::new();
    if let Ok(directory) = std::env::current_dir() {
        directories.push(directory.join("data"));
    }
    if let Ok(executable) = std::env::current_exe()
        && let Some(directory) = executable.parent()
    {
        directories.push(directory.join("data"));
    }
    for directory in directories {
        let source = directory.join(name);
        let json = match std::fs::read_to_string(&source) {
            Ok(json) => json,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(format!("{}: {error}", source.display())),
        };
        let value = decode(&json, &validate)?;
        save_json(name, &value)?;
        return Ok(Some(value));
    }
    Ok(None)
}

fn decode<T: DeserializeOwned>(
    json: &str,
    validate: &impl Fn(&T) -> Result<(), String>,
) -> Result<T, String> {
    let value = serde_json::from_str(json).map_err(|error| error.to_string())?;
    validate(&value)?;
    Ok(value)
}

pub fn save_json<T: Serialize + ?Sized>(name: &str, value: &T) -> Result<(), String> {
    let json = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    write_json(&data_dir(), name, &json).map_err(|error| error.to_string())
}

fn write_json(directory: &Path, name: &str, json: &[u8]) -> std::io::Result<()> {
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
    // create_new не открывает существующий файл или символическую ссылку.
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
        // После успешного rename данные уже изменены. Ошибка fsync каталога
        // не должна приводить к расхождению между экраном и записанным файлом.
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
