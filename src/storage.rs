use serde::{Serialize, de::DeserializeOwned};
use std::path::PathBuf;

pub fn data_dir() -> PathBuf {
    gtk::glib::user_data_dir().join("school-day")
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
    let directory = data_dir();
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let target = directory.join(name);
    let temporary = target.with_extension("json.tmp");
    std::fs::write(&temporary, json).map_err(|error| error.to_string())?;
    std::fs::rename(&temporary, &target).map_err(|error| error.to_string())
}
