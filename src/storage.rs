use serde::{Serialize, de::DeserializeOwned};
use std::fs::{File, OpenOptions, TryLockError};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

const MAX_JSON_BYTES: u64 = 32 * 1024 * 1024;
const MAX_UPDATE_LOG_BYTES: u64 = 64 * 1024;

pub fn data_dir() -> PathBuf {
    gtk::glib::user_data_dir().join("school-day")
}

pub fn log_update_error(message: &str) {
    if let Err(error) = append_update_error(message) {
        eprintln!("Не удалось записать журнал обновлений: {error}. {message}");
    }
}

fn append_update_error(message: &str) -> io::Result<()> {
    let mut options = local_file_options();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut log = options.open(data_dir().join("updates.log"))?;
    let metadata = log.metadata()?;
    require_regular_file(&metadata)?;
    let message: String = message
        .replace(['\r', '\n'], " ")
        .chars()
        .take(2048)
        .collect();
    let now = chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false);
    let line = format!("[{now}] {message}\n");
    if metadata.len().saturating_add(line.len() as u64) > MAX_UPDATE_LOG_BYTES {
        log.set_len(0)?;
    }
    log.write_all(line.as_bytes())
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
        save_json(name, &value)?;
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
    String::from_utf8(read_bounded_file(path)?)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn read_bounded_file(path: &Path) -> io::Result<Vec<u8>> {
    require_regular_file(&std::fs::symlink_metadata(path)?)?;
    let file = local_file_options().read(true).open(path)?;
    let metadata = file.metadata()?;
    require_regular_file(&metadata)?;
    if metadata.len() > MAX_JSON_BYTES {
        return Err(json_too_large());
    }
    let mut json = Vec::new();
    file.take(MAX_JSON_BYTES + 1).read_to_end(&mut json)?;
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
    reset_json_in(&data_dir(), name, value)
}

fn reset_json_in<T: Serialize>(
    directory: &Path,
    name: &str,
    value: &T,
) -> Result<Option<PathBuf>, String> {
    let replacement = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    let original = match read_bounded_file(&directory.join(name)) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.to_string()),
    };
    let backup = original
        .map(|bytes| {
            let backup_name = format!("{name}.{}.bak", gtk::glib::uuid_string_random());
            write_json(directory, &backup_name, &bytes).map(|()| directory.join(backup_name))
        })
        .transpose()
        .map_err(|error| error.to_string())?;
    // Keep the original in place until its replacement has been written and synced.
    write_json(directory, name, &replacement).map_err(|error| error.to_string())?;
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

pub fn open_schedule() -> Result<(school_day_core::lessons::SchoolData, Option<PathBuf>), String> {
    let legacy_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|p| p.join("data")));
    open_schedule_in(
        &data_dir(),
        legacy_dir
            .filter(|_| std::env::var_os("FLATPAK_ID").is_none())
            .as_deref(),
    )
}

fn open_schedule_in(
    dir: &Path,
    legacy: Option<&Path>,
) -> Result<(school_day_core::lessons::SchoolData, Option<PathBuf>), String> {
    use school_day_core::lessons::SchoolData;
    let target = dir.join("lessons.json");
    let (source, portable) = match read_json_file(&target) {
        Ok(json) => (json, false),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let Some(old) = legacy else {
                return Ok((SchoolData::default(), None));
            };
            match read_json_file(&old.join("lessons.json")) {
                Ok(json) => (json, true),
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    return Ok((SchoolData::default(), None));
                }
                Err(e) => return Err(e.to_string()),
            }
        }
        Err(e) => return Err(e.to_string()),
    };
    let (data, migrate) = SchoolData::decode(source.as_bytes())?;
    let mut backup = None;
    if migrate || portable {
        let bytes = serde_json::to_vec(&data).map_err(|e| e.to_string())?;
        if migrate {
            let name = format!("lessons-0.6.2-{}.json", gtk::glib::uuid_string_random());
            write_json(&dir.join("backups"), &name, source.as_bytes())
                .map_err(|e| e.to_string())?;
            backup = Some(dir.join("backups").join(name));
        }
        write_json(dir, "lessons.json", &bytes).map_err(|e| e.to_string())?;
    }
    Ok((data, backup))
}

fn backup_in(directory: &Path, prefix: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    let backup = directory.join("backups");
    let name = format!(
        "{prefix}-{}-{}.json",
        chrono::Local::now().format("%Y%m%d-%H%M%S"),
        gtk::glib::uuid_string_random()
    );
    write_json(&backup, &name, bytes).map_err(|e| e.to_string())?;
    Ok(backup.join(name))
}

pub fn backup(data: &crate::lessons::SchoolData) -> Result<PathBuf, String> {
    let json = serde_json::to_vec(data).map_err(|e| e.to_string())?;
    backup_in(&data_dir(), "manual", &json)
}

