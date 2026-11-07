use adw::prelude::*;
use semver::Version;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use crate::{column, notify, primary, text};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::install_pending;

const API: &str = "https://api.github.com/repos/sqeezy-53/school-day/releases/latest";
#[cfg(not(windows))]
const RELEASES: &str = "https://github.com/sqeezy-53/school-day/releases";
const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, serde::Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    draft: bool,
    prerelease: bool,
    #[cfg(windows)]
    assets: Vec<Asset>,
}

#[cfg(windows)]
#[derive(Clone, Debug, serde::Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    size: u64,
    digest: Option<String>,
}

impl Release {
    fn version(&self) -> Result<Version, String> {
        let version = self.tag_name.strip_prefix('v').unwrap_or(&self.tag_name);
        let version = Version::parse(version).map_err(|_| "Неверный номер версии в релизе")?;
        if self.draft || self.prerelease || !version.pre.is_empty() || !version.build.is_empty() {
            return Err("Ожидался опубликованный стабильный релиз".into());
        }
        Ok(version)
    }

    #[cfg(not(windows))]
    fn url(&self) -> String {
        format!("{RELEASES}/tag/{}", self.tag_name)
    }
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .https_only(true)
        .user_agent(format!("SchoolDay/{VERSION}"))
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_global(Some(timeout))
        .max_redirects(5)
        .build()
        .into()
}

fn latest() -> Result<Release, String> {
    let mut response = agent(Duration::from_secs(30))
        .get(API)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|error| error.to_string())?;
    let body = response
        .body_mut()
        .with_config()
        .limit(512 * 1024)
        .read_to_vec()
        .map_err(|error| error.to_string())?;
    let release: Release = serde_json::from_slice(&body).map_err(|error| error.to_string())?;
    release.version()?;
    Ok(release)
}

pub fn section(
    automatic: bool,
    on_change: impl Fn(bool) -> Result<(), String> + 'static,
) -> gtk::Box {
    let root = column(12);
    root.add_css_class("settings-section");
    root.append(&text("Обновления", "settings-title"));
    root.append(&text(&format!("Установлена версия {VERSION}"), "section"));
    let automatic_check = gtk::CheckButton::new();
    automatic_check.set_child(Some(&text("Проверять обновления автоматически", "section")));
    automatic_check.set_active(automatic);
    root.append(&automatic_check);
    let check = primary("Проверить обновления");
    check.set_halign(gtk::Align::Start);
    root.append(&check);
    let spinner = gtk::Spinner::new();
    spinner.set_halign(gtk::Align::Start);
    spinner.set_visible(false);
    root.append(&spinner);
    let feedback = text("", "section");
    feedback.set_visible(false);
    root.append(&feedback);
    let notes = text("", "muted");
    notes.set_max_width_chars(64);
    let notes_scroll = gtk::ScrolledWindow::new();
    notes_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    notes_scroll.set_propagate_natural_height(true);
    notes_scroll.set_max_content_height(180);
    notes_scroll.set_child(Some(&notes));
    notes_scroll.set_visible(false);
    let action = primary(if cfg!(windows) {
        "Скачать обновление"
    } else {
        "Открыть загрузку"
    });
    action.set_halign(gtk::Align::Start);
    action.set_visible(false);
    root.append(&action);
    #[cfg(windows)]
    let progress = {
        let progress = gtk::ProgressBar::new();
        progress.set_show_text(true);
        progress.set_visible(false);
        root.append(&progress);
        progress
    };
    root.append(&notes_scroll);
    #[cfg(not(windows))]
    root.append(&text(
        "Новую сборку Flatpak можно скачать и открыть для установки.",
        "muted",
    ));

    let view = Rc::new(UpdateView {
        check: check.downgrade(),
        action: action.downgrade(),
        spinner: spinner.downgrade(),
        feedback: feedback.downgrade(),
        notes: notes.downgrade(),
        notes_scroll: notes_scroll.downgrade(),
        #[cfg(windows)]
        progress: progress.downgrade(),
        automatic: automatic_check.downgrade(),
        busy: Cell::new(false),
        release: RefCell::new(None),
        #[cfg(windows)]
        prepared: RefCell::new(None),
    });
    let checking = Rc::clone(&view);
    check.connect_clicked(move |_| checking.check());
    let acting = Rc::clone(&view);
    action.connect_clicked(move |button| {
        #[cfg(windows)]
        acting.download_or_install(button);
        #[cfg(not(windows))]
        {
            button.set_sensitive(false);
            let url = acting.release.borrow().as_ref().map(Release::url);
            if let Some(url) = url {
                let parent = button
                    .root()
                    .and_then(|root| root.downcast::<gtk::Window>().ok());
                let context = parent
                    .map(|parent| gtk::prelude::WidgetExt::display(&parent).app_launch_context());
                let weak_button = button.downgrade();
                let weak_view = Rc::downgrade(&acting);
                gtk::gio::AppInfo::launch_default_for_uri_async(
                    &url,
                    context.as_ref(),
                    None::<&gtk::gio::Cancellable>,
                    move |result| {
                        if let Some(button) = weak_button.upgrade() {
                            button.set_sensitive(true);
                        }
                        if result.is_err()
                            && let Some(view) = weak_view.upgrade()
                        {
                            view.message("Не удалось открыть браузер. Повторите попытку.");
                        }
                    },
                );
            } else {
                button.set_sensitive(true);
            }
        }
    });
    let saving = Rc::clone(&view);
    let reverting = Cell::new(false);
    automatic_check.connect_toggled(move |button| {
        if reverting.get() {
            return;
        }
        match on_change(button.is_active()) {
            Ok(()) => {
                if button.is_active() {
                    saving.check();
                }
            }
            Err(error) => {
                reverting.set(true);
                button.set_active(!button.is_active());
                reverting.set(false);
                saving.message(&format!("Не удалось сохранить настройку: {error}"));
            }
        }
    });
    let weak_view = Rc::downgrade(&view);
    gtk::glib::timeout_add_seconds_local(24 * 60 * 60, move || {
        let Some(view) = weak_view.upgrade() else {
            return gtk::glib::ControlFlow::Break;
        };
        if view
            .automatic
            .upgrade()
            .is_some_and(|button| button.is_active())
        {
            view.check();
        }
        gtk::glib::ControlFlow::Continue
    });
    if automatic {
        gtk::glib::idle_add_local_once(move || view.check());
    }
    root
}

