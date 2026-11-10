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

pub fn indicator(
    automatic: bool,
    on_change: impl Fn(bool) -> Result<(), String> + 'static,
) -> gtk::MenuButton {
    let indicator = gtk::MenuButton::new();
    indicator.set_direction(gtk::ArrowType::Up);
    let icon = gtk::DrawingArea::new();
    icon.set_content_width(16);
    icon.set_content_height(16);
    icon.set_halign(gtk::Align::Center);
    icon.set_valign(gtk::Align::Center);
    icon.set_draw_func(|icon, context, _, _| {
        let color = icon.style_context().color();
        context.set_source_rgba(
            color.red().into(),
            color.green().into(),
            color.blue().into(),
            color.alpha().into(),
        );
        context.set_line_width(1.7);
        context.set_line_cap(gtk::cairo::LineCap::Round);
        context.set_line_join(gtk::cairo::LineJoin::Round);
        context.move_to(8.0, 2.0);
        context.line_to(8.0, 10.0);
        context.move_to(4.5, 6.5);
        context.line_to(8.0, 10.0);
        context.line_to(11.5, 6.5);
        context.move_to(3.0, 11.0);
        context.line_to(3.0, 14.0);
        context.line_to(13.0, 14.0);
        context.line_to(13.0, 11.0);
        let _ = context.stroke();
    });
    indicator.set_child(Some(&icon));
    indicator.set_tooltip_text(Some("Обновления приложения"));
    indicator.add_css_class("circular");
    indicator.add_css_class("flat");
    indicator.add_css_class("update-indicator");
    indicator.set_halign(gtk::Align::Start);
    indicator.set_valign(gtk::Align::Center);
    let popover = gtk::Popover::new();
    popover.set_has_arrow(false);
    popover.add_css_class("updates-popover");
    popover.set_halign(gtk::Align::Start);
    popover.set_offset(0, -4);
    let root = column(10);
    root.set_width_request(272);
    crate::margins(&root, 16);
    root.add_css_class("update-card");
    popover.set_child(Some(&root));
    indicator.set_popover(Some(&popover));
    let heading = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let title = text("Обновления", "settings-title");
    title.set_hexpand(true);
    heading.append(&title);
    let check = gtk::Button::from_icon_name("view-refresh-symbolic");
    check.add_css_class("flat");
    check.add_css_class("circular");
    check.set_tooltip_text(Some("Проверить обновления"));
    heading.append(&check);
    root.append(&heading);
    root.append(&text(&format!("Версия {VERSION}"), "section"));
    let automatic_row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let automatic_label = text("Автопроверка", "section");
    automatic_label.set_hexpand(true);
    let automatic_check = gtk::Switch::new();
    automatic_check.set_valign(gtk::Align::Center);
    automatic_label.set_mnemonic_widget(Some(&automatic_check));
    automatic_check.set_active(automatic);
    automatic_row.append(&automatic_label);
    automatic_row.append(&automatic_check);
    root.append(&automatic_row);
    let spinner = gtk::Spinner::new();
    spinner.set_halign(gtk::Align::Start);
    spinner.set_visible(false);
    root.append(&spinner);
    let feedback = text("", "section");
    feedback.set_visible(false);
    feedback.set_max_width_chars(32);
    root.append(&feedback);
    let notes = text("", "section");
    notes.set_max_width_chars(32);
    let notes_scroll = gtk::ScrolledWindow::new();
    notes_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    notes_scroll.set_propagate_natural_height(true);
    notes_scroll.set_max_content_height(140);
    let weak_notes_scroll = notes_scroll.downgrade();
    let weak_indicator = indicator.downgrade();
    popover.connect_map(move |_| {
        if let (Some(notes), Some(indicator)) =
            (weak_notes_scroll.upgrade(), weak_indicator.upgrade())
        {
            let height = indicator
                .root()
                .and_then(|root| root.downcast::<gtk::Window>().ok())
                .map_or(700, |root| root.height());
            notes.set_max_content_height((height - 300).clamp(40, 140));
        }
    });
    notes_scroll.set_child(Some(&notes));
    notes_scroll.set_visible(false);
    let action = primary(if cfg!(windows) {
        "Скачать обновление"
    } else {
        "Открыть релиз"
    });
    action.set_halign(gtk::Align::Fill);
    action.set_visible(false);
    #[cfg(windows)]
    let progress = {
        let progress = gtk::ProgressBar::new();
        progress.set_show_text(true);
        progress.set_visible(false);
        root.append(&progress);
        progress
    };
    root.append(&notes_scroll);
    root.append(&action);

    let view = Rc::new(UpdateView {
        indicator: indicator.downgrade(),
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
    let saved_automatic = Cell::new(automatic);
    automatic_check.connect_active_notify(move |button| {
        if button.is_active() == saved_automatic.get() {
            return;
        }
        match on_change(button.is_active()) {
            Ok(()) => {
                saved_automatic.set(button.is_active());
                if button.is_active() {
                    saving.check();
                }
            }
            Err(error) => {
                button.set_active(saved_automatic.get());
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
    indicator
}

struct UpdateView {
    indicator: gtk::glib::WeakRef<gtk::MenuButton>,
    check: gtk::glib::WeakRef<gtk::Button>,
    action: gtk::glib::WeakRef<gtk::Button>,
    spinner: gtk::glib::WeakRef<gtk::Spinner>,
    feedback: gtk::glib::WeakRef<gtk::Label>,
    notes: gtk::glib::WeakRef<gtk::Label>,
    notes_scroll: gtk::glib::WeakRef<gtk::ScrolledWindow>,
    #[cfg(windows)]
    progress: gtk::glib::WeakRef<gtk::ProgressBar>,
    automatic: gtk::glib::WeakRef<gtk::Switch>,
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
        if let Some(indicator) = self.indicator.upgrade() {
            indicator.set_tooltip_text(Some(value));
        }
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
            if let Some(indicator) = self.indicator.upgrade() {
                indicator.remove_css_class("update-available");
            }
            self.release.borrow_mut().take();
            return;
        }
        self.message(&format!("Доступна версия {version}"));
        if let Some(indicator) = self.indicator.upgrade() {
            indicator.add_css_class("update-available");
        }
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
    #[ignore = "Нужен графический сеанс; запускать отдельно с --ignored --test-threads=1"]
    fn failed_update_preference_save_reverts_once() {
        adw::init().unwrap();
        let attempts = Rc::new(Cell::new(0));
        let recorded = Rc::clone(&attempts);
        let indicator = indicator(false, move |_| {
            recorded.set(recorded.get() + 1);
            Err("Нет доступа для записи".into())
        });
        let mut widgets = vec![indicator.popover().unwrap().upcast::<gtk::Widget>()];
        let mut automatic = None;
        while let Some(widget) = widgets.pop() {
            if let Some(switch) = widget.downcast_ref::<gtk::Switch>() {
                automatic = Some(switch.clone());
                break;
            }
            let mut child = widget.first_child();
            while let Some(current) = child {
                child = current.next_sibling();
                widgets.push(current);
            }
        }
        let automatic = automatic.expect("Нет переключателя автопроверки");
        for expected in [1, 2] {
            automatic.set_active(true);
            assert!(!automatic.is_active());
            assert_eq!(attempts.get(), expected);
        }
    }

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