fn daily_backup(directory: &Path) -> Result<(), String> {
    let backups = directory.join("backups");
    let date = chrono::Local::now().format("%Y%m%d").to_string();
    let name = format!("auto-{date}.json");
    if backups.join(&name).is_file() {
        return Ok(());
    }
    let source = match read_json_file(&directory.join("lessons.json")) {
        Ok(s) => s,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.to_string()),
    };
    crate::lessons::SchoolData::decode(source.as_bytes())?;
    write_json(&backups, &name, source.as_bytes()).map_err(|e| e.to_string())?;
    let mut previous = std::fs::read_dir(&backups)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .filter(|item| {
            let name = item.file_name();
            let name = name.to_string_lossy();
            name.strip_prefix("auto-")
                .and_then(|s| s.strip_suffix(".json"))
                .is_some_and(|date| date.len() == 8 && date.bytes().all(|b| b.is_ascii_digit()))
        })
        .map(|item| item.path())
        .collect::<Vec<_>>();
    previous.sort();
    for old in previous.iter().rev().skip(7) {
        std::fs::remove_file(old).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn save_schedule(data: &crate::lessons::SchoolData) -> Result<(), String> {
    if crate::sync_ui::WRITING_SYNC.load(std::sync::atomic::Ordering::Acquire) {
        return Err("Дождитесь завершения обмена с телефоном".into());
    }
    let directory = data_dir();
    school_day_core::lan::local_write(&directory, || {
        if crate::settings::display().auto_backup {
            daily_backup(&directory)?;
        }
        let bytes = serde_json::to_vec(data).map_err(|e| e.to_string())?;
        write_json(&directory, "lessons.json", &bytes).map_err(|e| e.to_string())
    })
}

pub fn latest_backup() -> Option<chrono::DateTime<chrono::Local>> {
    std::fs::read_dir(data_dir().join("backups"))
        .ok()?
        .filter_map(Result::ok)
        .filter(|p| p.path().extension().is_some_and(|s| s == "json"))
        .filter_map(|p| p.metadata().ok()?.modified().ok())
        .max()
        .map(Into::into)
}

pub fn read_import(path: &Path) -> Result<crate::lessons::SchoolData, String> {
    let bytes = read_json_file(path).map_err(|e| e.to_string())?;
    crate::lessons::SchoolData::decode(bytes.as_bytes()).map(|(d, _)| d)
}

pub fn device_id() -> Result<crate::lessons::RecordId, String> {
    let dir = data_dir();
    let file = dir.join("device.json");
    match read_json_file(&file) {
        Ok(bytes) => {
            serde_json::from_str::<crate::lessons::RecordId>(&bytes).map_err(|e| e.to_string())
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let id = crate::lessons::RecordId::new_v4();
            save_json("device.json", &id)?;
            Ok(id)
        }
        Err(e) => Err(e.to_string()),
    }
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
    fn reset_retains_even_a_non_utf8_original_in_a_private_backup() {
        let sandbox = Sandbox::new();
        let path = sandbox.0.join("settings.json");
        let prior = b"damaged\xff";
        write_json(&sandbox.0, "settings.json", prior).unwrap();
        let backup = reset_json_in(&sandbox.0, "settings.json", &42)
            .unwrap()
            .unwrap();
        assert_eq!(read_bounded_file(&backup).unwrap(), prior);
        assert_eq!(read_json_file(&path).unwrap(), "42");
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
    #[test]
    fn migrating_or_restoring_never_overwrites_a_good_original_on_parse_error() {
        let dir = Sandbox::new();
        let old = include_bytes!("../crates/school-day-core/tests/fixtures/school-day-062.json");
        write_json(&dir.0, "lessons.json", old).unwrap();
        let (migrated, backup) = open_schedule_in(&dir.0, None).unwrap();
        let path = backup.unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), old);
        let (again, next) = open_schedule_in(&dir.0, None).unwrap();
        assert!(next.is_none());
        assert_eq!(
            serde_json::to_vec(&migrated).unwrap(),
            serde_json::to_vec(&again).unwrap()
        );
        daily_backup(&dir.0).unwrap();
        daily_backup(&dir.0).unwrap();
        assert_eq!(std::fs::read_dir(dir.0.join("backups")).unwrap().count(), 2);
        write_json(&dir.0, "lessons.json", b"{broken").unwrap();
        assert!(open_schedule_in(&dir.0, None).is_err());
        assert_eq!(
            std::fs::read(dir.0.join("lessons.json")).unwrap(),
            b"{broken"
        );
        let recovered = read_import(&path).unwrap();
        assert_eq!(recovered.counts(), (1, 2, 1));
    }
}