struct UpdateView {
    check: gtk::glib::WeakRef<gtk::Button>,
    action: gtk::glib::WeakRef<gtk::Button>,
    spinner: gtk::glib::WeakRef<gtk::Spinner>,
    feedback: gtk::glib::WeakRef<gtk::Label>,
    notes: gtk::glib::WeakRef<gtk::Label>,
    notes_scroll: gtk::glib::WeakRef<gtk::ScrolledWindow>,
    #[cfg(windows)]
    progress: gtk::glib::WeakRef<gtk::ProgressBar>,
    automatic: gtk::glib::WeakRef<gtk::CheckButton>,
    busy: Cell<bool>,
    release: RefCell<Option<Release>>,
    #[cfg(windows)]
    prepared: RefCell<Option<windows::PreparedUpdate>>,
}

fn restore_button_focus(button: &gtk::glib::WeakRef<gtk::Button>, requested: bool) {
    if requested
        && let Some(button) = button.upgrade().filter(|button| button.is_mapped())
        && let Some(window) = button
            .root()
            .and_then(|root| root.downcast::<gtk::Window>().ok())
        && gtk::prelude::GtkWindowExt::focus(&window).is_none()
    {
        button.grab_focus();
    }
}

impl UpdateView {
    fn message(&self, value: &str) {
        if let Some(feedback) = self.feedback.upgrade() {
            notify(&feedback, value);
        }
    }

    fn set_busy(&self, busy: bool) {
        self.busy.set(busy);
        if let Some(check) = self.check.upgrade() {
            check.set_sensitive(!busy);
        }
        if let Some(action) = self.action.upgrade() {
            action.set_sensitive(!busy);
        }
        if let Some(spinner) = self.spinner.upgrade() {
            spinner.set_spinning(busy);
            spinner.set_visible(busy);
        }
    }

    fn check(self: &Rc<Self>) {
        if self.busy.get() {
            return;
        }
        #[cfg(windows)]
        if self.prepared.borrow().is_some() {
            return;
        }
        let restore_focus = self
            .check
            .upgrade()
            .is_some_and(|button| button.has_focus());
        self.set_busy(true);
        self.message("Проверяем обновления…");
        if let Some(action) = self.action.upgrade() {
            action.set_visible(false);
        }
        if let Some(scroll) = self.notes_scroll.upgrade() {
            scroll.set_visible(false);
        }
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(latest());
        });
        let weak = Rc::downgrade(self);
        gtk::glib::timeout_add_local(Duration::from_millis(100), move || {
            let Some(view) = weak.upgrade() else {
                return gtk::glib::ControlFlow::Break;
            };
            match receiver.try_recv() {
                Ok(result) => {
                    view.set_busy(false);
                    match result {
                        Ok(release) => view.show_release(release),
                        Err(error) => {
                            eprintln!("Проверка обновлений: {error}");
                            view.message("Не удалось проверить обновления. Проверьте подключение и повторите попытку.");
                        }
                    }
                }
                Err(mpsc::TryRecvError::Empty) => return gtk::glib::ControlFlow::Continue,
                Err(mpsc::TryRecvError::Disconnected) => {
                    view.set_busy(false);
                    view.message("Проверка прервалась. Повторите попытку.");
                }
            }
            restore_button_focus(&view.check, restore_focus);
            gtk::glib::ControlFlow::Break
        });
    }

    fn show_release(&self, release: Release) {
        let current = Version::parse(VERSION).expect("Некорректная версия приложения");
        let Ok(version) = release.version() else {
            self.message("Не удалось прочитать версию обновления.");
            return;
        };
        if version <= current {
            self.message("Установлена последняя версия");
            self.release.borrow_mut().take();
            return;
        }
        self.message(&format!("Доступна версия {version}"));
        if let Some(notes) = self.notes.upgrade() {
            let body: String = release
                .body
                .as_deref()
                .unwrap_or("Описание обновления появится на странице релиза.")
                .chars()
                .take(6000)
                .collect();
            notes.set_text(&body);
        }
        if let Some(scroll) = self.notes_scroll.upgrade() {
            scroll.set_visible(true);
            scroll.vadjustment().set_value(0.0);
        }
        if let Some(action) = self.action.upgrade() {
            action.set_visible(true);
        }
        *self.release.borrow_mut() = Some(release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_versions_are_compared_numerically_and_preview_tags_are_rejected() {
        let release = |tag| Release {
            tag_name: tag,
            body: None,
            draft: false,
            prerelease: false,
            #[cfg(windows)]
            assets: Vec::new(),
        };
        assert!(release("v0.10.0".into()).version().unwrap() > Version::new(0, 9, 0));
        for tag in ["../../bad", "v0.4.0-beta.1", "v0.4.0+build", "0.4"] {
            assert!(release(tag.into()).version().is_err());
        }
        let mut preview = release("v0.4.0".into());
        preview.prerelease = true;
        assert!(preview.version().is_err());
        preview.prerelease = false;
        preview.draft = true;
        assert!(preview.version().is_err());
    }
}
