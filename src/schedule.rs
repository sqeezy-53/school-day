use adw::prelude::*;
use chrono::{Datelike, NaiveDate, NaiveTime, Timelike};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::lessons::{Lesson, delete_lesson, load_lessons, parse_lesson, store_lesson};
use crate::{column, empty_state, field, margins, notify, page, primary, text};

type Lessons = Rc<RefCell<Vec<Lesson>>>;

pub fn schedule_page(status: &gtk::Label) -> gtk::Box {
    let page = page("Расписание");
    let loaded = match load_lessons() {
        Ok(items) => items,
        Err(error) => {
            // При ошибке не открываем пустой список, чтобы не затереть исходный файл.
            page.append(&text(
                &format!("Не удалось загрузить расписание: {error}"),
                "muted",
            ));
            return page;
        }
    };
    let lessons = Rc::new(RefCell::new(loaded));
    let calendar = gtk::Calendar::new();
    let date = date_button(&calendar);
    let list = column(0);
    render_lessons(&list, &lessons, &calendar, status);

    let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    for (icon, offset, label) in [
        ("go-previous-symbolic", -1, "Предыдущий день"),
        ("go-next-symbolic", 1, "Следующий день"),
    ] {
        let button = gtk::Button::from_icon_name(icon);
        button.add_css_class("flat");
        button.set_tooltip_text(Some(label));
        let calendar = calendar.clone();
        button.connect_clicked(move |_| {
            if let Ok(day) = calendar.date().add_days(offset) {
                calendar.select_day(&day);
            }
        });
        toolbar.append(&button);
        if offset == -1 {
            toolbar.append(&date);
        }
    }
    let today = gtk::Button::with_label("Сегодня");
    today.add_css_class("flat");
    let calendar_for_today = calendar.clone();
    today.connect_clicked(move |_| {
        if let Ok(now) = gtk::glib::DateTime::now_local() {
            calendar_for_today.select_day(&now);
        }
    });
    toolbar.append(&today);
    let spacer = column(0);
    spacer.set_hexpand(true);
    toolbar.append(&spacer);
    let add = primary("Добавить урок");
    let (items, calendar_for_add, status_for_add, weak_list) = (
        Rc::clone(&lessons),
        calendar.clone(),
        status.clone(),
        list.downgrade(),
    );
    add.connect_clicked(move |_| {
        if let Some(list) = weak_list.upgrade() {
            on_add_lesson(&status_for_add, &items, &list, &calendar_for_add, None);
        }
    });
    toolbar.append(&add);
    page.append(&toolbar);
    page.append(&list);

    let weak_list = list.downgrade();
    let status = status.clone();
    calendar.connect_day_selected(move |calendar| {
        if let Some(list) = weak_list.upgrade() {
            render_lessons(&list, &lessons, calendar, &status);
        }
    });
    page
}

fn selected_date(calendar: &gtk::Calendar) -> NaiveDate {
    let date = calendar.date();
    NaiveDate::from_ymd_opt(date.year(), date.month() as u32, date.day_of_month() as u32)
        .expect("GTK Calendar возвращает существующую дату")
}

fn select_date(calendar: &gtk::Calendar, date: NaiveDate) {
    if let Ok(date) = gtk::glib::DateTime::from_local(
        date.year(),
        date.month() as i32,
        date.day() as i32,
        12,
        0,
        0.0,
    ) {
        calendar.select_day(&date);
    }
}

fn date_button(calendar: &gtk::Calendar) -> gtk::MenuButton {
    let button = gtk::MenuButton::new();
    button.set_label(&selected_date(calendar).format("%d.%m.%Y").to_string());
    let popover = gtk::Popover::new();
    popover.set_child(Some(calendar));
    button.set_popover(Some(&popover));
    let weak_button = button.downgrade();
    let weak_popover = popover.downgrade();
    calendar.connect_day_selected(move |calendar| {
        if let Some(button) = weak_button.upgrade() {
            button.set_label(&selected_date(calendar).format("%d.%m.%Y").to_string());
        }
        if let Some(popover) = weak_popover.upgrade() {
            popover.popdown();
        }
    });
    button
}

