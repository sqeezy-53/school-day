use adw::prelude::*;
use gtk::{gio, glib};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};
const BUS: &str = "org.freedesktop.portal.Flatpak";
const RESTART: &str = "Перезапустите приложение";
const INSTALL_FAILED: &str = "Не удалось установить обновление";
const MONITOR: &str = "org.freedesktop.portal.Flatpak.UpdateMonitor";
type Options = HashMap<String, glib::Variant>;

struct Monitor {
    bus: gio::DBusConnection,
    path: String,
    _signals: gio::SignalSubscription,
}
impl Drop for Monitor {
    fn drop(&mut self) {
        self.bus.call(
            Some(BUS),
            &self.path,
            MONITOR,
            "Close",
            None,
            None,
            gio::DBusCallFlags::NONE,
            3000,
            None::<&gio::Cancellable>,
            |_| {},
        );
    }
}
async fn watch(
    label: gtk::glib::WeakRef<gtk::Label>,
    install: gtk::glib::WeakRef<gtk::Button>,
    dot: gtk::glib::WeakRef<gtk::Box>,
    current: Rc<Cell<u64>>,
    ticket: u64,
) -> Result<Monitor, String> {
    let bus = gio::bus_get_future(gio::BusType::Session)
        .await
        .map_err(|e| e.to_string())?;
    let name = bus
        .unique_name()
        .ok_or("Нет соединения с системным порталом")?;
    let token = format!("sd{}", glib::uuid_string_random().replace('-', ""));
    let path = format!(
        "/org/freedesktop/portal/Flatpak/update_monitor/{}/{}",
        name.trim_start_matches(':').replace('.', "_"),
        token
    );
    let signals = bus.subscribe_to_signal(
        Some(BUS),
        Some(MONITOR),
        None,
        Some(&path),
        None,
        gio::DBusSignalFlags::NONE,
        move |signal| {
            if current.get() != ticket {
                return;
            }
            let (Some(label), Some(install), Some((info,))) = (
                label.upgrade(),
                install.upgrade(),
                signal.parameters.get::<(Options,)>(),
            ) else {
                return;
            };
            match signal.signal_name {
                "UpdateAvailable" => {
                    let field = |key| {
                        info.get(key)
                            .and_then(|v| v.get::<String>())
                            .filter(|s| !s.is_empty())
                    };
                    let running = field("running-commit");
                    let local = field("local-commit");
                    let remote = field("remote-commit");
                    let available = remote.is_some() && remote != local;
                    install.set_visible(available);
                    install.set_sensitive(true);
                    label.set_label(if available {
                        "Доступно обновление"
                    } else if let (Some(running), Some(local)) = (running, local)
                        && running != local
                    {
                        RESTART
                    } else {
                        super::UP_TO_DATE
                    });
                    set_badge(&dot, available);
                }
                "Progress" => match info.get("status").and_then(|s| s.get::<u32>()) {
                    Some(0) => label.set_label("Устанавливаем обновление…"),
                    Some(status @ (1 | 2)) => {
                        label.set_label(if status == 2 {
                            RESTART
                        } else {
                            super::UP_TO_DATE
                        });
                        install.set_visible(false);
                        set_badge(&dot, false);
                    }
                    Some(3) => {
                        crate::storage::log_update_error(&format!(
                            "Установка обновления Flatpak: {info:?}"
                        ));
                        label.set_label(INSTALL_FAILED);
                        install.set_sensitive(true);
                    }
                    _ => {}
                },
                _ => {}
            }
        },
    );
    let mut options = Options::new();
    options.insert("handle_token".into(), token.to_variant());
    let reply = bus
        .call_future(
            Some(BUS),
            "/org/freedesktop/portal/Flatpak",
            BUS,
            "CreateUpdateMonitor",
            Some(&(options,).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            10000,
        )
        .await
        .map_err(|e| e.to_string())?;
    if reply.child_value(0).str() != Some(&path) {
        return Err("Неожиданный ответ портала Flatpak".into());
    }
    Ok(Monitor {
        bus,
        path,
        _signals: signals,
    })
}
fn set_badge(dot: &glib::WeakRef<gtk::Box>, available: bool) {
    if let Some(dot) = dot.upgrade() {
        dot.set_visible(available && crate::settings::display().update_badge);
    }
}

pub fn indicator(
    automatic: bool,
    badge: bool,
    save: impl Fn(super::Preference, bool) -> Result<(), String> + 'static,
) -> gtk::MenuButton {
    let (button, dot) = super::indicator_button();
    let (_, body, check) = super::popover_parts(&button, &format!("Версия {}", super::VERSION));
    let preferences = crate::column(4);
    let auto = super::preference_switch(&preferences, "Проверять при запуске", automatic);
    body.append(&preferences);
    let feedback = crate::text("", "muted");
    feedback.set_visible(false);
    feedback.set_max_width_chars(32);
    body.append(&feedback);
    let install = gtk::Button::with_label("Установить");
    install.set_visible(false);
    body.append(&install);
    let save = Rc::new(save);
    let save_badge = Rc::clone(&save);
    let update = install.downgrade();
    let badge_dot = dot.downgrade();
    super::append_badge_switch(
        &preferences,
        badge,
        move |value| {
            save_badge(super::Preference::Badge, value)?;
            set_badge(
                &badge_dot,
                update.upgrade().is_some_and(|button| button.is_visible()),
            );
            Ok(())
        },
        &feedback,
    );
    let state = Rc::new(RefCell::new(None::<Monitor>));
    let generation = Rc::new(Cell::new(0u64));
    let (label, action, icon) = (feedback.downgrade(), install.downgrade(), dot.downgrade());
    let (holder, counter) = (state.clone(), generation.clone());
    let start = Rc::new(move || {
        holder.borrow_mut().take();
        counter.set(counter.get().wrapping_add(1));
        let ticket = counter.get();
        if let Some(label) = label.upgrade() {
            crate::notify(&label, super::CHECKING);
        }
        set_badge(&icon, false);
        if let Some(action) = action.upgrade() {
            action.set_visible(false);
        }
        let (holder, counter, label, action, icon) = (
            holder.clone(),
            counter.clone(),
            label.clone(),
            action.clone(),
            icon.clone(),
        );
        glib::spawn_future_local(async move {
            let result = watch(label.clone(), action, icon, counter.clone(), ticket).await;
            if ticket != counter.get() {
                return;
            }
            match result {
                Ok(m) => {
                    *holder.borrow_mut() = Some(m);
                    glib::timeout_add_seconds_local_once(12, move || {
                        if counter.get() == ticket
                            && let Some(label) =
                                label.upgrade().filter(|l| l.label() == super::CHECKING)
                        {
                            eprintln!(
                                "Проверка обновлений Flatpak: портал не прислал ответ за 12 секунд"
                            );
                            label.set_label(super::CHECK_FAILED);
                        }
                    });
                }
                Err(e) => {
                    crate::storage::log_update_error(&format!("Проверка обновлений Flatpak: {e}"));
                    if let Some(label) = label.upgrade() {
                        label.set_label(super::CHECK_FAILED);
                    }
                }
            }
        });
    });
    let click = start.clone();
    check.connect_clicked(move |_| click());
    if automatic {
        start();
    }
    let holder = state.clone();
    let weak = feedback.downgrade();
    let auto_dot = dot.downgrade();
    let saved = Cell::new(automatic);
    let auto_action = install.downgrade();
    auto.connect_active_notify(move |check| {
        let value = check.is_active();
        if value == saved.get() {
            return;
        }
        match save(super::Preference::Automatic, value) {
            Ok(()) => {
                saved.set(value);
                if value {
                    start();
                } else {
                    generation.set(generation.get().wrapping_add(1));
                    holder.borrow_mut().take();
                    if let Some(b) = auto_action.upgrade() {
                        b.set_visible(false);
                    }
                    set_badge(&auto_dot, false);
                    if let Some(l) = weak.upgrade() {
                        l.set_visible(false);
                    }
                }
            }
            Err(e) => {
                crate::storage::log_update_error(&format!(
                    "Настройка проверки обновлений Flatpak: {e}"
                ));
                check.set_active(saved.get());
                if let Some(l) = weak.upgrade() {
                    crate::notify(&l, super::SAVE_FAILED);
                }
            }
        }
    });
    let weak_label = feedback.downgrade();
    install.connect_clicked(move |button| {
        let state = state.borrow();
        let Some(m) = state.as_ref() else {
            return;
        };
        button.set_sensitive(false);
        let (label, action) = (weak_label.clone(), button.downgrade());
        m.bus.call(
            Some(BUS),
            &m.path,
            MONITOR,
            "Update",
            Some(&("", Options::new()).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            30000,
            None::<&gio::Cancellable>,
            move |result| {
                if let Err(e) = result {
                    crate::storage::log_update_error(&format!(
                        "Запрос установки обновления Flatpak: {e}"
                    ));
                    if let Some(label) = label.upgrade() {
                        label.set_label(INSTALL_FAILED);
                    }
                    if let Some(b) = action.upgrade() {
                        b.set_sensitive(true);
                    }
                }
            },
        );
    });
    button
}
