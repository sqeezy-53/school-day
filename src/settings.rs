use adw::prelude::*;
use chrono::NaiveTime;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::forms::{self, CLASS_LIMIT};
use crate::tutorial::Anchor;
use crate::{column, notify, primary, text};

pub use school_day_core::settings::*;
pub fn load_settings() -> Result<Settings, String> {
    Ok(
        crate::storage::load_json("settings.json", |settings| validate(settings))?
            .unwrap_or_default(),
    )
}

fn save_settings(settings: &Settings) -> Result<(), String> {
    validate(settings)?;

    crate::storage::save_json("settings.json", settings)
}

thread_local! { static DISPLAY: Cell<DisplayOptions> = Cell::new(DisplayOptions::default()); }
pub fn display() -> DisplayOptions {
    DISPLAY.get()
}
thread_local! { static APPEARANCE: gtk::gio::SimpleAction = gtk::gio::SimpleAction::new("appearance",None); }
pub fn watch_appearance(callback: impl Fn() + 'static) -> gtk::glib::SignalHandlerId {
    APPEARANCE.with(|action| action.connect_activate(move |_, _| callback()))
}
pub fn disconnect_appearance(id: gtk::glib::SignalHandlerId) {
    APPEARANCE.with(|action| action.disconnect(id));
}
pub fn set_display(value: DisplayOptions) {
    let previous = DISPLAY.replace(value);
    if previous.seasonal != value.seasonal || previous.animations != value.animations {
        APPEARANCE.with(|a| a.activate(None));
    }
    crate::forms::set_wheel(value.wheel);
}

pub fn apply_color_scheme(settings: &Settings) {
    let scheme = if settings.is_dark_at(chrono::Local::now().time()) {
        adw::ColorScheme::ForceDark
    } else {
        adw::ColorScheme::ForceLight
    };

    adw::StyleManager::default().set_color_scheme(scheme);
}

fn apply_settings(settings: &Settings, brand: &gtk::Label) {
    apply_color_scheme(settings);

    let title = if settings.class.is_empty() {
        "Школьный день".to_string()
    } else {
        format!("Школьный день · {}", settings.class)
    };

    brand.set_text(&title);
    brand.set_tooltip_text(Some(&title));
}

fn time_field(title: &str, value: NaiveTime) -> (gtk::Box, gtk::SpinButton, gtk::SpinButton) {
    use chrono::Timelike;

    let group = column(8);
    group.append(&text(title, "section"));

    let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);

    let hour = forms::time_input(23.0, value.hour(), &format!("{title}: часы"));
    let minute = forms::time_input(59.0, value.minute(), &format!("{title}: минуты"));

    row.append(&hour);
    row.append(&text(":", "muted"));
    row.append(&minute);

    group.append(&row);

    (group, hour, minute)
}

pub fn settings_page(
    items: Option<&crate::lessons::Lessons>,
    status: &gtk::Label,
    brand: &gtk::Label,
    service: &gtk::Box,
    loaded: Result<Settings, String>,
    sync: Option<&crate::sync_host::Host>,
) -> gtk::Box {
    let container = column(14);
    container.append(&settings_form(
        status, brand, &container, service, items, loaded, sync,
    ));
    container
}

