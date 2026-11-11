use adw::prelude::*;
use chrono::NaiveTime;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::bells::BellSchedule;
use crate::forms::{self, CLASS_LIMIT};
use crate::renderer::Renderer;
use crate::tutorial::Anchor;
use crate::{column, field, notify, primary, text};

#[derive(Clone, Copy, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    Light,
    Dark,
    #[default]
    Auto,
}

#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Settings {
    pub class: String,
    pub theme: Theme,
    pub light_from: NaiveTime,
    pub dark_from: NaiveTime,
    pub bells: BellSchedule,
    pub check_updates: bool,
    pub hide_old_lessons: bool,
    pub old_lesson_days: u16,
    pub renderer: Renderer,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            class: String::new(),
            theme: Theme::Auto,
            light_from: NaiveTime::from_hms_opt(7, 0, 0).unwrap(),
            dark_from: NaiveTime::from_hms_opt(20, 0, 0).unwrap(),
            bells: BellSchedule::default(),
            check_updates: false,
            hide_old_lessons: false,
            old_lesson_days: 14,
            renderer: Renderer::Auto,
        }
    }
}

impl Settings {
    pub fn is_dark_at(&self, now: NaiveTime) -> bool {
        match self.theme {
            Theme::Light => false,
            Theme::Dark => true,

            Theme::Auto => {
                let light = if self.light_from < self.dark_from {
                    now >= self.light_from && now < self.dark_from
                } else {
                    now >= self.light_from || now < self.dark_from
                };

                !light
            }
        }
    }
}

fn validate(settings: &Settings) -> Result<(), String> {
    if !(1..=180).contains(&settings.old_lesson_days) {
        return Err("Срок скрытия должен быть от 1 до 180 дней".to_string());
    }
    if settings.class.contains('\0') {
        return Err("Недопустимые символы в названии класса".to_string());
    }
    if settings.theme == Theme::Auto && settings.light_from == settings.dark_from {
        return Err("Укажите разное время для светлой и тёмной темы".to_string());
    }

    settings.bells.slots()?;
    Ok(())
}

pub fn load_settings() -> Result<Settings, String> {
    Ok(
        crate::storage::load_json("settings.json", |settings| validate(settings))?
            .unwrap_or_default(),
    )
}

pub fn load_startup_settings() -> Result<Settings, String> {
    Ok(
        crate::storage::read_json("settings.json", |settings| validate(settings))?
            .unwrap_or_default(),
    )
}

fn save_settings(settings: &Settings) -> Result<(), String> {
    validate(settings)?;

    crate::storage::save_json("settings.json", settings)
}

