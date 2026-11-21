use school_day_core::lan::{Cancel, Outcome, Profile, SessionMode};
use std::cell::{Cell, RefCell};
use std::net::TcpListener;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::lessons::Lessons;
use crate::sync_ui::WRITING_SYNC;

const PREFERENCES: &str = "lan-preferences.json";

#[derive(Clone, Copy, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Automatic,
    #[default]
    Manual,
    Off,
}

impl Mode {
    pub const ALL: [Self; 3] = [Self::Automatic, Self::Manual, Self::Off];

    pub fn index(self) -> u32 {
        Self::ALL.iter().position(|mode| *mode == self).unwrap_or(1) as u32
    }
}

#[derive(Clone, Copy, Default, serde::Deserialize, serde::Serialize)]
#[serde(default)]
struct Preferences {
    mode: Mode,
    background: bool,
}

#[derive(Clone)]
pub struct Host(Rc<Inner>);

#[derive(Clone, Default)]
pub struct LocalChanges(Rc<RefCell<Weak<Inner>>>);

impl LocalChanges {
    pub fn saved(&self) {
        if let Some(host) = self.0.borrow().upgrade().map(Host)
            && host.mode() == Mode::Automatic
        {
            host.message("Локальные изменения ожидают обмена");
        }
    }
}

struct Inner {
    preferences: Cell<Preferences>,
    items: Lessons,
    data_guard: Arc<std::fs::File>,
    run: RefCell<Option<Cancel>>,
    paused: Cell<bool>,
    bind_retry: Cell<Duration>,
    label: gtk::glib::WeakRef<gtk::Label>,
    message: RefCell<String>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Some(cancel) = self.run.get_mut().take() {
            cancel.stop();
        }
    }
}

impl Host {
    pub fn new(items: &Lessons, changes: &LocalChanges, data_guard: Arc<std::fs::File>) -> Self {
        let loaded = crate::storage::load_json(PREFERENCES, |_: &mut Preferences| Ok(()));
        let message = loaded
            .as_ref()
            .err()
            .map(|_| "Не удалось открыть настройки обмена");
        let host = Self(Rc::new(Inner {
            preferences: Cell::new(loaded.ok().flatten().unwrap_or_default()),
            items: items.clone(),
            data_guard,
            run: RefCell::default(),
            paused: Cell::new(false),
            bind_retry: Cell::new(Duration::from_secs(3)),
            label: gtk::glib::WeakRef::new(),
            message: RefCell::new(message.unwrap_or_default().into()),
        }));
        changes.0.replace(Rc::downgrade(&host.0));
        host.start();
        host
    }

    pub fn mode(&self) -> Mode {
        self.0.preferences.get().mode
    }

    pub fn data_guard(&self) -> Arc<std::fs::File> {
        self.0.data_guard.clone()
    }

    pub fn background_available() -> bool {
        cfg!(target_os = "linux") && std::env::var_os("FLATPAK_ID").is_none()
            || cfg!(target_os = "windows")
    }

    pub fn background(&self) -> bool {
        Self::background_available()
            && self.mode() == Mode::Automatic
            && self.0.preferences.get().background
    }

    pub fn set_mode(&self, mode: Mode) -> Result<(), String> {
        let mut next = self.0.preferences.get();
        next.mode = mode;
        self.set_preferences(next)?;
        self.0.bind_retry.set(Duration::from_secs(3));
        self.message(match mode {
            Mode::Manual => "Ручной обмен",
            Mode::Off => "Синхронизация отключена",
            Mode::Automatic => "Ожидаем подключение телефона",
        });
        self.stop();
        self.start();
        Ok(())
    }

    pub fn set_background(&self, value: bool) -> Result<(), String> {
        let mut next = self.0.preferences.get();
        next.background = value;
        self.set_preferences(next)
    }

    fn set_preferences(&self, preferences: Preferences) -> Result<(), String> {
        crate::storage::save_json(PREFERENCES, &preferences)?;
        self.0.preferences.set(preferences);
        Ok(())
    }

    pub fn bind_status(&self, label: &gtk::Label) {
        label.set_text(&self.0.message.borrow());
        self.0.label.set(Some(label));
    }

    fn message(&self, message: &str) {
        self.0.message.replace(message.to_owned());
        if let Some(label) = self.0.label.upgrade() {
            label.set_text(message);
        }
    }

    pub fn pause(&self) {
        self.0.paused.set(true);
        self.stop();
    }

    pub fn resume(&self) {
        self.0.paused.set(false);
        self.start();
    }

    fn stop(&self) {
        if let Some(cancel) = self.0.run.borrow().as_ref() {
            cancel.stop();
        }
    }

