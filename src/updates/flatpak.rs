use adw::prelude::*;
use gtk::{gio, glib};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};
const BUS: &str = "org.freedesktop.portal.Flatpak";
const WAITING: &str = "Ожидание сведений от источника Flatpak…";
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
    indicator: gtk::glib::WeakRef<gtk::MenuButton>,
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
    let signals=bus.subscribe_to_signal(
        Some(BUS),Some(MONITOR),None,Some(&path),None,gio::DBusSignalFlags::NONE,
        move |signal|{
            if current.get()!=ticket{return;}
            let (Some(label),Some(install),Some((info,)))=(label.upgrade(),install.upgrade(),signal.parameters.get::<(Options,)>()) else{return;};
            match signal.signal_name {
                "UpdateAvailable" => {
                    let field=|key|info.get(key).and_then(|v|v.get::<String>()).filter(|s|!s.is_empty());
                    let running=field("running-commit");
                    let local=field("local-commit");
                    let remote=field("remote-commit");
                    let available=remote.is_some() && remote!=local;
                    install.set_visible(available);
                    install.set_sensitive(true);
                    label.set_label(if available {
                        "Доступно обновление из источника Flatpak"
                    }else if let (Some(running),Some(local))=(running,local) && running!=local {
                        "Обновление установлено. Перезапустите приложение."
                    }else{
                        "Flatpak пока не сообщил о доступном обновлении."
                    });
                    set_icon(&indicator,available);
                }
                "Progress" => {
                    match info.get("status").and_then(|s|s.get::<u32>()){
                        Some(0) => label.set_label("Flatpak устанавливает обновление…"),
                        Some(status @ (1|2)) => {
                            label.set_label(if status==2 {"Обновление установлено. Перезапустите приложение."}else{"Обновлений для установки нет."});
                            install.set_visible(false);
                            set_icon(&indicator,false);
                        }
                        Some(3) => {
                            label.set_label("Flatpak не завершил обновление. Проверьте интернет и источник в центре приложений.");
                            install.set_sensitive(true);
                        }
                        _=>{}
                    }
                }
                _=>{}
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
fn set_icon(button: &glib::WeakRef<gtk::MenuButton>, available: bool) {
    if let Some(button) = button.upgrade() {
        button.set_icon_name(if available && crate::settings::display().update_badge {
            "software-update-available-symbolic"
        } else {
            "folder-download-symbolic"
        });
    }
}

pub fn indicator(
    automatic: bool,
    badge: bool,
    save: impl Fn(super::Preference, bool) -> Result<(), String> + 'static,
) -> gtk::MenuButton {
    let button = gtk::MenuButton::builder()
        .icon_name("folder-download-symbolic")
        .tooltip_text("Обновления Flatpak")
        .build();
    button.set_halign(gtk::Align::Start);
    button.add_css_class("flat");
    button.add_css_class("update-indicator");
    let (_, body, check) =
        super::popover_parts(&button, &format!("Версия {} · Flatpak", super::VERSION));
    let preferences = crate::column(4);
    let auto = super::preference_switch(&preferences, "Проверять при запуске", automatic);
    body.append(&preferences);
    let feedback = crate::text(
        "Обновления поступают из подключённого источника Flatpak.",
        "muted",
    );
    feedback.set_max_width_chars(32);
    body.append(&feedback);
    let install = gtk::Button::with_label("Установить через Flatpak");
    install.set_visible(false);
    body.append(&install);
    let save = Rc::new(save);
    let save_badge = Rc::clone(&save);
    let update = install.downgrade();
    let badge_button = button.downgrade();
    super::append_badge_switch(
        &preferences,
        badge,
        move |value| {
            save_badge(super::Preference::Badge, value)?;
            set_icon(
                &badge_button,
                update.upgrade().is_some_and(|button| button.is_visible()),
            );
            Ok(())
        },
        &feedback,
    );
    let state = Rc::new(RefCell::new(None::<Monitor>));
    let generation = Rc::new(Cell::new(0u64));
    let (label, action, icon) = (
        feedback.downgrade(),
        install.downgrade(),
        button.downgrade(),
    );
    let (holder, counter) = (state.clone(), generation.clone());
    let start = Rc::new(move || {
        holder.borrow_mut().take();
        counter.set(counter.get().wrapping_add(1));
        let ticket = counter.get();
        if let Some(label) = label.upgrade() {
            label.set_label(WAITING);
            set_icon(&icon, false);
        }
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
                            && let Some(label) = label.upgrade().filter(|l| l.label() == WAITING)
                        {
                            label.set_label("Источник пока не ответил. Проверьте подключение и источник в центре приложений.");
                        }
                    });
                }
                Err(e) => {
                    if let Some(label) = label.upgrade() {
                        label.set_label(&format!("Источник обновлений недоступен: {e}"));
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
                    if let Some(l) = weak.upgrade() {
                        l.set_label("Автоматическая проверка выключена");
                    }
                }
            }
            Err(e) => {
                check.set_active(saved.get());
                if let Some(l) = weak.upgrade() {
                    l.set_label(&e);
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
                    if let Some(label) = label.upgrade() {
                        label.set_label(&e.to_string());
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
