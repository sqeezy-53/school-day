use adw::prelude::*;
use chrono::NaiveTime;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::{column, field, notify, page, text};

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
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            class: String::new(),
            theme: Theme::Auto,
            light_from: NaiveTime::from_hms_opt(7, 0, 0).unwrap(),
            dark_from: NaiveTime::from_hms_opt(20, 0, 0).unwrap(),
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
    if settings.light_from == settings.dark_from {
        return Err("Время включения тем должно различаться".to_string());
    }

    Ok(())
}

fn load_settings() -> Result<Settings, String> {
    let json = match std::fs::read_to_string("data/settings.json") {
        Ok(json) => json,

        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Settings::default());
        }

        Err(error) => return Err(error.to_string()),
    };

    let settings: Settings = serde_json::from_str(&json).map_err(|error| error.to_string())?;
    validate(&settings)?;

    Ok(settings)
}

fn save_settings(settings: &Settings) -> Result<(), String> {
    validate(settings)?;

    let json = serde_json::to_string_pretty(settings).map_err(|error| error.to_string())?;

    std::fs::create_dir_all("data").map_err(|error| error.to_string())?;

    std::fs::write("data/settings.json.tmp", json).map_err(|error| error.to_string())?;

    std::fs::rename("data/settings.json.tmp", "data/settings.json")
        .map_err(|error| error.to_string())
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
    let page = page("Настройки");

    let loaded = match load_settings() {
        Ok(settings) => settings,

        Err(error) => {
            page.append(&text(
                &format!("Не удалось загрузить настройки: {error}"),
                "muted",
            ));

            return page;
        }
    };

    apply_settings(&loaded, brand);

    let class = field(&page, "Класс", "Например, 7А");
    class.set_text(&loaded.class);

    page.append(&text("Тема", "section"));

    let theme = gtk::DropDown::from_strings(&["Светлая", "Тёмная", "Авто"]);

    theme.set_selected(match loaded.theme {
        Theme::Light => 0,
        Theme::Dark => 1,
        Theme::Auto => 2,
    });

    page.append(&theme);

    let automatic = gtk::Box::new(gtk::Orientation::Horizontal, 24);

    let (light_group, light_hour, light_minute) = time_field("Светлая с", loaded.light_from);

    let (dark_group, dark_hour, dark_minute) = time_field("Тёмная с", loaded.dark_from);

    automatic.append(&light_group);
    automatic.append(&dark_group);
    automatic.set_visible(loaded.theme == Theme::Auto);

    page.append(&automatic);

    page.append(&text(
        "Изменения сохраняются автоматически. Авто использует системное время.",
        "muted",
    ));

    let current = Rc::new(RefCell::new(loaded));
    let saved = Rc::clone(&current);

    let weak_class = class.downgrade();
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

        let (Some(class), Some(theme), Some(status), Some(brand), Some(automatic)) = (
            weak_class.upgrade(),
            weak_theme.upgrade(),
            weak_status.upgrade(),
            weak_brand.upgrade(),
            weak_automatic.upgrade(),
        ) else {
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

        let candidate = Settings {
            class: class.text().trim().to_string(),

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