fn apply_settings(settings: &Settings, brand: &gtk::Label) {
    let scheme = if settings.is_dark_at(chrono::Local::now().time()) {
        adw::ColorScheme::ForceDark
    } else {
        adw::ColorScheme::ForceLight
    };

    adw::StyleManager::default().set_color_scheme(scheme);

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

pub fn settings_page(status: &gtk::Label, brand: &gtk::Label, service: &gtk::Box) -> gtk::Box {
    let container = column(0);
    container.append(&settings_form(status, brand, &container, service));
    container
}

fn settings_form(
    status: &gtk::Label,
    brand: &gtk::Label,
    container: &gtk::Box,
    service: &gtk::Box,
) -> gtk::Box {
    while let Some(child) = service.first_child() {
        service.remove(&child);
    }
    let page = column(32);
    let heading = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let title = text("Настройки", "title");
    title.set_hexpand(true);
    heading.append(&title);
    let tutorial = gtk::Button::from_icon_name("help-browser-symbolic");
    tutorial.add_css_class("flat");
    tutorial.add_css_class("circular");
    tutorial.set_valign(gtk::Align::Center);
    tutorial.set_tooltip_text(Some("Пройти обучение снова"));
    tutorial.set_action_name(Some("win.tutorial"));
    heading.append(&tutorial);
    page.append(&heading);

    let loaded = match load_settings() {
        Ok(settings) => settings,

        Err(error) => {
            service.append(&crate::updates::indicator(false, |_| {
                Err("Сначала восстановите настройки приложения".into())
            }));
            page.append(&text(
                &format!("Не удалось загрузить настройки: {error}"),
                "form-error",
            ));
            let reset = primary("Восстановить настройки по умолчанию");
            let (status, brand, container) = (status.clone(), brand.clone(), container.downgrade());
            let weak_service = service.downgrade();
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
                            container.append(&settings_form(&status, &brand, &container, &service));
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

    let sections = gtk::Stack::new();
    sections.set_vhomogeneous(false);
    sections.add_named(&page, Some("main"));
    let advanced = column(28);
    let back_row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let back = gtk::Button::from_icon_name("go-previous-symbolic");
    back.set_tooltip_text(Some("Назад к настройкам"));
    back.add_css_class("flat");
    back_row.append(&back);
    back_row.append(&text("Дополнительные настройки", "settings-title"));
    advanced.append(&back_row);
    sections.add_named(&advanced, Some("advanced"));
    let actions = gtk::gio::SimpleActionGroup::new();
    let show_main = gtk::gio::SimpleAction::new("main", None);
    let weak_sections = sections.downgrade();
    show_main.connect_activate(move |_, _| {
        if let Some(sections) = weak_sections.upgrade() {
            sections.set_visible_child_name("main");
        }
    });
    actions.add_action(&show_main);
    container.insert_action_group("settings", Some(&actions));
    let more = gtk::Button::new();
    more.add_css_class("settings-link");
    let more_row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let more_label = text("Дополнительные настройки", "section");
    more_label.set_hexpand(true);
    more_row.append(&more_label);
    more_row.append(&gtk::Image::from_icon_name("go-next-symbolic"));
    more.set_child(Some(&more_row));
    let weak_sections = sections.downgrade();
    let weak_back = back.downgrade();
    more.connect_clicked(move |_| {
        if let Some(sections) = weak_sections.upgrade() {
            sections.set_visible_child_name("advanced");
        }
        if let Some(back) = weak_back.upgrade() {
            back.grab_focus();
        }
    });
    let weak_container = container.downgrade();
    let weak_more = more.downgrade();
    back.connect_clicked(move |_| {
        if let Some(container) = weak_container.upgrade() {
            let _ = container.activate_action("settings.main", None);
        }
        if let Some(more) = weak_more.upgrade() {
            more.grab_focus();
        }
    });

    let class = field(&advanced, "Класс", "Например, 7А");
    class.set_text(&loaded.class);
    let class_error = forms::error_label(&advanced, &class);

    let appearance = column(12);
    appearance.append(&text("Тема", "section"));

    let theme = gtk::DropDown::from_strings(&["Светлая", "Тёмная", "Авто"]);
    Anchor::Theme.mark(&theme);

    theme.set_selected(match loaded.theme {
        Theme::Light => 0,
        Theme::Dark => 1,
        Theme::Auto => 2,
    });

    appearance.append(&theme);

    let automatic = adw::WrapBox::new();
    automatic.set_child_spacing(24);
    automatic.set_line_spacing(12);

    let (light_group, light_hour, light_minute) = time_field("Светлая с", loaded.light_from);

    let (dark_group, dark_hour, dark_minute) = time_field("Тёмная с", loaded.dark_from);

    automatic.append(&light_group);
    automatic.append(&dark_group);
    automatic.set_visible(loaded.theme == Theme::Auto);

    appearance.append(&automatic);
    page.append(&appearance);

    let old_lessons = column(18);
    old_lessons.add_css_class("settings-section");
    old_lessons.append(&text("Старые уроки", "settings-title"));
    let hide_row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let hide_label = text("Автоматически скрывать старые уроки", "section");
    hide_label.set_hexpand(true);
    let hide_old = gtk::Switch::new();
    hide_old.set_valign(gtk::Align::Center);
    hide_old.set_active(loaded.hide_old_lessons);
    hide_label.set_mnemonic_widget(Some(&hide_old));
    hide_row.append(&hide_label);
    hide_row.append(&hide_old);
    old_lessons.append(&hide_row);
    let days_label = text(
        &format!("Скрывать уроки старше {} дней", loaded.old_lesson_days),
        "section",
    );
    let days = gtk::Scale::with_range(gtk::Orientation::Horizontal, 1.0, 180.0, 1.0);
    days.set_digits(0);
    days.set_round_digits(0);
    days.set_draw_value(false);
    days.set_value(f64::from(loaded.old_lesson_days));
    days.set_sensitive(loaded.hide_old_lessons);
    days.set_tooltip_text(Some("Срок скрытия старых уроков в днях"));
    days_label.set_mnemonic_widget(Some(&days));
    old_lessons.append(&days_label);
    old_lessons.append(&days);
    advanced.append(&old_lessons);

    let graphics = column(12);
    graphics.add_css_class("settings-section");
    let renderer_label = text("Графический рендерер", "settings-title");
    let renderer = gtk::DropDown::from_strings(&Renderer::LABELS);
    renderer.set_widget_name("renderer-choice");
    renderer.set_selected(loaded.renderer.index());
    renderer_label.set_mnemonic_widget(Some(&renderer));
    let renderer_status = text(&crate::renderer::status_text(), "muted");
    renderer_status.connect_map(|label| {
        let status = crate::renderer::status_text();
        label.set_text(&status);
        label.set_visible(!status.is_empty());
    });
    graphics.append(&renderer_label);
    graphics.append(&renderer);
    graphics.append(&renderer_status);
    advanced.append(&graphics);

    let current = Rc::new(RefCell::new(loaded));
    let saved = Rc::clone(&current);

    let weak_class = class.downgrade();
    let weak_class_error = class_error.downgrade();
    let weak_theme = theme.downgrade();
    let weak_renderer = renderer.downgrade();
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

    let updating = Cell::new(false);

    let commit: Rc<dyn Fn()> = Rc::new(move || {
        if updating.get() {
            return;
        }

        let (
            Some(class),
            Some(class_error),
            Some(theme),
            Some(renderer),
            Some(status),
            Some(brand),
            Some(automatic),
            Some(hide_old),
            Some(days),
        ) = (
            weak_class.upgrade(),
            weak_class_error.upgrade(),
            weak_theme.upgrade(),
            weak_renderer.upgrade(),
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
            renderer: Renderer::from_index(renderer.selected()),
        };

        automatic.set_visible(candidate.theme == Theme::Auto);

        if candidate == *saved.borrow() {
            status.set_visible(false);
            return;
        }

        let renderer_changed = candidate.renderer != saved.borrow().renderer;
        match save_settings(&candidate) {
            Ok(()) => {
                apply_settings(&candidate, &brand);
                *saved.borrow_mut() = candidate;

                notify(
                    &status,
                    if renderer_changed {
                        "Рендерер сохранён. Выбор применится после следующего запуска"
                    } else {
                        "Настройки сохранены"
                    },
                );
            }

            Err(error) => {
                if renderer_changed {
                    updating.set(true);
                    renderer.set_selected(saved.borrow().renderer.index());
                    updating.set(false);
                }
                notify(&status, &format!("Не удалось сохранить настройки: {error}"));
            }
        }
    });

    let callback = Rc::clone(&commit);
    class.connect_changed(move |_| callback());

    let callback = Rc::clone(&commit);
    theme.connect_selected_notify(move |_| callback());

    let callback = Rc::clone(&commit);
    renderer.connect_selected_notify(move |_| callback());

    for spin in [&light_hour, &light_minute, &dark_hour, &dark_minute] {
        let callback = Rc::clone(&commit);
        spin.connect_value_changed(move |_| callback());
    }

    let callback = Rc::clone(&commit);
    let weak_days = days.downgrade();
    hide_old.connect_active_notify(move |switch| {
        if let Some(days) = weak_days.upgrade() {
            days.set_sensitive(switch.is_active());
        }
        callback();
    });

    let pending = Rc::new(RefCell::new(None::<gtk::glib::SourceId>));
    let pending_save = Rc::clone(&pending);
    let callback = Rc::clone(&commit);
    let weak_days_label = days_label.downgrade();
    days.connect_value_changed(move |days| {
        if let Some(label) = weak_days_label.upgrade() {
            label.set_text(&format!("Скрывать уроки старше {:.0} дней", days.value()));
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
    for section in [&page, &advanced] {
        let pending = Rc::clone(&pending);
        let commit = Rc::clone(&commit);
        section.connect_unmap(move |_| {
            if let Some(source) = pending.borrow_mut().take() {
                source.remove();
                commit();
            }
        });
    }

    let initial_bells = current.borrow().bells.clone();
    let school_settings = Rc::clone(&current);
    let weak_status = status.downgrade();
    page.append(&crate::bells::editor(&initial_bells, move |bells| {
        let mut candidate = school_settings.borrow().clone();
        candidate.bells = bells;
        save_settings(&candidate)?;
        *school_settings.borrow_mut() = candidate;
        if let Some(status) = weak_status.upgrade() {
            notify(&status, "Звонки сохранены");
        }
        Ok(())
    }));

    page.append(&more);

    let automatic_updates = current.borrow().check_updates;
    let update_settings = Rc::clone(&current);
    service.append(&crate::updates::indicator(
        automatic_updates,
        move |enabled| {
            let mut candidate = update_settings.borrow().clone();
            candidate.check_updates = enabled;
            save_settings(&candidate)?;
            *update_settings.borrow_mut() = candidate;
            Ok(())
        },
    ));

    let weak_brand = brand.downgrade();

    let weak_page = page.downgrade();
    gtk::glib::timeout_add_seconds_local(60, move || {
        let Some(brand) = weak_brand
            .upgrade()
            .filter(|_| weak_page.upgrade().is_some())
        else {
            return gtk::glib::ControlFlow::Break;
        };

        apply_settings(&current.borrow(), &brand);

        gtk::glib::ControlFlow::Continue
    });

    let root = column(0);
    root.append(&sections);
    root
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
        assert_eq!(settings.renderer, Renderer::Auto);
        assert!(validate(&settings).is_ok());
        Ok(())
    }

    #[test]
    fn renderer_preferences_preserve_other_settings_and_recover_unknown_modes()
    -> Result<(), serde_json::Error> {
        let original = Settings {
            class: "10Б".into(),
            theme: Theme::Dark,
            check_updates: true,
            hide_old_lessons: true,
            old_lesson_days: 90,
            renderer: Renderer::Cairo,
            ..Settings::default()
        };
        let mut json = serde_json::to_value(&original)?;
        let decoded: Settings = serde_json::from_value(json.clone())?;
        assert!(decoded == original);
        json["renderer"] = "unknown-future-mode".into();
        let decoded: Settings = serde_json::from_value(json)?;
        assert_eq!(decoded.renderer, Renderer::Auto);
        assert!(
            decoded
                == Settings {
                    renderer: Renderer::Auto,
                    ..original
                }
        );
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
