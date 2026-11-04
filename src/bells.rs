use adw::prelude::*;
use chrono::{Duration, NaiveTime, Timelike};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::{column, text};

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct BellSchedule {
    pub lesson_minutes: u32,
    pub first_start: NaiveTime,
    pub breaks: Vec<u32>,
}

impl Default for BellSchedule {
    fn default() -> Self {
        Self {
            lesson_minutes: 40,
            first_start: NaiveTime::from_hms_opt(8, 0, 0).unwrap(),
            breaks: vec![10, 10, 15, 15, 10, 10, 10, 10],
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Bell {
    pub start: NaiveTime,
    pub end: NaiveTime,
}

impl BellSchedule {
    pub fn slots(&self) -> Result<Vec<Bell>, String> {
        if ![40, 45].contains(&self.lesson_minutes) {
            return Err("Длительность урока: 40 или 45 минут".to_string());
        }
        if self.breaks.len() > 11 {
            return Err("Можно задать от 1 до 12 уроков".to_string());
        }
        if self.breaks.iter().any(|minutes| *minutes > 120) {
            return Err("Перемена должна быть от 0 до 120 минут".to_string());
        }

        let mut slots = Vec::with_capacity(self.breaks.len() + 1);
        let mut start = self.first_start;
        for index in 0..=self.breaks.len() {
            let end = add_minutes(start, self.lesson_minutes)?;
            slots.push(Bell { start, end });
            if let Some(minutes) = self.breaks.get(index) {
                start = add_minutes(end, *minutes)?;
            }
        }
        Ok(slots)
    }
}

fn add_minutes(time: NaiveTime, minutes: u32) -> Result<NaiveTime, String> {
    let (result, overflow) = time.overflowing_add_signed(Duration::minutes(minutes as i64));
    if overflow != 0 {
        return Err("Звонки должны закончиться до полуночи".to_string());
    }
    Ok(result)
}

struct ClockLabels {
    start: gtk::glib::WeakRef<gtk::Label>,
    end: gtk::glib::WeakRef<gtk::Label>,
}

struct Editor {
    duration: gtk::glib::WeakRef<gtk::DropDown>,
    hour: gtk::glib::WeakRef<gtk::SpinButton>,
    minute: gtk::glib::WeakRef<gtk::SpinButton>,
    count: gtk::glib::WeakRef<gtk::SpinButton>,
    grid: gtk::glib::WeakRef<gtk::Grid>,
    feedback: gtk::glib::WeakRef<gtk::Label>,
    breaks: RefCell<Vec<gtk::glib::WeakRef<gtk::SpinButton>>>,
    rows: RefCell<Vec<ClockLabels>>,
    saved: RefCell<BellSchedule>,
    busy: Cell<bool>,
    on_change: Box<dyn Fn(BellSchedule) -> Result<(), String>>,
}

impl Editor {
    fn rebuild(self: &Rc<Self>, schedule: &BellSchedule) {
        let Some(grid) = self.grid.upgrade() else {
            return;
        };
        while let Some(child) = grid.first_child() {
            grid.remove(&child);
        }
        for (column, title) in ["Урок", "Начало", "Конец", "Перемена, мин"]
            .iter()
            .enumerate()
        {
            grid.attach(&text(title, "muted"), column as i32, 0, 1, 1);
        }

        let mut rows = Vec::new();
        let mut breaks = Vec::new();
        for index in 0..=schedule.breaks.len() {
            let row = index as i32 + 1;
            let number = text(&(index + 1).to_string(), "muted");
            number.set_width_chars(2);
            let start = text("", "section");
            let end = text("", "section");
            start.set_hexpand(true);
            end.set_hexpand(true);
            grid.attach(&number, 0, row, 1, 1);
            grid.attach(&start, 1, row, 1, 1);
            grid.attach(&end, 2, row, 1, 1);
            rows.push(ClockLabels {
                start: start.downgrade(),
                end: end.downgrade(),
            });
            if let Some(minutes) = schedule.breaks.get(index) {
                let spin = number_input(
                    0.0,
                    120.0,
                    *minutes,
                    &format!("Перемена после урока {}", index + 1),
                );
                spin.set_halign(gtk::Align::Start);
                let editor = Rc::clone(self);
                spin.connect_value_changed(move |_| editor.refresh());
                grid.attach(&spin, 3, row, 1, 1);
                breaks.push(spin.downgrade());
            }
        }
        *self.rows.borrow_mut() = rows;
        *self.breaks.borrow_mut() = breaks;
    }

    fn refresh(self: &Rc<Self>) {
        if self.busy.replace(true) {
            return;
        }
        self.refresh_inner();
        self.busy.set(false);
    }

    fn refresh_inner(self: &Rc<Self>) {
        let (Some(duration), Some(hour), Some(minute), Some(count), Some(feedback)) = (
            self.duration.upgrade(),
            self.hour.upgrade(),
            self.minute.upgrade(),
            self.count.upgrade(),
            self.feedback.upgrade(),
        ) else {
            return;
        };
        let Some(inputs) = self
            .breaks
            .borrow()
            .iter()
            .map(|spin| spin.upgrade())
            .collect::<Option<Vec<_>>>()
        else {
            return;
        };
        for spin in [&hour, &minute, &count].into_iter().chain(inputs.iter()) {
            spin.update();
        }
        let mut breaks: Vec<u32> = inputs
            .iter()
            .map(|spin| spin.value_as_int() as u32)
            .collect();
        breaks.resize(count.value_as_int() as usize - 1, 10);
        let candidate = BellSchedule {
            lesson_minutes: if duration.selected() == 0 { 40 } else { 45 },
            first_start: NaiveTime::from_hms_opt(
                hour.value_as_int() as u32,
                minute.value_as_int() as u32,
                0,
            )
            .expect("Переключатели ограничены временем суток"),
            breaks,
        };
        if candidate.breaks.len() != inputs.len() {
            self.rebuild(&candidate);
        }
        match candidate.slots() {
            Ok(slots) => {
                for (row, slot) in self.rows.borrow().iter().zip(slots) {
                    if let Some(label) = row.start.upgrade() {
                        label.set_text(&slot.start.format("%H:%M").to_string());
                    }
                    if let Some(label) = row.end.upgrade() {
                        label.set_text(&slot.end.format("%H:%M").to_string());
                    }
                }
            }
            Err(error) => {
                for row in self.rows.borrow().iter() {
                    if let Some(label) = row.start.upgrade() {
                        label.set_text("");
                    }
                    if let Some(label) = row.end.upgrade() {
                        label.set_text("");
                    }
                }
                feedback.set_text(&error);
                feedback.set_visible(true);
                return;
            }
        }
        if candidate == *self.saved.borrow() {
            feedback.set_visible(false);
            return;
        }
        match (self.on_change)(candidate.clone()) {
            Ok(()) => {
                *self.saved.borrow_mut() = candidate;
                feedback.set_visible(false);
            }
            Err(error) => {
                feedback.set_text(&format!("Не удалось сохранить звонки: {error}"));
                feedback.set_visible(true);
            }
        }
    }
}

pub fn editor(
    initial: &BellSchedule,
    on_change: impl Fn(BellSchedule) -> Result<(), String> + 'static,
) -> gtk::Box {
    let root = column(16);
    root.set_widget_name("bell-settings");
    root.add_css_class("settings-section");
    root.append(&text("Звонки", "settings-title"));
    let controls = gtk::Box::new(gtk::Orientation::Horizontal, 20);
    let length_group = column(8);
    length_group.set_hexpand(true);
    length_group.append(&text("Длительность", "section"));
    let duration = gtk::DropDown::from_strings(&["40 мин", "45 мин"]);
    duration.set_selected(u32::from(initial.lesson_minutes == 45));
    duration.set_tooltip_text(Some("Длительность урока"));
    length_group.append(&duration);
    controls.append(&length_group);

    let first_group = column(8);
    first_group.append(&text("Первый урок", "section"));
    let first = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let hour = clock_input(23.0, initial.first_start.hour(), "Часы начала занятий");
    let minute = clock_input(59.0, initial.first_start.minute(), "Минуты начала занятий");
    first.append(&hour);
    first.append(&text(":", "muted"));
    first.append(&minute);
    first_group.append(&first);
    controls.append(&first_group);

    let count_group = column(8);
    count_group.append(&text("Уроков", "section"));
    let count = number_input(1.0, 12.0, initial.breaks.len() as u32 + 1, "Уроков в день");
    count_group.append(&count);
    controls.append(&count_group);
    root.append(&controls);
    let feedback = text("", "error");
    feedback.set_visible(false);
    root.append(&feedback);
    let grid = gtk::Grid::new();
    grid.set_column_spacing(24);
    grid.set_row_spacing(8);
    grid.add_css_class("bell-table");
    root.append(&grid);

    let editor = Rc::new(Editor {
        duration: duration.downgrade(),
        hour: hour.downgrade(),
        minute: minute.downgrade(),
        count: count.downgrade(),
        grid: grid.downgrade(),
        feedback: feedback.downgrade(),
        breaks: RefCell::new(Vec::new()),
        rows: RefCell::new(Vec::new()),
        saved: RefCell::new(initial.clone()),
        busy: Cell::new(false),
        on_change: Box::new(on_change),
    });
    editor.rebuild(initial);
    for spin in [&hour, &minute, &count] {
        let editor = Rc::clone(&editor);
        spin.connect_value_changed(move |_| editor.refresh());
    }
    let callback = Rc::clone(&editor);
    duration.connect_selected_notify(move |_| callback.refresh());
    editor.refresh();
    root
}

fn number_input(minimum: f64, maximum: f64, value: u32, label: &str) -> gtk::SpinButton {
    let spin = gtk::SpinButton::with_range(minimum, maximum, 1.0);
    spin.set_numeric(true);
    spin.set_width_chars(2);
    spin.set_update_policy(gtk::SpinButtonUpdatePolicy::IfValid);
    spin.set_tooltip_text(Some(label));
    spin.set_value(value as f64);
    spin
}

fn clock_input(maximum: f64, value: u32, label: &str) -> gtk::SpinButton {
    let spin = number_input(0.0, maximum, value, label);
    spin.set_wrap(true);
    spin.connect_output(|spin| {
        spin.set_text(&format!("{:02}", spin.value_as_int()));
        gtk::glib::Propagation::Stop
    });
    spin.set_text(&format!("{value:02}"));
    spin
}