fn settings_form(
    status: &gtk::Label,
    brand: &gtk::Label,
    container: &gtk::Box,
    service: &gtk::Box,
    items: Option<&crate::lessons::Lessons>,
    loaded: Result<Settings, String>,
    sync: Option<&crate::sync_host::Host>,
) -> gtk::Box {
    while let Some(child) = service.first_child() {
        service.remove(&child);
    }
    let page = column(10);
    let heading = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let title = text("Настройки", "page-title");
    title.set_hexpand(true);
    heading.append(&title);
    let help = gtk::MenuButton::builder()
        .icon_name("help-browser-symbolic")
        .tooltip_text("Помощь")
        .build();
    help.add_css_class("flat");
    let guide = gtk::gio::Menu::new();
    guide.append(Some("Пройти обучение"), Some("win.tutorial"));
    help.set_menu_model(Some(&guide));
    heading.append(&help);
    page.append(&heading);

    let loaded = match loaded {
        Ok(settings) => settings,

        Err(error) => {
            service.append(&crate::updates::indicator(false, true, |_, _| {
                Err("Сначала восстановите настройки приложения".into())
            }));
            page.append(&text(
                &format!("Не удалось загрузить настройки: {error}"),
                "form-error",
            ));
            let reset = primary("Восстановить настройки по умолчанию");
            let (status, brand, container) = (status.clone(), brand.clone(), container.downgrade());
            let weak_service = service.downgrade();
            let items = items.cloned();
            let sync = sync.cloned();
            reset.connect_clicked(move |_| {
                let Some(container) = container.upgrade() else {
                    return;
                };
                match crate::storage::reset_json("settings.json", &Settings::default()) {
                    Ok(backup) => {
                        while let Some(child) = container.first_child() {
                            container.remove(&child);
                        }
                        if let Some(service) = weak_service.upgrade() {
                            container.append(&settings_form(
                                &status,
                                &brand,
                                &container,
                                &service,
                                items.as_ref(),
                                load_settings(),
                                sync.as_ref(),
                            ));
                        }
                        let message = backup.map_or_else(
                            || "Настройки восстановлены".to_string(),
                            |path| {
                                format!("Настройки сброшены. Резервная копия: {}", path.display())
                            },
                        );
                        notify(&status, &message);
                    }
                    Err(error) => notify(
                        &status,
                        &format!("Не удалось восстановить настройки: {error}"),
                    ),
                }
            });
            page.append(&reset);

            return page;
        }
    };

    apply_settings(&loaded, brand);

    let stack = gtk::Stack::new();
    stack.set_hhomogeneous(false);
    stack.set_vhomogeneous(false);
    stack.add_named(&page, Some("home"));
    let general = adw::PreferencesGroup::builder().title("Общие").build();
    page.append(&general);
    let class = adw::EntryRow::builder().title("Класс").build();
    class.set_text(&loaded.class);
    general.add(&class);
    let class_error = text("", "form-error");
    class_error.set_visible(false);
    page.append(&class_error);

    let theme = gtk::DropDown::from_strings(&["Светлая", "Тёмная", "Авто"]);
    theme.set_widget_name("settings-theme");
    crate::forms::guard_wheel(&theme);

    theme.set_selected(match loaded.theme {
        Theme::Light => 0,
        Theme::Dark => 1,
        Theme::Auto => 2,
    });

    theme.set_valign(gtk::Align::Center);
    let theme_row = adw::ActionRow::builder().title("Тема").build();
    theme_row.add_suffix(&theme);
    general.add(&theme_row);
    Anchor::Theme.mark(&theme);

    let automatic = adw::WrapBox::new();
    automatic.set_child_spacing(24);
    automatic.set_line_spacing(12);

    let (light_group, light_hour, light_minute) = time_field("Светлая с", loaded.light_from);

    let (dark_group, dark_hour, dark_minute) = time_field("Тёмная с", loaded.dark_from);

    automatic.append(&light_group);
    automatic.append(&dark_group);
    automatic.set_visible(loaded.theme == Theme::Auto);

    let hide_old = adw::SwitchRow::builder()
        .title("Автоматически скрывать старые уроки")
        .active(loaded.hide_old_lessons)
        .build();
    let days = gtk::SpinButton::with_range(1.0, 180.0, 1.0);
    days.set_numeric(true);
    days.set_valign(gtk::Align::Center);
    days.set_value(f64::from(loaded.old_lesson_days));
    days.set_tooltip_text(Some("Срок скрытия в днях"));
    days.update_property(&[gtk::accessible::Property::Label("Скрывать спустя, дней")]);
    crate::forms::guard_wheel(&days);
    let days_row = adw::ActionRow::builder()
        .title("Скрывать спустя, дней")
        .visible(loaded.hide_old_lessons)
        .build();
    days_row.add_suffix(&days);
    days_row.set_activatable_widget(Some(&days));

    set_display(loaded.display);
    let current = Rc::new(RefCell::new(loaded));
    let saved = Rc::clone(&current);

    let weak_class = class.downgrade();
    let weak_class_error = class_error.downgrade();
    let weak_theme = theme.downgrade();
    let weak_hide_old = hide_old.downgrade();
    let weak_days = days.downgrade();

    let weak_times = [
        light_hour.downgrade(),
        light_minute.downgrade(),
        dark_hour.downgrade(),
        dark_minute.downgrade(),
    ];

    let weak_automatic = automatic.downgrade();
    let weak_status = status.downgrade();
    let weak_brand = brand.downgrade();

    let updating = Rc::new(Cell::new(false));
    let updating_controls = updating.clone();

    let commit: Rc<dyn Fn()> = Rc::new(move || {
        let updating = &updating_controls;
        if updating.get() {
            return;
        }

        let (
            Some(class),
            Some(class_error),
            Some(theme),
            Some(status),
            Some(brand),
            Some(automatic),
            Some(hide_old),
            Some(days),
        ) = (
            weak_class.upgrade(),
            weak_class_error.upgrade(),
            weak_theme.upgrade(),
            weak_status.upgrade(),
            weak_brand.upgrade(),
            weak_automatic.upgrade(),
            weak_hide_old.upgrade(),
            weak_days.upgrade(),
        )
        else {
            return;
        };

        let Some(times) = weak_times
            .iter()
            .map(|spin| spin.upgrade())
            .collect::<Option<Vec<_>>>()
        else {
            return;
        };

        updating.set(true);

        for spin in &times {
            spin.update();
        }

        updating.set(false);

        let entered_class = class.text();
        let mut class_value = entered_class.trim().to_string();
        if let Some(error) =
            forms::length_error(&class_value, saved.borrow().class.trim(), CLASS_LIMIT)
        {
            notify(&class_error, &error);
            class_value = saved.borrow().class.clone();
        } else {
            class_error.set_visible(false);
            if entered_class == saved.borrow().class {
                class_value = saved.borrow().class.clone();
            }
        }
        let candidate = Settings {
            display: saved.borrow().display,
            class: class_value,

            theme: match theme.selected() {
                0 => Theme::Light,
                1 => Theme::Dark,
                _ => Theme::Auto,
            },

            light_from: NaiveTime::from_hms_opt(
                times[0].value_as_int() as u32,
                times[1].value_as_int() as u32,
                0,
            )
            .unwrap(),

            dark_from: NaiveTime::from_hms_opt(
                times[2].value_as_int() as u32,
                times[3].value_as_int() as u32,
                0,
            )
            .unwrap(),

            bells: saved.borrow().bells.clone(),
            check_updates: saved.borrow().check_updates,
            hide_old_lessons: hide_old.is_active(),
            old_lesson_days: days.value().round() as u16,
        };

        automatic.set_visible(candidate.theme == Theme::Auto);

        if candidate == *saved.borrow() {
            status.set_visible(false);
            return;
        }

        match save_settings(&candidate) {
            Ok(()) => {
                apply_settings(&candidate, &brand);
                *saved.borrow_mut() = candidate;

                notify(&status, "Настройки сохранены");
            }

            Err(error) => {
                use chrono::Timelike;
                let old = saved.borrow();
                updating.set(true);
                class.set_text(&old.class);
                theme.set_selected(match old.theme {
                    Theme::Light => 0,
                    Theme::Dark => 1,
                    Theme::Auto => 2,
                });
                automatic.set_visible(old.theme == Theme::Auto);
                hide_old.set_active(old.hide_old_lessons);
                days.set_value(f64::from(old.old_lesson_days));
                for (spin, previous) in times.iter().zip([
                    old.light_from.hour(),
                    old.light_from.minute(),
                    old.dark_from.hour(),
                    old.dark_from.minute(),
                ]) {
                    spin.set_value(f64::from(previous));
                }
                updating.set(false);
                notify(&status, &format!("Не удалось сохранить настройки: {error}"));
            }
        }
    });

    let callback = Rc::clone(&commit);
    class.connect_entry_activated(move |_| callback());
    let callback = Rc::clone(&commit);
    let focus = gtk::EventControllerFocus::new();
    focus.connect_leave(move |_| callback());
    class.add_controller(focus);

    let callback = Rc::clone(&commit);
    theme.connect_selected_notify(move |_| callback());

    for spin in [&light_hour, &light_minute, &dark_hour, &dark_minute] {
        let callback = Rc::clone(&commit);
        spin.connect_value_changed(move |_| callback());
    }

    let callback = Rc::clone(&commit);
    let weak_days_row = days_row.downgrade();
    hide_old.connect_active_notify(move |switch| {
        if let Some(row) = weak_days_row.upgrade() {
            row.set_visible(switch.is_active());
        }
        callback();
    });

    let pending = Rc::new(RefCell::new(None::<gtk::glib::SourceId>));
    let pending_save = Rc::clone(&pending);
    let callback = Rc::clone(&commit);
    let queue = Rc::new(move || {
        if updating.get() {
            return;
        }
        if let Some(source) = pending_save.borrow_mut().take() {
            source.remove();
        }
        let callback = Rc::clone(&callback);
        let pending = Rc::clone(&pending_save);
        let source =
            gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(400), move || {
                pending.borrow_mut().take();
                callback();
            });
        *pending_save.borrow_mut() = Some(source);
    });
    let class_save = queue.clone();
    class.connect_changed(move |_| class_save());
    days.connect_value_changed(move |_| queue());
    let flush = move || {
        let source = pending.borrow_mut().take();
        if let Some(source) = source {
            source.remove();
            commit();
        }
    };
    let flush_on_leave = flush.clone();
    stack.connect_unmap(move |_| flush_on_leave());
    stack.connect_visible_child_notify(move |_| flush());
    let bells_page = subpage(&stack, "bells", "Расписание звонков", "home");
    let bell_editor = column(0);
    bells_page.append(&bell_editor);
    let school_settings = current.clone();
    let model = items.cloned();
    let feedback = status.downgrade();
    bell_editor.connect_map(move |editor| {
        if editor.first_child().is_some() {
            return;
        }
        let initial = model
            .as_ref()
            .and_then(|m| m.data().bells.clone())
            .unwrap_or_else(|| school_settings.borrow().bells.clone());
        let (model, saved, status) = (model.clone(), school_settings.clone(), feedback.clone());
        editor.append(&crate::bells::editor(&initial, move |bells| {
            if let Some(model) = &model {
                model.set_bells(bells)?;
            } else {
                let mut settings = saved.borrow().clone();
                settings.bells = bells;
                save_settings(&settings)?;
                *saved.borrow_mut() = settings;
            }
            if let Some(status) = status.upgrade() {
                notify(&status, "Звонки сохранены");
            }
            Ok(())
        }));
    });
    bell_editor.connect_unmap(|editor| {
        if let Some(child) = editor.first_child() {
            let weak = editor.downgrade();
            gtk::glib::idle_add_local_once(move || {
                if let Some(editor) = weak.upgrade().filter(|editor| !editor.is_mapped())
                    && child.parent().as_ref() == Some(editor.upcast_ref())
                {
                    editor.remove(&child);
                }
            });
        }
    });

    let links = adw::PreferencesGroup::builder().title("Приложение").build();
    page.append(&links);
    link(&links, &stack, "bells", "Расписание звонков", "");
    link(&links, &stack, "advanced", "Дополнительные настройки", "");
    let advanced = subpage(&stack, "advanced", "Дополнительные настройки", "home");
    let advanced_links = adw::PreferencesGroup::new();
    advanced.append(&advanced_links);
    let actions = gtk::gio::SimpleActionGroup::new();
    let home = gtk::gio::SimpleAction::new("home", None);
    let weak_stack = stack.downgrade();
    home.connect_activate(move |_, _| {
        if let Some(stack) = weak_stack.upgrade() {
            stack.set_visible_child_name("home");
        }
    });
    actions.add_action(&home);
    container.insert_action_group("school-settings", Some(&actions));
    stack.connect_visible_child_notify(|stack| {
        if let Some(scroll) = stack
            .ancestor(gtk::ScrolledWindow::static_type())
            .and_downcast::<gtk::ScrolledWindow>()
        {
            scroll.vadjustment().set_value(0.0);
        }
    });

    let update_settings = Rc::clone(&current);
    let (automatic_updates, badge) = {
        let settings = current.borrow();
        (settings.check_updates, settings.display.update_badge)
    };
    service.append(&crate::updates::indicator(
        automatic_updates,
        badge,
        move |preference, enabled| {
            let mut candidate = update_settings.borrow().clone();
            match preference {
                crate::updates::Preference::Automatic => candidate.check_updates = enabled,
                crate::updates::Preference::Badge => candidate.display.update_badge = enabled,
            }
            save_settings(&candidate)?;
            set_display(candidate.display);
            *update_settings.borrow_mut() = candidate;
            Ok(())
        },
    ));

    for (key, title, choices) in preference_groups() {
        let main = matches!(key, "data" | "sync");
        let page = subpage(&stack, key, title, if main { "home" } else { "advanced" });
        let group = adw::PreferencesGroup::new();
        page.append(&group);
        link(
            if main { &links } else { &advanced_links },
            &stack,
            key,
            title,
            "",
        );
        match key {
            "appearance" => group.add(&automatic),
            "schedule" => {
                group.add(&hide_old);
                group.add(&days_row);
            }
            "data" => {
                if let Some(items) = items {
                    crate::transfer::populate(&group, items, status);
                }
            }
            "sync" => {
                if let (Some(items), Some(sync)) = (items, sync) {
                    crate::sync_ui::populate(&group, items, status, sync);
                }
            }
            "about" => {
                crate::transfer::about(&group);
                page.append(&crate::transfer::system_requirements());
            }
            _ => {}
        }
        for (title, field) in choices {
            let row = adw::SwitchRow::builder().title(title).build();
            row.set_active(*field(&mut current.borrow().display.clone()));
            let state = current.clone();
            let status = status.downgrade();
            row.connect_active_notify(move |row| {
                let mut next = state.borrow().clone();
                let old = *field(&mut next.display);
                if old == row.is_active() {
                    return;
                }
                *field(&mut next.display) = row.is_active();
                match save_settings(&next) {
                    Ok(()) => {
                        set_display(next.display);
                        *state.borrow_mut() = next;
                    }
                    Err(error) => {
                        row.set_active(old);
                        if let Some(status) = status.upgrade() {
                            notify(&status, &error);
                        }
                    }
                }
            });
            group.add(&row);
        }
        if key == "appearance" {
            let mode = gtk::DropDown::from_strings(&["По календарю", "Всегда", "Выключено"]);
            crate::forms::guard_wheel(&mode);
            let index = |mode| match mode {
                HolidayStyle::Calendar => 0,
                HolidayStyle::Always => 1,
                HolidayStyle::Off => 2,
            };
            mode.set_selected(index(current.borrow().display.seasonal));
            mode.set_widget_name("holiday-mode");
            mode.set_tooltip_text(Some(
                "В режиме «Всегда» между праздниками используется оформление ближайшего праздника.",
            ));
            let holiday = column(5);
            crate::margins(&holiday, 8);
            let label = text("Праздничное оформление", "section");
            label.set_mnemonic_widget(Some(&mode));
            holiday.append(&label);
            holiday.append(&mode);
            group.add(&holiday);
            let (state, feedback) = (current.clone(), status.downgrade());
            mode.connect_selected_notify(move |mode| {
                let mut next = state.borrow().clone();
                let old = next.display.seasonal;
                next.display.seasonal = match mode.selected() {
                    0 => HolidayStyle::Calendar,
                    1 => HolidayStyle::Always,
                    _ => HolidayStyle::Off,
                };
                if next.display.seasonal == old {
                    return;
                }
                match save_settings(&next) {
                    Ok(()) => {
                        set_display(next.display);
                        *state.borrow_mut() = next;
                    }
                    Err(e) => {
                        mode.set_selected(index(old));
                        if let Some(label) = feedback.upgrade() {
                            notify(&label, &e);
                        }
                    }
                }
            });
        }
    }

    let weak_brand = brand.downgrade();

    let weak_page = page.downgrade();
    gtk::glib::timeout_add_seconds_local(60, move || {
        let Some(brand) = weak_brand
            .upgrade()
            .filter(|_| weak_page.upgrade().is_some())
        else {
            return gtk::glib::ControlFlow::Break;
        };

        if current.borrow().theme == Theme::Auto {
            apply_settings(&current.borrow(), &brand);
        }

        gtk::glib::ControlFlow::Continue
    });

    let layout = column(0);
    layout.append(&stack);
    layout
}