fn render_lessons(
    list: &gtk::Box,
    lessons: &Lessons,
    calendar: &gtk::Calendar,
    status: &gtk::Label,
) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    // Сохраняем исходные индексы: в окне показываются уроки только выбранной даты.
    let day: Vec<_> = lessons
        .borrow()
        .iter()
        .enumerate()
        .filter(|(_, lesson)| lesson.date == selected_date(calendar))
        .map(|(index, lesson)| (index, lesson.clone()))
        .collect();
    if day.is_empty() {
        list.append(&empty_state(
            "x-office-calendar-symbolic",
            "Нет уроков",
            "Добавьте урок на выбранную дату.",
        ));
        return;
    }
    for (index, lesson) in day {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 16);
        row.add_css_class("panel");
        let time = text(
            &format!(
                "{}\n{}",
                lesson.time.format("%H:%M"),
                lesson.end.format("%H:%M")
            ),
            "muted",
        );
        time.set_width_chars(5);
        time.set_valign(gtk::Align::Center);
        row.append(&time);
        let details = column(4);
        details.set_hexpand(true);
        details.set_valign(gtk::Align::Center);
        details.append(&text(&lesson.title, "section"));
        details.append(&text(&format!("Кабинет {}", lesson.room), "muted"));
        row.append(&details);
        for (icon, label, editing) in [
            ("document-edit-symbolic", "Изменить урок", true),
            ("user-trash-symbolic", "Удалить урок", false),
        ] {
            let button = gtk::Button::from_icon_name(icon);
            button.add_css_class("flat");
            button.set_tooltip_text(Some(label));
            button.set_valign(gtk::Align::Center);
            let (items, calendar, status, weak_list) = (
                Rc::clone(lessons),
                calendar.clone(),
                status.clone(),
                list.downgrade(),
            );
            button.connect_clicked(move |_| {
                let Some(list) = weak_list.upgrade() else {
                    return;
                };
                if editing {
                    on_add_lesson(&status, &items, &list, &calendar, Some(index));
                } else {
                    let result = delete_lesson(&mut items.borrow_mut(), index);
                    match result {
                        Ok(()) => {
                            render_lessons(&list, &items, &calendar, &status);
                            notify(&status, "Урок удалён");
                        }
                        Err(error) => notify(&status, &format!("Не удалось удалить урок: {error}")),
                    }
                }
            });
            row.append(&button);
        }
        list.append(&row);
    }
}

#[derive(Clone)]
struct TimePicker {
    widget: gtk::Box,
    hour: gtk::SpinButton,
    minute: gtk::SpinButton,
}

impl TimePicker {
    fn new(value: NaiveTime, label: &str) -> Self {
        let widget = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let hour = time_spin(23.0, 1.0, &format!("Часы: {label}"));
        let minute = time_spin(59.0, 5.0, &format!("Минуты: {label}"));
        widget.append(&hour);
        widget.append(&text(":", "muted"));
        widget.append(&minute);
        let picker = Self {
            widget,
            hour,
            minute,
        };
        picker.set_time(value);
        picker
    }

    fn time(&self) -> NaiveTime {
        NaiveTime::from_hms_opt(
            self.hour.value_as_int() as u32,
            self.minute.value_as_int() as u32,
            0,
        )
        .expect("Диапазоны переключателей ограничены временем суток")
    }

    fn set_time(&self, time: NaiveTime) {
        self.hour.set_value(time.hour() as f64);
        self.minute.set_value(time.minute() as f64);
    }

    fn update(&self) {
        self.hour.update();
        self.minute.update();
    }
}