    fn start(&self) {
        if self.mode() != Mode::Automatic || self.0.paused.get() || self.0.run.borrow().is_some() {
            return;
        }
        let cancel = Cancel::default();
        self.0.run.replace(Some(cancel.clone()));
        let weak = Rc::downgrade(&self.0);
        gtk::glib::spawn_future_local(async move {
            let preparing = cancel.clone();
            let ready = gtk::gio::spawn_blocking(move || {
                let profile = Profile::open(&crate::storage::data_dir())
                    .map_err(|_| ("Не удалось открыть настройки сопряжения", false))?;
                let Some(peer) = profile
                    .peer()
                    .map_err(|_| ("Подключите устройство заново", false))?
                else {
                    return Ok(None);
                };
                let listener = bind(peer.endpoint(), &preparing).map_err(|_| {
                    (
                        "Ожидаем локальную сеть. Если адрес ПК изменился, обновите его",
                        true,
                    )
                })?;
                Ok::<_, (&str, bool)>(Some((profile, listener)))
            })
            .await;
            let retry = matches!(ready, Ok(Err((_, true))));
            let mut session = match ready {
                Ok(Ok(Some(session))) => {
                    if let Some(host) = weak.upgrade().map(Host) {
                        host.0.bind_retry.set(Duration::from_secs(3));
                    }
                    Some(session)
                }
                result => {
                    if let Some(host) = weak.upgrade().map(Host)
                        && !cancel.is_cancelled()
                    {
                        host.message(match result {
                            Ok(Err((message, _))) => message,
                            Ok(Ok(None)) => "Подключите телефон",
                            _ => "Не удалось запустить обмен",
                        });
                    }
                    None
                }
            };
            let mut reload_failed = false;
            while let Some((profile, listener)) = session.take() {
                let Some(host) = weak.upgrade().map(Host).filter(|_| !cancel.is_cancelled()) else {
                    break;
                };
                let bells = host.0.items.data().bells.clone();
                let data_guard = host.data_guard();
                drop(host);
                let stop = cancel.clone();
                let exchange = gtk::gio::spawn_blocking(move || {
                    let _data_guard = data_guard;
                    let reserved = Cell::new(false);
                    let result = listener
                        .try_clone()
                        .map_err(school_day_core::lan::Error::from)
                        .and_then(|socket| {
                            profile.serve_in(
                                socket,
                                stop,
                                SessionMode::Automatic,
                                || {
                                    let accepted = !WRITING_SYNC.swap(true, Ordering::AcqRel);
                                    reserved.set(accepted);
                                    accepted
                                },
                                |_, _| None,
                            )
                        });
                    let data = reserved.get().then(|| crate::lessons::read_schedule(bells));
                    ((profile, listener), result, data)
                })
                .await;
                let Ok((resources, result, data)) = exchange else {
                    break;
                };
                let Some(host) = weak.upgrade().map(Host) else {
                    if data.is_some() {
                        WRITING_SYNC.store(false, Ordering::Release);
                    }
                    break;
                };
                if let Some(data) = data {
                    match data {
                        Ok((data, _)) => {
                            host.0.items.adopt_durable_schedule(data);
                            WRITING_SYNC.store(false, Ordering::Release);
                        }
                        Err(_) => {
                            host.message(
                                "Перезапустите приложение, чтобы открыть сохранённое расписание",
                            );
                            reload_failed = true;
                            break;
                        }
                    }
                }
                if !cancel.is_cancelled() {
                    match result {
                        Ok(Outcome::ReviewRequired) => {
                            host.message("Требуется ручная проверка изменений")
                        }
                        Ok(Outcome::Unchanged | Outcome::Synchronized | Outcome::Resumed) => {
                            host.message("Все изменения синхронизированы");
                        }
                        Err(
                            school_day_core::lan::Error::Deadline
                            | school_day_core::lan::Error::UntrustedDevice,
                        )
                        | Ok(Outcome::Cancelled) => {}
                        _ => host.message("Связь прервана. Изменения сохранены"),
                    }
                }
                session = Some(resources);
            }
            if let Some(host) = weak.upgrade().map(Host) {
                host.0.run.borrow_mut().take();
                if cancel.is_cancelled() && !reload_failed {
                    host.start();
                } else if retry {
                    let delay = host.0.bind_retry.get();
                    host.0
                        .bind_retry
                        .set((delay * 2).min(Duration::from_secs(60)));
                    let weak = Rc::downgrade(&host.0);
                    gtk::glib::timeout_add_local_once(delay, move || {
                        if let Some(host) = weak.upgrade().map(Host) {
                            host.start();
                        }
                    });
                }
            }
        });
    }
}

pub fn bind(endpoint: std::net::SocketAddrV4, cancel: &Cancel) -> std::io::Result<TcpListener> {
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        match TcpListener::bind(endpoint) {
            Err(error)
                if error.kind() == std::io::ErrorKind::AddrInUse
                    && !cancel.is_cancelled()
                    && std::time::Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(70));
            }
            result => return result,
        }
    }
}
