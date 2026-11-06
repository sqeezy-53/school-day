use adw::prelude::*;
use chrono::NaiveTime;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::bells::BellSchedule;
use crate::forms::{self, CLASS_LIMIT};
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
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            class: String::new(),
            theme: Theme::Auto,
            light_from: NaiveTime::from_hms_opt(7, 0, 0).unwrap(),
            dark_from: NaiveTime::from_hms_opt(20, 0, 0).unwrap(),
            bells: BellSchedule::default(),
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
    if settings.class.contains('\0') {
        return Err("В названии класса есть недопустимый нулевой символ".to_string());
    }
    if settings.theme == Theme::Auto && settings.light_from == settings.dark_from {
        return Err("Время включения тем должно различаться".to_string());
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

    let hour = time_number(23.0, value.hour(), &format!("{title}: часы"));
    let minute = time_number(59.0, value.minute(), &format!("{title}: минуты"));

    row.append(&hour);
    row.append(&text(":", "muted"));
    row.append(&minute);

    group.append(&row);

    (group, hour, minute)
}

fn time_number(maximum: f64, value: u32, label: &str) -> gtk::SpinButton {
    let spin = gtk::SpinButton::with_range(0.0, maximum, 1.0);
    spin.set_numeric(true);
    spin.set_wrap(true);
    spin.set_width_chars(2);
    spin.set_tooltip_text(Some(label));
    spin.set_update_policy(gtk::SpinButtonUpdatePolicy::IfValid);
    spin.set_value(value as f64);

    spin.connect_output(|spin| {
        spin.set_text(&format!("{:02}", spin.value_as_int()));
        gtk::glib::Propagation::Stop
    });

    spin.set_text(&format!("{value:02}"));

    spin
}

pub fn settings_page(status: &gtk::Label, brand: &gtk::Label) -> gtk::Box {
    let container = column(0);
    container.append(&settings_form(status, brand, &container));
    container
}

fn settings_form(status: &gtk::Label, brand: &gtk::Label, container: &gtk::Box) -> gtk::Box {
    let page = column(24);
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
            page.append(&text(
                &format!("Не удалось загрузить настройки: {error}"),
                "muted",
            ));
            let reset = primary("Восстановить настройки по умолчанию");
            let (status, brand, container) = (status.clone(), brand.clone(), container.downgrade());
            reset.connect_clicked(move |_| {
                let Some(container) = container.upgrade() else {
                    return;
                };
                match crate::storage::reset_json("settings.json", &Settings::default()) {
                    Ok(backup) => {
                        while let Some(child) = container.first_child() {
                            container.remove(&child);
                        }
                        container.append(&settings_form(&status, &brand, &container));
                        let message = backup.map_or_else(
                            || "Настройки восстановлены".to_string(),
                            |path| {
                                format!(
                                    "Настройки восстановлены. Копия прежних настроек: {}",
                                    path.display()
                                )
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

    let class = field(&page, "Класс", "Например, 7А");
    class.set_text(&loaded.class);
    let class_error = forms::error_label(&page, &class);

    page.append(&text("Тема", "section"));

    let theme = gtk::DropDown::from_strings(&["Светлая", "Тёмная", "Авто"]);
    Anchor::Theme.mark(&theme);

    theme.set_selected(match loaded.theme {
        Theme::Light => 0,
        Theme::Dark => 1,
        Theme::Auto => 2,
    });

    page.append(&theme);

    let automatic = adw::WrapBox::new();
    automatic.set_child_spacing(24);
    automatic.set_line_spacing(12);

    let (light_group, light_hour, light_minute) = time_field("Светлая с", loaded.light_from);

    let (dark_group, dark_hour, dark_minute) = time_field("Тёмная с", loaded.dark_from);

    automatic.append(&light_group);
    automatic.append(&dark_group);
    automatic.set_visible(loaded.theme == Theme::Auto);

    page.append(&automatic);

    page.append(&text("Сохраняется автоматически", "muted"));

    let current = Rc::new(RefCell::new(loaded));
    let saved = Rc::clone(&current);

    let weak_class = class.downgrade();
    let weak_class_error = class_error.downgrade();
    let weak_theme = theme.downgrade();

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
            Some(status),
            Some(brand),
            Some(automatic),
        ) = (
            weak_class.upgrade(),
            weak_class_error.upgrade(),
            weak_theme.upgrade(),
            weak_status.upgrade(),
            weak_brand.upgrade(),
            weak_automatic.upgrade(),
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

        // Применяем набранный текст, защищаясь от повторных сигналов.
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
                notify(&status, &format!("Не удалось сохранить настройки: {error}"));
            }
        }
    });

    let callback = Rc::clone(&commit);
    class.connect_changed(move |_| callback());

    let callback = Rc::clone(&commit);
    theme.connect_selected_notify(move |_| callback());

    for spin in [&light_hour, &light_minute, &dark_hour, &dark_minute] {
        let callback = Rc::clone(&commit);
        spin.connect_value_changed(move |_| callback());
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

    let weak_brand = brand.downgrade();

    gtk::glib::timeout_add_seconds_local(60, move || {
        let Some(brand) = weak_brand.upgrade() else {
            return gtk::glib::ControlFlow::Break;
        };

        apply_settings(&current.borrow(), &brand);

        gtk::glib::ControlFlow::Continue
    });

    page
}

#[cfg(test)]
mod tests {
    use super::*;

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
