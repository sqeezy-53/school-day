use super::{Asset, Release, agent};
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

const MAX_INSTALLER: u64 = 512 * 1024 * 1024;

#[derive(Clone)]
struct DownloadSpec {
    url: String,
    size: u64,
    sha256: String,
}

impl Release {
    fn installer(&self) -> Result<DownloadSpec, String> {
        let version = self.version()?;
        let name = format!("school-day-{version}-windows-x64-setup.exe");
        let mut matching = self.assets.iter().filter(|asset| asset.name == name);
        let asset = matching
            .next()
            .ok_or("В релизе ещё нет установщика Windows")?;
        if matching.next().is_some() {
            return Err("В релизе несколько установщиков с одинаковым именем".into());
        }
        let url = format!(
            "https://github.com/sqeezy-53/school-day/releases/download/{}/{name}",
            self.tag_name
        );
        if asset.browser_download_url != url {
            return Err("Неверный адрес установщика".into());
        }
        DownloadSpec::from_asset(asset)
    }
}

impl DownloadSpec {
    fn from_asset(asset: &Asset) -> Result<Self, String> {
        let digest = asset
            .digest
            .as_deref()
            .and_then(|digest| digest.strip_prefix("sha256:"))
            .filter(|hash| hash.len() == 64 && hash.bytes().all(|value| value.is_ascii_hexdigit()))
            .ok_or("Для установщика не опубликована контрольная сумма SHA-256")?;
        if asset.size == 0 || asset.size > MAX_INSTALLER {
            return Err("Неверный размер установщика".into());
        }
        Ok(Self {
            url: asset.browser_download_url.clone(),
            size: asset.size,
            sha256: digest.to_ascii_lowercase(),
        })
    }
}

pub(super) struct PreparedUpdate {
    directory: PathBuf,
    installer: PathBuf,
    file: Option<File>,
    spec: DownloadSpec,
    handed_off: bool,
}

impl Drop for PreparedUpdate {
    fn drop(&mut self) {
        self.file.take();
        if self.handed_off {
            return;
        }
        let _ = std::fs::remove_file(self.directory.join("helper.exe"));
        let _ = std::fs::remove_file(&self.installer);
        let _ = std::fs::remove_dir(&self.directory);
    }
}

enum DownloadEvent {
    Progress(u64, u64),
    Finished(Result<PreparedUpdate, String>),
}

fn download(spec: DownloadSpec, sender: &Sender<DownloadEvent>) -> Result<PreparedUpdate, String> {
    let directory = std::env::temp_dir().join(format!(
        "school-day-update-{}",
        gtk::glib::uuid_string_random()
    ));
    std::fs::create_dir(&directory).map_err(|error| error.to_string())?;
    let mut prepared = PreparedUpdate {
        installer: directory.join("setup.exe"),
        directory,
        file: None,
        spec: spec.clone(),
        handed_off: false,
    };
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&prepared.installer)
        .map_err(|error| error.to_string())?;
    let mut response = agent(Duration::from_secs(15 * 60))
        .get(&spec.url)
        .call()
        .map_err(|error| error.to_string())?;
    let mut reader = response.body_mut().as_reader();
    copy_verified(&mut reader, &mut file, &spec, |received| {
        sender
            .send(DownloadEvent::Progress(received, spec.size))
            .map_err(|_| "Загрузка отменена".to_string())
    })?;
    file.sync_all().map_err(|error| error.to_string())?;
    prepared.file = Some(file);
    Ok(prepared)
}

fn copy_verified(
    reader: &mut impl Read,
    file: &mut impl Write,
    spec: &DownloadSpec,
    mut progress: impl FnMut(u64) -> Result<(), String>,
) -> Result<(), String> {
    let mut hash = Sha256::new();
    let mut received = 0;
    let mut buffer = [0; 64 * 1024];
    let mut last_progress = Instant::now();
    progress(0)?;
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        received += count as u64;
        if received > spec.size {
            return Err("Размер загруженного файла отличается от опубликованного".into());
        }
        hash.update(&buffer[..count]);
        file.write_all(&buffer[..count])
            .map_err(|error| error.to_string())?;
        if last_progress.elapsed() >= Duration::from_millis(200) {
            progress(received)?;
            last_progress = Instant::now();
        }
    }
    if received != spec.size {
        return Err("Установщик загружен не полностью".into());
    }
    if format!("{:x}", hash.finalize()) != spec.sha256 {
        return Err("Контрольная сумма установщика не совпала. Повторите загрузку.".into());
    }
    progress(received)
}

