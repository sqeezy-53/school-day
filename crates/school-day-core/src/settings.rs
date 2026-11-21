use crate::bells::BellSchedule;
use chrono::NaiveTime;
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
    pub display: DisplayOptions,
    pub class: String,
    pub theme: Theme,
    pub light_from: NaiveTime,
    pub dark_from: NaiveTime,
    pub bells: BellSchedule,
    pub check_updates: bool,
    pub hide_old_lessons: bool,
    pub old_lesson_days: u16,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            display: DisplayOptions::default(),
            class: String::new(),
            theme: Theme::Auto,
            light_from: NaiveTime::from_hms_opt(7, 0, 0).unwrap(),
            dark_from: NaiveTime::from_hms_opt(20, 0, 0).unwrap(),
            bells: BellSchedule::default(),
            check_updates: false,
            hide_old_lessons: false,
            old_lesson_days: 14,
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

pub fn validate(settings: &Settings) -> Result<(), String> {
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

#[derive(Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct DisplayOptions {
    pub rooms: bool,
    pub notes: bool,
    pub times: bool,
    pub finished: bool,
    pub compact: bool,
    pub compact_status: bool,
    pub animations: bool,
    pub seasonal: HolidayStyle,
    pub wheel: bool,
    pub confirm_delete: bool,
    pub auto_backup: bool,
    pub update_badge: bool,
    pub reminders: bool,
}
impl Default for DisplayOptions {
    fn default() -> Self {
        Self {
            rooms: true,
            notes: true,
            times: true,
            finished: true,
            compact: false,
            compact_status: false,
            animations: true,
            seasonal: HolidayStyle::Calendar,
            wheel: false,
            confirm_delete: true,
            auto_backup: true,
            update_badge: true,
            reminders: false,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HolidayStyle {
    #[default]
    Calendar,
    Always,
    Off,
}