fn time_spin(maximum: f64, step: f64, label: &str) -> gtk::SpinButton {
    let spin = gtk::SpinButton::with_range(0.0, maximum, step);
    spin.set_numeric(true);
    spin.set_wrap(true);
    spin.set_width_chars(2);
    spin.set_tooltip_text(Some(label));
    spin.set_update_policy(gtk::SpinButtonUpdatePolicy::IfValid);
    spin.connect_output(|spin| {
        spin.set_text(&format!("{:02}", spin.value_as_int()));
        gtk::glib::Propagation::Stop
    });
    spin
}

fn suggested_time(items: &[Lesson], date: NaiveDate) -> NaiveTime {
    items
        .iter()
        .filter(|lesson| lesson.date == date)
        .map(|lesson| lesson.end)
        .max()
        .and_then(|end| {
            let (next, overflow) = end.overflowing_add_signed(chrono::Duration::minutes(10));
            (overflow == 0).then_some(next)
        })
        .unwrap_or_else(|| NaiveTime::from_hms_opt(8, 0, 0).unwrap())
}

fn on_add_lesson(
    status: &gtk::Label,
    lessons: &Lessons,
    list: &gtk::Box,
    calendar: &gtk::Calendar,
    index: Option<usize>,
) {
    let Some(parent) = status
        .root()
        .and_then(|root| root.downcast::<gtk::Window>().ok())
    else {
        return;
    };
    let existing = index.and_then(|index| lessons.borrow().get(index).cloned());
    let date = existing
        .as_ref()
        .map_or_else(|| selected_date(calendar), |lesson| lesson.date);
    let start = existing.as_ref().map_or_else(
        || suggested_time(&lessons.borrow(), date),
        |lesson| lesson.time,
    );
    let finish = existing
        .as_ref()
        .map_or(start + chrono::Duration::minutes(45), |lesson| lesson.end);
    let root = column(0);
    let header = adw::HeaderBar::new();
    let heading = if index.is_some() {
        "Изменить урок"
    } else {
        "Добавить урок"
    };
    header.set_title_widget(Some(&text(heading, "section")));
    root.append(&header);
    let form = column(18);
    margins(&form, 24);
    let title = field(&form, "Предмет", "Например, алгебра");
    let date_picker = gtk::Calendar::new();
    select_date(&date_picker, date);
    form.append(&text("Дата", "section"));
    let date_menu = date_button(&date_picker);
    date_menu.set_halign(gtk::Align::Start);
    form.append(&date_menu);

    let times = gtk::Box::new(gtk::Orientation::Horizontal, 24);
    let start_picker = TimePicker::new(start, "начало");
    let end_picker = TimePicker::new(finish, "конец");
    for (label, picker) in [("Начало", &start_picker), ("Конец", &end_picker)] {
        let group = column(8);
        group.append(&text(label, "section"));
        group.append(&picker.widget);
        times.append(&group);
    }
    form.append(&times);
    let durations = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    durations.append(&text("Длительность", "muted"));
    let duration = Rc::new(Cell::new(
        existing
            .as_ref()
            .map_or(45, |lesson| (lesson.end - lesson.time).num_minutes()),
    ));
    for minutes in [40, 45] {
        let button = gtk::Button::with_label(&format!("{minutes} мин"));
        button.add_css_class("flat");
        let (start, end, duration) = (
            start_picker.clone(),
            end_picker.clone(),
            Rc::clone(&duration),
        );
        button.connect_clicked(move |_| {
            duration.set(minutes);
            end.set_time(start.time() + chrono::Duration::minutes(minutes));
        });
        durations.append(&button);
    }
    form.append(&durations);
    // После изменения начала сохраняем выбранную длительность урока.
    for spin in [&start_picker.hour, &start_picker.minute] {
        let (hour, minute, end, duration) = (
            start_picker.hour.downgrade(),
            start_picker.minute.downgrade(),
            end_picker.clone(),
            Rc::clone(&duration),
        );
        spin.connect_value_changed(move |_| {
            if let (Some(hour), Some(minute)) = (hour.upgrade(), minute.upgrade()) {
                let start = NaiveTime::from_hms_opt(
                    hour.value_as_int() as u32,
                    minute.value_as_int() as u32,
                    0,
                )
                .unwrap();
                end.set_time(start + chrono::Duration::minutes(duration.get()));
            }
        });
    }
    let room = field(&form, "Кабинет", "Например, 43 или спортзал");
    if let Some(lesson) = &existing {
        title.set_text(&lesson.title);
        room.set_text(&lesson.room);
    }
    let feedback = text("", "muted");
    feedback.set_visible(false);
    for entry in [&title, &room] {
        let feedback = feedback.clone();
        entry.connect_changed(move |_| feedback.set_visible(false));
    }
    for spin in [
        &start_picker.hour,
        &start_picker.minute,
        &end_picker.hour,
        &end_picker.minute,
    ] {
        let feedback = feedback.clone();
        spin.connect_value_changed(move |_| feedback.set_visible(false));
    }
    let feedback_for_date = feedback.clone();
    date_picker.connect_day_selected(move |_| feedback_for_date.set_visible(false));
    form.append(&feedback);
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    actions.set_halign(gtk::Align::End);
    let cancel = gtk::Button::with_label("Отмена");
    let save = primary(if index.is_some() {
        "Сохранить"
    } else {
        "Добавить"
    });
    actions.append(&cancel);
    actions.append(&save);
    actions.set_margin_start(24);
    actions.set_margin_end(24);
    actions.set_margin_top(12);
    actions.set_margin_bottom(16);
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_vexpand(true);
    scroll.set_child(Some(&form));
    root.append(&scroll);
    // Действия остаются видимыми даже в небольшом окне с прокруткой.
    root.append(&actions);
    let window = adw::Window::builder()
        .title(heading)
        .default_width(560)
        .default_height(640)
        .modal(true)
        .transient_for(&parent)
        .content(&root)
        .build();
    window.set_destroy_with_parent(true);
    let weak_window = window.downgrade();
    cancel.connect_clicked(move |_| {
        if let Some(window) = weak_window.upgrade() {
            window.close();
        }
    });
    let escape = gtk::EventControllerKey::new();
    let weak_window = window.downgrade();
    escape.connect_key_pressed(move |_, key, _, _| {
        if key == gtk::gdk::Key::Escape {
            if let Some(window) = weak_window.upgrade() {
                window.close();
            }
            return gtk::glib::Propagation::Stop;
        }
        gtk::glib::Propagation::Proceed
    });
    window.add_controller(escape);
    let (items, calendar, status, weak_list, weak_window) = (
        Rc::clone(lessons),
        calendar.clone(),
        status.clone(),
        list.downgrade(),
        window.downgrade(),
    );
    let title_for_focus = title.clone();
    save.connect_clicked(move |_| {
        start_picker.update();
        end_picker.update();
        let date = selected_date(&date_picker);
        let result = parse_lesson(
            title.text().as_str(),
            &date.format("%d.%m.%Y").to_string(),
            &start_picker.time().format("%H:%M").to_string(),
            &end_picker.time().format("%H:%M").to_string(),
            room.text().as_str(),
        )
        .and_then(|lesson| store_lesson(&mut items.borrow_mut(), lesson, index));
        match result {
            Ok(()) => {
                select_date(&calendar, date);
                if let Some(list) = weak_list.upgrade() {
                    render_lessons(&list, &items, &calendar, &status);
                }
                notify(
                    &status,
                    if index.is_some() {
                        "Урок сохранён"
                    } else {
                        "Урок добавлен"
                    },
                );
                if let Some(window) = weak_window.upgrade() {
                    window.close();
                }
            }
            Err(error) => notify(&feedback, &error),
        }
    });
    window.present();
    title_for_focus.grab_focus();
}