#[cfg(windows)]
thread_local! {
    static PENDING: std::cell::RefCell<Option<PreparedUpdate>> = const { std::cell::RefCell::new(None) };
}

#[cfg(windows)]
pub fn install_pending() -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let Some(mut prepared) = PENDING.with_borrow_mut(Option::take) else {
        return Ok(());
    };
    let application = installed_executable()?;
    let helper = application.with_file_name("school-day-updater.exe");
    if !helper.is_file() {
        return Err("Не найден помощник обновления. Переустановите «Школьный день».".into());
    }
    let temporary_helper = prepared.directory.join("helper.exe");
    std::fs::copy(helper, &temporary_helper)
        .map_err(|error| format!("Не удалось подготовить обновление: {error}"))?;
    std::process::Command::new(temporary_helper)
        .arg("install")
        .arg(std::process::id().to_string())
        .arg(&prepared.directory)
        .arg(application)
        .arg(prepared.spec.size.to_string())
        .arg(&prepared.spec.sha256)
        .creation_flags(0x0800_0000)
        .spawn()
        .map_err(|error| format!("Не удалось запустить обновление: {error}"))?;
    prepared.handed_off = true;
    Ok(())
}

#[cfg(windows)]
fn installed_executable() -> Result<PathBuf, String> {
    use std::os::windows::ffi::OsStringExt;
    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn RegGetValueW(
            key: *mut std::ffi::c_void,
            subkey: *const u16,
            value: *const u16,
            flags: u32,
            value_type: *mut u32,
            data: *mut std::ffi::c_void,
            bytes: *mut u32,
        ) -> i32;
    }
    let subkey: Vec<u16> = "Software\\SchoolDay"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let value: Vec<u16> = "InstallDir".encode_utf16().chain(Some(0)).collect();
    let mut path = vec![0u16; 32768];
    let mut bytes = (path.len() * 2) as u32;
    let result = unsafe {
        RegGetValueW(
            (-2147483647isize) as *mut std::ffi::c_void,
            subkey.as_ptr(),
            value.as_ptr(),
            2,
            std::ptr::null_mut(),
            path.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    if result != 0 || bytes < 2 || bytes as usize > path.len() * 2 {
        return Err("Для обновления установите приложение через установщик Windows.".into());
    }
    let length = path[..bytes as usize / 2]
        .iter()
        .position(|value| *value == 0)
        .ok_or("Не удалось прочитать папку установки")?;
    let directory = PathBuf::from(std::ffi::OsString::from_wide(&path[..length]));
    let installed = directory.join("bin/school-day.exe");
    let current = std::env::current_exe().map_err(|error| error.to_string())?;
    if !directory.join("Uninstall.exe").is_file()
        || installed.canonicalize().ok() != current.canonicalize().ok()
        || !installed.is_file()
    {
        return Err("Откройте установленную версию приложения для обновления.".into());
    }
    Ok(current)
}

#[cfg(windows)]
impl super::UpdateView {
    pub(super) fn download_or_install(self: &std::rc::Rc<Self>, button: &gtk::Button) {
        use adw::prelude::*;
        let prepared = self.prepared.borrow_mut().take();
        if let Some(prepared) = prepared {
            let Some(app) = button
                .root()
                .and_then(|root| root.downcast::<gtk::Window>().ok())
                .and_then(|window| window.application())
            else {
                *self.prepared.borrow_mut() = Some(prepared);
                self.message("Не удалось закрыть приложение для обновления.");
                return;
            };
            PENDING.with_borrow_mut(|pending| *pending = Some(prepared));
            for window in app.windows() {
                window.close();
            }
            app.quit();
            return;
        }
        if self.busy.get() {
            return;
        }
        if let Err(error) = installed_executable() {
            self.message(&error);
            return;
        }
        let spec = match self.release.borrow().as_ref().map(Release::installer) {
            Some(Ok(spec)) => spec,
            Some(Err(error)) => {
                self.message(&error);
                return;
            }
            None => return,
        };
        let restore_focus = button.has_focus();
        self.set_busy(true);
        self.message("Загружаем обновление…");
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = download(spec, &sender);
            let _ = sender.send(DownloadEvent::Finished(result));
        });
        let weak = std::rc::Rc::downgrade(self);
        gtk::glib::timeout_add_local(Duration::from_millis(100), move || {
            let Some(view) = weak.upgrade() else {
                return gtk::glib::ControlFlow::Break;
            };
            loop {
                match receiver.try_recv() {
                    Ok(DownloadEvent::Progress(received, total)) => {
                        if let Some(progress) = view.progress.upgrade() {
                            progress.set_visible(true);
                            progress.set_fraction(received as f64 / total as f64);
                            progress.set_text(Some(&format!("{}%", received * 100 / total)));
                        }
                    }
                    Ok(DownloadEvent::Finished(result)) => {
                        view.set_busy(false);
                        if let Some(progress) = view.progress.upgrade() {
                            progress.set_visible(false);
                        }
                        match result {
                            Ok(prepared) => {
                                *view.prepared.borrow_mut() = Some(prepared);
                                if let Some(button) = view.action.upgrade() {
                                    button.set_label("Установить и перезапустить");
                                }
                                if let Some(check) = view.check.upgrade() {
                                    check.set_sensitive(false);
                                }
                                view.message("Обновление скачано и проверено. Можно установить.");
                            }
                            Err(error) => {
                                view.message(&format!("Не удалось загрузить обновление: {error}"))
                            }
                        }
                        super::restore_button_focus(&view.action, restore_focus);
                        return gtk::glib::ControlFlow::Break;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        return gtk::glib::ControlFlow::Continue;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        view.set_busy(false);
                        if let Some(progress) = view.progress.upgrade() {
                            progress.set_visible(false);
                        }
                        view.message("Загрузка прервалась. Повторите попытку.");
                        super::restore_button_focus(&view.action, restore_focus);
                        return gtk::glib::ControlFlow::Break;
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_verification_rejects_truncated_changed_and_oversized_contents() {
        let data = b"installer";
        let spec = DownloadSpec {
            url: String::new(),
            size: data.len() as u64,
            sha256: format!("{:x}", Sha256::digest(data)),
        };
        let mut output = Vec::new();
        copy_verified(&mut data.as_slice(), &mut output, &spec, |_| Ok(())).unwrap();
        assert_eq!(output, data);
        for mut bad in [
            b"installe".as_slice(),
            b"installeX",
            b"something much longer",
        ] {
            assert!(copy_verified(&mut bad, &mut Vec::new(), &spec, |_| Ok(())).is_err());
        }
    }

    #[test]
    fn installer_must_have_the_expected_repository_url_name_digest_and_size() {
        let name = "school-day-0.4.0-windows-x64-setup.exe";
        let asset = Asset {
            name: name.into(),
            browser_download_url: format!(
                "https://github.com/sqeezy-53/school-day/releases/download/v0.4.0/{name}"
            ),
            size: 100,
            digest: Some(format!("sha256:{}", "a".repeat(64))),
        };
        let mut release = Release {
            tag_name: "v0.4.0".into(),
            body: None,
            draft: false,
            prerelease: false,
            assets: vec![asset.clone()],
        };
        assert!(release.installer().is_ok());
        release.assets[0].browser_download_url = "https://example.com/setup.exe".into();
        assert!(release.installer().is_err());
        release.assets[0] = asset.clone();
        release.assets[0].digest = None;
        assert!(release.installer().is_err());
        release.assets[0] = asset.clone();
        release.assets[0].size = MAX_INSTALLER + 1;
        assert!(release.installer().is_err());
        release.assets = vec![asset.clone(), asset];
        assert!(release.installer().is_err());
    }
}