fn subpage(stack: &gtk::Stack, key: &str, title: &str, parent: &'static str) -> gtk::Box {
    let page = column(14);
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let back = gtk::Button::from_icon_name("go-previous-symbolic");
    back.set_tooltip_text(Some("Назад к настройкам"));
    back.add_css_class("flat");
    header.append(&back);
    header.append(&text(title, "page-title"));
    page.append(&header);
    let weak = stack.downgrade();
    back.connect_clicked(move |_| {
        if let Some(stack) = weak.upgrade() {
            stack.set_visible_child_name(parent);
        }
    });
    stack.add_named(&page, Some(key));
    page
}

fn link(
    group: &adw::PreferencesGroup,
    stack: &gtk::Stack,
    key: &'static str,
    title: &str,
    subtitle: &str,
) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(title)
        .subtitle(subtitle)
        .activatable(true)
        .build();
    row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    let weak = stack.downgrade();
    row.connect_activated(move |_| {
        if let Some(stack) = weak.upgrade() {
            stack.set_visible_child_name(key);
        }
    });
    group.add(&row);
    row
}

type Choice = (&'static str, fn(&mut DisplayOptions) -> &mut bool);
fn preference_groups() -> Vec<(&'static str, &'static str, Vec<Choice>)> {
    vec![
        (
            "appearance",
            "Оформление и анимации",
            vec![
                ("Компактный режим", |s| &mut s.compact),
                ("Анимации праздников", |s| {
                    &mut s.animations
                }),
            ],
        ),
        (
            "schedule",
            "Отображение расписания",
            vec![
                ("Напоминать за 5 минут до урока", |s| &mut s.reminders),
                ("Кабинеты", |s| &mut s.rooms),
                ("Заметки", |s| &mut s.notes),
                ("Время уроков", |s| &mut s.times),
                ("Прошедшие уроки", |s| &mut s.finished),
                ("Краткие отметки уроков", |s| {
                    &mut s.compact_status
                }),
            ],
        ),
        (
            "control",
            "Управление",
            vec![
                ("Менять значения колёсиком", |s| {
                    &mut s.wheel
                }),
                ("Подтверждать удаление", |s| {
                    &mut s.confirm_delete
                }),
            ],
        ),
        (
            "data",
            "Данные",
            vec![(
                "Ежедневная копия перед изменением расписания",
                |s| &mut s.auto_backup,
            )],
        ),
        ("sync", "Синхронизация", vec![]),
        ("about", "О приложении", vec![]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_settings_keep_values_and_default_to_no_hiding() -> Result<(), serde_json::Error> {
        let settings: Settings =
            serde_json::from_str(r#"{"class":"9Б","theme":"dark","check_updates":true}"#)?;
        assert_eq!(settings.class, "9Б");
        assert!(settings.theme == Theme::Dark);
        assert!(settings.check_updates);
        assert!(!settings.hide_old_lessons);
        assert_eq!(settings.old_lesson_days, 14);
        assert!(validate(&settings).is_ok());
        Ok(())
    }

    #[test]
    fn legacy_renderer_is_ignored_while_other_settings_round_trip() -> Result<(), serde_json::Error>
    {
        let original = Settings {
            class: "10Б".into(),
            theme: Theme::Dark,
            check_updates: true,
            hide_old_lessons: true,
            old_lesson_days: 90,
            ..Settings::default()
        };
        for renderer in ["auto", "cairo", "gl", "opengl", "vulkan", "future-mode"] {
            let mut legacy = serde_json::to_value(&original)?;
            legacy["renderer"] = renderer.into();
            let loaded: Settings = serde_json::from_value(legacy)?;
            assert!(loaded == original);
            let saved = serde_json::to_value(&loaded)?;
            assert!(saved.get("renderer").is_none());
            assert!(serde_json::from_value::<Settings>(saved)? == original);
        }
        Ok(())
    }

    #[test]
    fn hiding_settings_survive_round_trips_and_other_changes() -> Result<(), serde_json::Error> {
        for days in [1, 14, 180] {
            let original = Settings {
                class: "9Б".into(),
                hide_old_lessons: true,
                old_lesson_days: days,
                ..Settings::default()
            };
            let mut loaded: Settings = serde_json::from_str(&serde_json::to_string(&original)?)?;
            assert!(loaded == original);
            loaded.theme = Theme::Dark;
            loaded.class = "10Б".into();
            loaded.check_updates = true;
            loaded.bells.lesson_minutes = 45;
            let reloaded: Settings = serde_json::from_str(&serde_json::to_string(&loaded)?)?;
            assert!(reloaded == loaded);
            assert!(reloaded.hide_old_lessons);
            assert_eq!(reloaded.old_lesson_days, days);
            assert!(validate(&reloaded).is_ok());
        }
        for days in [0, 181] {
            assert!(
                validate(&Settings {
                    old_lesson_days: days,
                    ..Settings::default()
                })
                .is_err()
            );
        }
        Ok(())
    }

    #[test]
    fn equal_automatic_times_do_not_block_manual_themes() {
        for theme in [Theme::Light, Theme::Dark] {
            let settings = Settings {
                theme,
                light_from: NaiveTime::from_hms_opt(20, 0, 0).unwrap(),
                ..Settings::default()
            };
            assert!(validate(&settings).is_ok());
        }
        let automatic = Settings {
            light_from: NaiveTime::from_hms_opt(20, 0, 0).unwrap(),
            ..Settings::default()
        };
        assert!(validate(&automatic).is_err());
    }

    #[test]
    fn automatic_theme_changes_at_both_boundaries() {
        let settings = Settings::default();
        for (hour, minute, expected_dark) in
            [(6, 59, true), (7, 0, false), (19, 59, false), (20, 0, true)]
        {
            assert_eq!(
                settings.is_dark_at(NaiveTime::from_hms_opt(hour, minute, 0).unwrap()),
                expected_dark
            );
        }
    }

    #[test]
    fn light_period_can_cross_midnight() {
        let settings = Settings {
            light_from: NaiveTime::from_hms_opt(20, 0, 0).unwrap(),
            dark_from: NaiveTime::from_hms_opt(7, 0, 0).unwrap(),
            ..Settings::default()
        };
        assert!(!settings.is_dark_at(NaiveTime::from_hms_opt(23, 0, 0).unwrap()));
        assert!(settings.is_dark_at(NaiveTime::from_hms_opt(12, 0, 0).unwrap()));
    }
}
