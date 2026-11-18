use adw::prelude::*;
use chrono::{Datelike, NaiveDate, NaiveTime, Timelike};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::bells::Bell;
use crate::forms::{self, HOMEWORK_LIMIT, ROOM_LIMIT, SUBJECT_LIMIT};
use crate::lessons::{
    DayOptions, HomeworkId, Lesson, LessonOptions, LessonWork, Lessons, MAX_SHORT_MINUTES,
    NOTE_LIMIT, RecordId,
};
use crate::tutorial::Anchor;
use crate::undo::UndoHandle;

use crate::{column, empty_state, field, margins, notify, page, primary, text};

struct ScheduleRows {
    content: gtk::Box,
    rows: RefCell<Vec<LessonRow>>,
    revision: Cell<u64>,
    focus_on_date_change: Cell<Option<NaiveTime>>,
}

struct LessonRow {
    time: gtk::Label,
    room: Option<gtk::Label>,
}

impl ScheduleRows {
    fn update_day(&self, lessons: &[Lesson], previous: DayOptions, options: DayOptions) {
        for (row, lesson) in self.rows.borrow().iter().zip(lessons) {
            if previous.short_minutes != options.short_minutes {
                row.time.set_label(&time_label(lesson, options));
            }
            if previous.remote != options.remote
                && let Some(room) = &row.room
            {
                room.set_visible(!options.remote);
            }
        }
    }
}

pub fn schedule_page(
    status: &gtk::Label,
    lessons: Lessons,
    undo: Option<&UndoHandle>,
) -> (gtk::Box, gtk::Calendar) {
    let page = page("Расписание");
    let calendar = gtk::Calendar::new();
    let date = date_button_with_holidays(&calendar, true);
    Anchor::Date.mark(&date);
    let list = Rc::new(ScheduleRows {
        content: column(0),
        rows: RefCell::new(Vec::new()),
        revision: Cell::new(lessons.revision()),
        focus_on_date_change: Cell::new(None),
    });
    render_lessons(&list, &lessons, &calendar, status, None, undo);

    let toolbar = adw::WrapBox::new();
    toolbar.set_child_spacing(8);
    toolbar.set_line_spacing(8);
    let date_controls = gtk::Box::new(gtk::Orientation::Horizontal, 6);
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
        date_controls.append(&button);
        if offset == -1 {
            date_controls.append(&date);
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
    date_controls.append(&today);
    toolbar.append(&date_controls);
    toolbar.append(&day_options_button(
        &lessons, &calendar, &list, status, undo,
    ));
    let add = primary("Добавить урок");
    Anchor::AddLesson.mark(&add);
    let (items, calendar_for_add, status_for_add, weak_list) = (
        lessons.clone(),
        calendar.clone(),
        status.clone(),
        Rc::downgrade(&list),
    );
    let undo_for_add = undo.cloned();
    add.connect_clicked(move |_| {
        if let Some(list) = weak_list.upgrade() {
            on_add_lesson(
                &status_for_add,
                &items,
                &list,
                &calendar_for_add,
                None,
                undo_for_add.as_ref(),
            );
        }
    });
    toolbar.append(&add);
    page.append(&toolbar);
    page.append(&list.content);

    if let Some(undo) = undo.cloned() {
        let (list, calendar, status) = (
            Rc::downgrade(&list),
            calendar.downgrade(),
            status.downgrade(),
        );
        lessons.connect_restored(move |lessons, removed| {
            let (Some(list), Some(calendar), Some(status)) =
                (list.upgrade(), calendar.upgrade(), status.upgrade())
            else {
                return;
            };
            if !list.content.is_mapped() {
                return;
            }
            let slot = removed.lesson_slot().or_else(|| {
                if let Some(HomeworkId::Linked(id)) = removed.homework_id() {
                    lessons
                        .borrow()
                        .iter()
                        .find(|lesson| lesson.id == id)
                        .map(|lesson| (lesson.date, lesson.time))
                } else {
                    None
                }
            });
            if let Some((date, time)) = slot
                && date == selected_date(&calendar)
            {
                render_lessons(&list, lessons, &calendar, &status, Some(time), Some(&undo));
            }
        });
    }

    let weak_list = Rc::downgrade(&list);
    let status = status.clone();
    let items_for_map = lessons.clone();
    let calendar_for_map = calendar.clone();
    let status_for_map = status.clone();
    let undo_for_map = undo.cloned();
    page.connect_map(move |_| {
        if list.revision.get() != items_for_map.revision() {
            render_lessons(
                &list,
                &items_for_map,
                &calendar_for_map,
                &status_for_map,
                None,
                undo_for_map.as_ref(),
            );
        }
    });

    let undo = undo.cloned();
    // GtkCalendar emits day-selected only when the day number changes.
    calendar.connect_date_notify(move |calendar| {
        if let Some(list) = weak_list.upgrade() {
            let focus = list.focus_on_date_change.take();
            render_lessons(&list, &lessons, calendar, &status, focus, undo.as_ref());
        }
    });
    (page, calendar)
}

fn show_day_options(button: &gtk::MenuButton, remote: &gtk::Image, options: DayOptions) {
    use gtk::accessible::Property;
    remote.set_visible(options.remote);
    button.set_css_classes(if options == DayOptions::default() {
        &["day-options"]
    } else {
        &["day-options", "day-configured"]
    });
    let mut description = Vec::new();
    if options.remote {
        description.push("Весь день дистанционно".to_string());
    }
    if let Some(minutes) = options.short_minutes {
        description.push(format!("Уроки до {minutes} мин"));
    }
    let description = description.join("\n");
    let tooltip = if description.is_empty() {
        "Настройки выбранного дня".to_string()
    } else {
        format!("Настройки выбранного дня\n{description}")
    };
    button.set_tooltip_text(Some(&tooltip));
    button.update_property(&[
        Property::Label("Настройки выбранного дня"),
        Property::Description(&description),
    ]);
}

fn remote_day_icon() -> gtk::Image {
    use gtk::accessible::Property;
    let svg = gtk::glib::Bytes::from_static(include_bytes!("icons/day-remote.svg"));
    let stream = gtk::gio::MemoryInputStream::from_bytes(&svg);
    let icon = match gtk::gdk_pixbuf::Pixbuf::from_stream(&stream, gtk::gio::Cancellable::NONE) {
        Ok(pixbuf) => gtk::Image::from_paintable(Some(&gtk::gdk::Texture::for_pixbuf(&pixbuf))),
        Err(_) => gtk::Image::from_icon_name("computer-symbolic"),
    };
    icon.add_css_class("remote-day");
    icon.set_valign(gtk::Align::Center);
    icon.set_tooltip_text(Some("Весь день дистанционно"));
    icon.update_property(&[Property::Label("Весь день дистанционно")]);
    icon
}

fn day_options_button(
    lessons: &Lessons,
    calendar: &gtk::Calendar,
    list: &Rc<ScheduleRows>,
    status: &gtk::Label,
    undo: Option<&UndoHandle>,
) -> gtk::Box {
    let button = gtk::MenuButton::new();
    button.set_label("День");
    let indicator = remote_day_icon();
    let day_controls = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    day_controls.append(&indicator);
    day_controls.append(&button);
    show_day_options(
        &button,
        &indicator,
        lessons.day_options(selected_date(calendar)),
    );

    let popover = gtk::Popover::new();
    let form = column(10);
    margins(&form, 12);
    let remote = gtk::ToggleButton::with_label("Дистант");
    remote.set_halign(gtk::Align::Start);
    form.append(&remote);
    let shortened = gtk::CheckButton::with_label("Сокращённый день");
    form.append(&shortened);
    let minutes_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let minutes = gtk::SpinButton::with_range(1.0, f64::from(MAX_SHORT_MINUTES), 1.0);
    minutes.set_numeric(true);
    let minutes_label = text("Минут на урок", "section");
    minutes_label.set_mnemonic_widget(Some(&minutes));
    minutes_row.append(&minutes_label);
    minutes_row.append(&minutes);
    form.append(&minutes_row);
    let feedback = text("", "form-error");
    feedback.set_visible(false);
    form.append(&feedback);
    let save = primary("Применить");
    form.append(&save);
    popover.set_child(Some(&form));
    button.set_popover(Some(&popover));

    let weak_button = button.downgrade();
    let weak_indicator = indicator.downgrade();
    let items = lessons.clone();
    calendar.connect_date_notify(move |date| {
        if let (Some(button), Some(indicator)) = (weak_button.upgrade(), weak_indicator.upgrade()) {
            button.popdown();
            show_day_options(&button, &indicator, items.day_options(selected_date(date)));
        }
    });

    let items = lessons.clone();
    let date = calendar.clone();
    let controls = (remote.clone(), shortened.clone(), minutes.clone());
    let show_feedback = feedback.clone();
    popover.connect_show(move |_| {
        show_feedback.set_visible(false);
        let options = items.day_options(selected_date(&date));
        controls.0.set_active(options.remote);
        controls.1.set_active(options.short_minutes.is_some());
        controls.2.set_sensitive(options.short_minutes.is_some());
        controls
            .2
            .set_value(f64::from(options.short_minutes.unwrap_or(30)));
    });
    let weak_minutes = minutes.downgrade();
    shortened.connect_toggled(move |shortened| {
        if let Some(minutes) = weak_minutes.upgrade() {
            minutes.set_sensitive(shortened.is_active());
        }
    });

    let (items, date, weak_list, status, weak_button, undo) = (
        lessons.clone(),
        calendar.clone(),
        Rc::downgrade(list),
        status.clone(),
        button.downgrade(),
        undo.cloned(),
    );
    let weak_indicator = indicator.downgrade();
    let persist = Rc::new(move |options: DayOptions| {
        let day = selected_date(&date);
        let previous = items.day_options(day);
        let previous_revision = items.revision();
        feedback.set_visible(false);
        match items.set_day_options(day, options) {
            Ok(()) => {
                if previous != options {
                    if let (Some(button), Some(indicator)) =
                        (weak_button.upgrade(), weak_indicator.upgrade())
                    {
                        show_day_options(&button, &indicator, options);
                    }
                    if let Some(list) = weak_list.upgrade() {
                        if list.revision.get() == previous_revision {
                            list.update_day(&items.on_date(day), previous, options);
                            list.revision.set(items.revision());
                        } else {
                            render_lessons(&list, &items, &date, &status, None, undo.as_ref());
                        }
                    }
                }
                true
            }
            Err(error) => {
                notify(
                    &feedback,
                    &format!("Не удалось сохранить настройки дня: {error}"),
                );
                false
            }
        }
    });
    let apply_remote = Rc::clone(&persist);
    let items = lessons.clone();
    let date = calendar.clone();
    remote.connect_clicked(move |remote| {
        let mut options = items.day_options(selected_date(&date));
        let previous = options.remote;
        options.remote = remote.is_active();
        if !apply_remote(options) {
            remote.set_active(previous);
        }
    });
    let items = lessons.clone();
    let date = calendar.clone();
    let weak_button = button.downgrade();
    save.connect_clicked(move |_| {
        minutes.update();
        let mut options = items.day_options(selected_date(&date));
        options.short_minutes = shortened.is_active().then(|| minutes.value_as_int() as u16);
        if persist(options)
            && let Some(button) = weak_button.upgrade()
        {
            button.popdown();
        }
    });
    day_controls
}

pub(crate) fn selected_date(calendar: &gtk::Calendar) -> NaiveDate {
    let date = calendar.date();
    NaiveDate::from_ymd_opt(date.year(), date.month() as u32, date.day_of_month() as u32)
        .expect("Не удалось получить выбранную дату")
}

pub(crate) fn select_date(calendar: &gtk::Calendar, date: NaiveDate) {
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

fn add_holiday_details(calendar: &gtk::Calendar, content: &gtk::Box) {
    let caption = gtk::Label::new(None);
    caption.add_css_class("caption");
    caption.set_wrap(true);
    caption.set_max_width_chars(34);
    caption.set_margin_bottom(6);
    content.append(&caption);

    let displayed_month = Cell::new(None);
    let update = move |calendar: &gtk::Calendar| {
        use gtk::accessible::Property;
        let date = selected_date(calendar);
        let month = date.with_day(1).expect("Every month has a first day");
        if displayed_month.replace(Some(month)) != Some(month) {
            calendar.clear_marks();
            for day in 1..=31 {
                if month
                    .with_day(day)
                    .and_then(crate::seasonal::name_on)
                    .is_some()
                {
                    calendar.mark_day(day);
                }
            }
        }
        if let Some(description) = crate::seasonal::date_title(date) {
            caption.set_label(&description);
            calendar.update_property(&[Property::Description(&description)]);
        } else {
            caption.set_label("");
            calendar.reset_property(gtk::AccessibleProperty::Description);
        }
    };
    update(calendar);
    calendar.connect_date_notify(update);
}

pub(crate) fn date_button(calendar: &gtk::Calendar) -> gtk::MenuButton {
    date_button_with_holidays(calendar, false)
}

fn date_button_with_holidays(calendar: &gtk::Calendar, holidays: bool) -> gtk::MenuButton {
    let button = gtk::MenuButton::new();
    button.set_label(&selected_date(calendar).format("%d.%m.%Y").to_string());
    let popover = gtk::Popover::new();
    popover.set_has_arrow(false);
    let preview = gtk::Calendar::new();
    if holidays {
        let content = gtk::Box::new(gtk::Orientation::Vertical, 5);
        content.append(&preview);
        add_holiday_details(&preview, &content);
        popover.set_child(Some(&content));
    } else {
        popover.set_child(Some(&preview));
    }
    button.set_popover(Some(&popover));
    let weak_button = button.downgrade();
    calendar.connect_date_notify(move |calendar| {
        if let Some(button) = weak_button.upgrade() {
            button.set_label(&selected_date(calendar).format("%d.%m.%Y").to_string());
        }
    });
    let weak_calendar = calendar.downgrade();
    let weak_popover = popover.downgrade();
    let select = Rc::new(preview.connect_day_selected(move |preview| {
        if let Some(calendar) = weak_calendar.upgrade() {
            calendar.select_day(&preview.date());
        }
        if let Some(popover) = weak_popover.upgrade() {
            popover.popdown();
        }
    }));
    let committed = calendar.clone();
    let map_selection = Rc::clone(&select);
    preview.connect_map(move |preview| {
        preview.block_signal(&map_selection);
        preview.select_day(&committed.date());
        preview.unblock_signal(&map_selection);
    });
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let (weak_preview, weak_popover, weak_calendar) = (
        preview.downgrade(),
        popover.downgrade(),
        calendar.downgrade(),
    );
    keys.connect_key_pressed(move |_, key, _, modifiers| {
        use gtk::gdk::{Key, ModifierType};
        use gtk::glib::Propagation;
        let Some(preview) = weak_preview.upgrade().filter(|preview| preview.has_focus()) else {
            return Propagation::Proceed;
        };
        if modifiers.intersects(
            ModifierType::CONTROL_MASK | ModifierType::ALT_MASK | ModifierType::SUPER_MASK,
        ) {
            return Propagation::Proceed;
        }
        let offset = match key {
            Key::Left | Key::KP_Left => -1,
            Key::Right | Key::KP_Right => 1,
            Key::Up | Key::KP_Up => -7,
            Key::Down | Key::KP_Down => 7,
            Key::Return | Key::KP_Enter | Key::space | Key::KP_Space => {
                if let Some(calendar) = weak_calendar.upgrade() {
                    calendar.select_day(&preview.date());
                }
                if let Some(popover) = weak_popover.upgrade() {
                    popover.popdown();
                }
                return Propagation::Stop;
            }
            _ => return Propagation::Proceed,
        };
        if let Ok(date) = preview.date().add_days(offset) {
            preview.block_signal(&select);
            preview.select_day(&date);
            preview.unblock_signal(&select);
        }
        Propagation::Stop
    });
    preview.add_controller(keys);
    button
}

fn time_label(lesson: &Lesson, options: DayOptions) -> String {
    format!(
        "{}\n{}",
        lesson.time.format("%H:%M"),
        options.end_time(lesson).format("%H:%M")
    )
}

fn lesson_badge(label: &str, description: &str, kind: &str) -> gtk::Label {
    use gtk::accessible::Property;
    let badge = gtk::Label::new(Some(label));
    badge.add_css_class("lesson-badge");
    badge.add_css_class(kind);
    badge.set_valign(gtk::Align::Center);
    badge.set_tooltip_text(Some(description));
    badge.update_property(&[Property::Label(description)]);
    badge
}

fn lesson_heading(lesson: &Lesson) -> gtk::Widget {
    let replacement = !lesson.options.replacement.is_empty();
    let title = text(
        if replacement {
            &lesson.options.replacement
        } else {
            &lesson.title
        },
        "section",
    );
    title.add_css_class("lesson-title");
    if !replacement && !lesson.options.cancelled && lesson.options.work == LessonWork::Regular {
        return title.upcast();
    }

    let heading = adw::WrapBox::new();
    heading.set_child_spacing(5);
    heading.set_line_spacing(3);
    heading.append(&title);
    if lesson.options.cancelled {
        heading.append(&lesson_badge("Отменён", "Урок отменён", "cancelled"));
    }
    if replacement {
        heading.append(&lesson_badge("Замена", "Предмет заменён", "replacement"));
    }
    let work = match lesson.options.work {
        LessonWork::Regular => None,
        LessonWork::Test => Some(("К/Р", "Контрольная работа")),
        LessonWork::Independent => Some(("С/Р", "Самостоятельная работа")),
    };
    if let Some((label, description)) = work {
        heading.append(&lesson_badge(label, description, "schoolwork"));
    }
    heading.upcast()
}

fn render_lessons(
    list: &Rc<ScheduleRows>,
    lessons: &Lessons,
    calendar: &gtk::Calendar,
    status: &gtk::Label,
    focus_time: Option<NaiveTime>,
    undo: Option<&UndoHandle>,
) {
    while let Some(child) = list.content.first_child() {
        list.content.remove(&child);
    }
    let date = selected_date(calendar);
    let options = lessons.day_options(date);
    let day = lessons.on_date(date);
    list.revision.set(lessons.revision());
    if day.is_empty() {
        list.rows.borrow_mut().clear();
        list.content
            .append(&empty_state("x-office-calendar-symbolic", "Нет уроков"));
        return;
    }
    let mut rows = Vec::with_capacity(day.len());
    for lesson in day.iter() {
        let (id, time_of_lesson) = (lesson.id, lesson.time);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 16);
        row.add_css_class("panel");
        if lesson.options.cancelled {
            row.add_css_class("lesson-cancelled");
        }
        let time = text(&time_label(lesson, options), "muted");
        time.set_width_chars(5);
        time.set_valign(gtk::Align::Center);
        row.append(&time);
        let details = column(4);
        details.add_css_class("lesson-details");
        details.set_hexpand(true);
        details.set_valign(gtk::Align::Center);
        details.append(&lesson_heading(lesson));
        let room = (!lesson.room.trim().is_empty()).then(|| {
            let room = text(&format!("Кабинет {}", lesson.room), "muted");
            room.set_visible(!options.remote);
            details.append(&room);
            room
        });
        rows.push(LessonRow { time, room });
        if !lesson.note.is_empty() {
            let note = text(&format!("Заметка: {}", lesson.note), "muted");
            note.set_lines(2);
            note.set_ellipsize(gtk::pango::EllipsizeMode::End);
            note.set_tooltip_text(Some(&lesson.note));
            details.append(&note);
        }
        if !lesson.homework.is_empty() {
            let label = if lesson.homework_completed {
                "ДЗ выполнено"
            } else {
                "ДЗ"
            };

            details.append(&text(&format!("{label}: {}", lesson.homework), "muted"));
        }
        row.append(&details);
        let mut focus = None;
        for (icon, label, editing) in [
            ("document-edit-symbolic", "Изменить урок", true),
            ("user-trash-symbolic", "Удалить урок", false),
        ] {
            let button = gtk::Button::from_icon_name(icon);
            button.add_css_class("flat");
            button.set_tooltip_text(Some(label));
            button.set_valign(gtk::Align::Center);
            if editing && focus_time == Some(lesson.time) {
                focus = Some(button.clone());
            }
            let (items, calendar, status, weak_list) = (
                lessons.clone(),
                calendar.clone(),
                status.clone(),
                Rc::downgrade(list),
            );
            let undo = undo.cloned();
            button.connect_clicked(move |_| {
                let Some(list) = weak_list.upgrade() else {
                    return;
                };
                if editing {
                    on_add_lesson(&status, &items, &list, &calendar, Some(id), undo.as_ref());
                } else {
                    let items = items.clone();
                    let calendar = calendar.clone();
                    let feedback = status.clone();
                    let undo = undo.clone();
                    crate::confirm_delete(
                        &status,
                        "Удалить урок?",
                        "Урок и его домашнее задание будут удалены.",
                        move || {
                            let result = items.delete_lesson(id);
                            match result {
                                Ok(removed) => {
                                    let focus_time = {
                                        let date = selected_date(&calendar);
                                        let day = items.on_date(date);
                                        day.iter()
                                            .find(|lesson| lesson.time > time_of_lesson)
                                            .or_else(|| day.last())
                                            .map(|lesson| lesson.time)
                                    };
                                    render_lessons(
                                        &list,
                                        &items,
                                        &calendar,
                                        &feedback,
                                        focus_time,
                                        undo.as_ref(),
                                    );
                                    if focus_time.is_none()
                                        && let Some(page) = list.content.parent()
                                    {
                                        page.child_focus(gtk::DirectionType::TabForward);
                                    }
                                    if let Some(undo) = &undo {
                                        undo.offer(&items, removed);
                                    } else {
                                        notify(&feedback, "Урок удалён");
                                    }
                                }
                                Err(error) => {
                                    notify(&feedback, &format!("Не удалось удалить урок: {error}"))
                                }
                            }
                        },
                    );
                }
            });
            row.append(&button);
        }
        list.content.append(&row);
        if let Some(focus) = focus {
            focus.grab_focus();
        }
    }
    *list.rows.borrow_mut() = rows;
}

#[derive(Clone)]
struct TimePicker {
    widget: gtk::Box,
    hour: gtk::SpinButton,
    minute: gtk::SpinButton,
    invalid_hour: Rc<Cell<bool>>,
    invalid_minute: Rc<Cell<bool>>,
}

impl TimePicker {
    fn new(value: NaiveTime, label: &str) -> Self {
        let widget = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let (hour, invalid_hour) = time_spin(23, 1.0, &format!("Часы: {label}"));
        let (minute, invalid_minute) = time_spin(59, 5.0, &format!("Минуты: {label}"));
        widget.append(&hour);
        widget.append(&text(":", "muted"));
        widget.append(&minute);
        let picker = Self {
            widget,
            hour,
            minute,
            invalid_hour,
            invalid_minute,
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
        .expect("Указаны неверные часы или минуты")
    }

    fn set_time(&self, time: NaiveTime) {
        self.hour.set_value(time.hour() as f64);
        self.minute.set_value(time.minute() as f64);
        self.hour.set_text(&format!("{:02}", time.hour()));
        self.minute.set_text(&format!("{:02}", time.minute()));
    }

    fn update(&self) {
        self.hour.update();
        self.minute.update();
    }

    fn has_pending_input(&self) -> bool {
        self.hour.text().parse::<i32>().ok() != Some(self.hour.value_as_int())
            || self.minute.text().parse::<i32>().ok() != Some(self.minute.value_as_int())
    }

    fn invalid_input(&self) -> Option<&gtk::SpinButton> {
        [
            (&self.hour, &self.invalid_hour),
            (&self.minute, &self.invalid_minute),
        ]
        .into_iter()
        .find_map(|(spin, invalid)| invalid.get().then_some(spin))
    }
}

fn time_spin(maximum: u32, step: f64, label: &str) -> (gtk::SpinButton, Rc<Cell<bool>>) {
    let spin = gtk::SpinButton::with_range(0.0, maximum as f64, step);
    spin.set_numeric(true);
    spin.set_wrap(true);
    spin.set_width_chars(2);
    spin.set_tooltip_text(Some(label));
    spin.set_update_policy(gtk::SpinButtonUpdatePolicy::IfValid);
    let invalid = Rc::new(Cell::new(false));
    let parse = move |spin: &gtk::SpinButton| {
        spin.text()
            .parse::<u32>()
            .ok()
            .filter(|value| *value <= maximum)
    };
    let invalid_for_input = Rc::clone(&invalid);
    spin.connect_changed(move |spin| invalid_for_input.set(parse(spin).is_none()));
    spin.connect_input(move |spin| Some(parse(spin).map(f64::from).ok_or(())));
    let invalid_for_output = Rc::clone(&invalid);
    spin.connect_output(move |spin| {
        if !invalid_for_output.get() {
            spin.set_text(&format!("{:02}", spin.value_as_int()));
        }
        gtk::glib::Propagation::Stop
    });
    (spin, invalid)
}

fn suggested_bell(day: &[Lesson], bells: &[Bell]) -> Option<usize> {
    let last_end = day.last().map(|lesson| lesson.end);
    bells
        .iter()
        .position(|bell| last_end.is_none_or(|end| bell.start >= end))
        .or_else(|| {
            bells.iter().position(|bell| {
                !day.iter()
                    .any(|lesson| lesson.time < bell.end && bell.start < lesson.end)
            })
        })
}

fn suggested_time(
    day: &[Lesson],
    bells: &[Bell],
    minutes: i64,
) -> (Option<usize>, NaiveTime, NaiveTime) {
    if let Some(index) = suggested_bell(day, bells) {
        return (Some(index), bells[index].start, bells[index].end);
    }
    let duration = chrono::Duration::minutes(minutes);
    let end_for = |start: NaiveTime| {
        let (end, overflow) = start.overflowing_add_signed(duration);
        (overflow == 0).then_some(end)
    };
    if let Some(last_end) = day.last().map(|lesson| lesson.end) {
        let (start, overflow) = last_end.overflowing_add_signed(chrono::Duration::minutes(10));
        if overflow == 0
            && let Some(end) = end_for(start)
        {
            return (None, start, end);
        }
    }
    for mut start in [bells[0].start, NaiveTime::MIN] {
        for lesson in day {
            if let Some(end) = end_for(start)
                && end <= lesson.time
            {
                return (None, start, end);
            }
            start = start.max(lesson.end);
        }
        if let Some(end) = end_for(start) {
            return (None, start, end);
        }
    }
    (None, bells[0].start, bells[0].end)
}

fn on_add_lesson(
    status: &gtk::Label,
    lessons: &Lessons,
    list: &Rc<ScheduleRows>,
    calendar: &gtk::Calendar,
    id: Option<RecordId>,
    undo: Option<&UndoHandle>,
) {
    let Some(parent) = status
        .root()
        .and_then(|root| root.downcast::<gtk::Window>().ok())
    else {
        return;
    };
    let config = crate::settings::load_settings().unwrap_or_default();
    let lesson_minutes = config.bells.lesson_minutes as i64;
    let bells = match config.bells.slots() {
        Ok(bells) => bells,
        Err(error) => {
            notify(status, &error);
            return;
        }
    };
    let existing = id.and_then(|id| {
        lessons
            .borrow()
            .iter()
            .find(|lesson| lesson.id == id)
            .cloned()
    });
    if id.is_some() && existing.is_none() {
        notify(status, "Урок уже удалён");
        return;
    }
    let date = existing
        .as_ref()
        .map_or_else(|| selected_date(calendar), |lesson| lesson.date);
    let (selected_bell, start, finish) = if let Some(lesson) = &existing {
        (
            bells
                .iter()
                .position(|bell| bell.start == lesson.time && bell.end == lesson.end),
            lesson.time,
            lesson.end,
        )
    } else {
        suggested_time(&lessons.on_date(date), &bells, lesson_minutes)
    };
    let root = column(0);
    let header = adw::HeaderBar::new();
    let heading = if id.is_some() {
        "Изменить урок"
    } else {
        "Добавить урок"
    };
    let heading_label = text(heading, "section");
    heading_label.set_wrap(false);
    header.set_title_widget(Some(&heading_label));
    root.append(&header);
    let form = column(12);
    margins(&form, 24);
    let title = field(&form, "Предмет", "Например, алгебра");
    let title_error = forms::error_label(&form, &title);
    let suggestions = gtk::ListBox::new();
    suggestions.set_selection_mode(gtk::SelectionMode::Single);
    suggestions.set_activate_on_single_click(true);
    suggestions.add_css_class("boxed-list");
    suggestions.set_visible(false);
    form.append(&suggestions);
    let date_picker = gtk::Calendar::new();
    select_date(&date_picker, date);
    form.append(&text("Дата", "section"));
    let date_menu = date_button(&date_picker);
    date_menu.set_halign(gtk::Align::Start);
    form.append(&date_menu);

    form.append(&text("Урок", "section"));
    let mut labels = vec!["Своё время".to_string()];
    labels.extend(
        bells
            .iter()
            .enumerate()
            .map(|(index, bell)| format!("Урок {} · {}", index + 1, bell.start.format("%H:%M"))),
    );
    let strings: Vec<&str> = labels.iter().map(String::as_str).collect();
    let number = gtk::DropDown::from_strings(&strings);
    number.set_widget_name("lesson-number");
    number.set_selected(selected_bell.map_or(0, |index| index as u32 + 1));
    form.append(&number);

    let times = adw::WrapBox::new();
    times.set_child_spacing(24);
    times.set_line_spacing(12);
    let start_picker = TimePicker::new(start, "начало");
    let end_picker = TimePicker::new(finish, "конец");
    for (label, picker) in [("Начало", &start_picker), ("Конец", &end_picker)] {
        let group = column(8);
        group.append(&text(label, "section"));
        group.append(&picker.widget);
        times.append(&group);
    }
    form.append(&times);
    let time_error = forms::error_label(&form, &times);
    let duration = Rc::new(Cell::new(
        existing.as_ref().map_or(lesson_minutes, |lesson| {
            (lesson.end - lesson.time).num_minutes()
        }),
    ));
    let syncing = Rc::new(Cell::new(false));
    let automatic_time = Rc::new(Cell::new(id.is_none()));
    {
        let (start, end, duration, syncing, choices, automatic) = (
            start_picker.clone(),
            end_picker.clone(),
            Rc::clone(&duration),
            Rc::clone(&syncing),
            bells.clone(),
            Rc::clone(&automatic_time),
        );
        number.connect_selected_notify(move |number| {
            if syncing.get() {
                return;
            }
            automatic.set(false);
            let Some(index) = number.selected().checked_sub(1) else {
                return;
            };
            if let Some(bell) = choices.get(index as usize) {
                syncing.set(true);
                duration.set(lesson_minutes);
                start.set_time(bell.start);
                end.set_time(bell.end);
                syncing.set(false);
            }
        });
    }
    for spin in [&start_picker.hour, &start_picker.minute] {
        let (hour, minute, end, duration, syncing, number, automatic) = (
            start_picker.hour.downgrade(),
            start_picker.minute.downgrade(),
            end_picker.clone(),
            Rc::clone(&duration),
            Rc::clone(&syncing),
            number.downgrade(),
            Rc::clone(&automatic_time),
        );
        spin.connect_value_changed(move |_| {
            if syncing.get() {
                return;
            }
            automatic.set(false);
            if let (Some(hour), Some(minute)) = (hour.upgrade(), minute.upgrade()) {
                let start = NaiveTime::from_hms_opt(
                    hour.value_as_int() as u32,
                    minute.value_as_int() as u32,
                    0,
                )
                .unwrap();
                syncing.set(true);
                end.set_time(start + chrono::Duration::minutes(duration.get()));
                syncing.set(false);
                if let Some(number) = number.upgrade() {
                    number.set_selected(0);
                }
            }
        });
    }
    for spin in [&end_picker.hour, &end_picker.minute] {
        let start_hour = start_picker.hour.downgrade();
        let start_minute = start_picker.minute.downgrade();
        let end_hour = end_picker.hour.downgrade();
        let end_minute = end_picker.minute.downgrade();
        let number = number.downgrade();
        let duration = Rc::clone(&duration);
        let syncing = Rc::clone(&syncing);
        let automatic = Rc::clone(&automatic_time);
        spin.connect_value_changed(move |_| {
            if syncing.get() {
                return;
            }
            automatic.set(false);
            if let (Some(sh), Some(sm), Some(eh), Some(em)) = (
                start_hour.upgrade(),
                start_minute.upgrade(),
                end_hour.upgrade(),
                end_minute.upgrade(),
            ) {
                let start = sh.value_as_int() * 60 + sm.value_as_int();
                let end = eh.value_as_int() * 60 + em.value_as_int();
                if end > start {
                    duration.set((end - start) as i64);
                }
            }
            if let Some(number) = number.upgrade() {
                number.set_selected(0);
            }
        });
    }
    let room = field(&form, "Кабинет", "Необязательно");

    let room_error = forms::error_label(&form, &room);
    let (homework_scroll, homework) = forms::homework_editor(100);
    let homework_label = text("Домашнее задание", "section");
    homework_label.set_mnemonic_widget(Some(&homework));
    form.append(&homework_label);
    form.append(&homework_scroll);
    let homework_error = forms::error_label(&form, &homework_scroll);
    let extras = gtk::Expander::new(Some("Заметка, замена и отметки"));
    let extras_form = column(10);
    extras_form.set_margin_top(10);
    let note = field(&extras_form, "Заметка", "Например, принести атлас");
    let note_error = forms::error_label(&extras_form, &note);
    let replacement = field(
        &extras_form,
        "Замена предмета",
        "Оставьте пустым для обычного урока",
    );
    let replacement_error = forms::error_label(&extras_form, &replacement);
    let cancelled = gtk::CheckButton::with_label("Урок отменён");
    extras_form.append(&cancelled);
    let work = gtk::DropDown::from_strings(&[
        "Обычный урок",
        "Контрольная работа",
        "Самостоятельная работа",
    ]);
    let work_label = text("Вид работы", "section");
    work_label.set_mnemonic_widget(Some(&work));
    extras_form.append(&work_label);
    extras_form.append(&work);
    extras.set_child(Some(&extras_form));
    form.append(&extras);

    if let Some(lesson) = &existing {
        note.set_text(&lesson.note);
        let controls = (replacement.clone(), cancelled.clone(), work.clone());
        let original_date = lesson.date;
        let options = lesson.options.clone();
        let sync = move |calendar: &gtk::Calendar| {
            let same_date = selected_date(calendar) == original_date;
            controls
                .0
                .set_text(if same_date { &options.replacement } else { "" });
            controls.1.set_active(same_date && options.cancelled);
            controls.2.set_selected(if same_date {
                work_index(options.work)
            } else {
                0
            });
        };
        sync(&date_picker);
        date_picker.connect_date_notify(sync);
        extras.set_expanded(!lesson.note.is_empty() || lesson.options != LessonOptions::default());
    }

    if let Some(lesson) = &existing {
        title.set_text(&lesson.title);
        room.set_text(&lesson.room);
        homework.buffer().set_text(&lesson.homework);
    }
    connect_subject_suggestions(&title, &room, &suggestions, lessons);

    let feedback = text("", "form-error");
    feedback.set_visible(false);
    for (entry, error) in [
        (&title, &title_error),
        (&room, &room_error),
        (&note, &note_error),
        (&replacement, &replacement_error),
    ] {
        let feedback = feedback.clone();
        let error = error.clone();
        entry.connect_changed(move |_| {
            feedback.set_visible(false);
            error.set_visible(false);
        });
    }
    let editor_error = homework_error.clone();
    homework
        .buffer()
        .connect_changed(move |_| editor_error.set_visible(false));
    for spin in [
        &start_picker.hour,
        &start_picker.minute,
        &end_picker.hour,
        &end_picker.minute,
    ] {
        let feedback = feedback.clone();
        let time_error = time_error.clone();
        spin.connect_changed(move |_| {
            feedback.set_visible(false);
            time_error.set_visible(false);
        });
    }
    {
        let (feedback, items, number, start, end, duration, syncing, bells) = (
            feedback.clone(),
            lessons.clone(),
            number.clone(),
            start_picker.clone(),
            end_picker.clone(),
            Rc::clone(&duration),
            Rc::clone(&syncing),
            bells.clone(),
        );
        date_picker.connect_date_notify(move |calendar| {
            feedback.set_visible(false);
            if !automatic_time.get() || start.has_pending_input() || end.has_pending_input() {
                return;
            }
            let (slot, from, to) = suggested_time(
                &items.on_date(selected_date(calendar)),
                &bells,
                lesson_minutes,
            );
            syncing.set(true);
            number.set_selected(slot.map_or(0, |index| index as u32 + 1));
            start.set_time(from);
            end.set_time(to);
            duration.set(lesson_minutes);
            syncing.set(false);
        });
    }
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    actions.set_halign(gtk::Align::End);
    let cancel = gtk::Button::with_label("Отмена");
    let save = primary(if id.is_some() {
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
    feedback.set_margin_start(24);
    feedback.set_margin_end(24);
    feedback.set_margin_top(8);
    feedback.set_lines(3);
    feedback.set_ellipsize(gtk::pango::EllipsizeMode::End);
    root.append(&feedback);
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
    forms::dialog_keys(&window, &cancel, Some(&save));
    let (items, calendar, status, weak_list, weak_window) = (
        lessons.clone(),
        calendar.clone(),
        status.clone(),
        Rc::downgrade(list),
        window.downgrade(),
    );
    let title_for_focus = title.clone();
    let undo = undo.cloned();
    save.connect_clicked(move |_| {
        for error in [
            &title_error,
            &room_error,
            &note_error,
            &replacement_error,
            &homework_error,
            &time_error,
            &feedback,
        ] {
            error.set_visible(false);
        }
        let title_value = title.text();
        let room_value = room.text();
        let note_value = note.text();
        let replacement_value = replacement.text();
        let buffer = homework.buffer();
        let homework_value = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
        for (value, previous, limit, widget, label, empty) in [
            (
                title_value.trim(),
                existing.as_ref().map_or("", |lesson| lesson.title.trim()),
                SUBJECT_LIMIT,
                title.upcast_ref::<gtk::Widget>(),
                &title_error,
                Some("Введите название предмета"),
            ),
            (
                room_value.trim(),
                existing.as_ref().map_or("", |lesson| lesson.room.trim()),
                ROOM_LIMIT,
                room.upcast_ref::<gtk::Widget>(),
                &room_error,
                None,
            ),
            (
                note_value.trim(),
                "",
                NOTE_LIMIT,
                note.upcast_ref::<gtk::Widget>(),
                &note_error,
                None,
            ),
            (
                replacement_value.trim(),
                "",
                SUBJECT_LIMIT,
                replacement.upcast_ref::<gtk::Widget>(),
                &replacement_error,
                None,
            ),
            (
                homework_value.trim(),
                existing
                    .as_ref()
                    .map_or("", |lesson| lesson.homework.trim()),
                HOMEWORK_LIMIT,
                homework.upcast_ref::<gtk::Widget>(),
                &homework_error,
                None,
            ),
        ] {
            let error = if value.is_empty() {
                empty.map(str::to_owned)
            } else {
                forms::length_error(value, previous, limit)
            };
            if let Some(error) = error {
                if widget == note.upcast_ref::<gtk::Widget>()
                    || widget == replacement.upcast_ref::<gtk::Widget>()
                {
                    extras.set_expanded(true);
                }
                notify(label, &error);
                widget.grab_focus();
                return;
            }
        }
        if let Some(spin) = [&start_picker, &end_picker]
            .into_iter()
            .find_map(TimePicker::invalid_input)
        {
            notify(
                &time_error,
                "Проверьте время. Часы от 0 до 23, минуты от 0 до 59.",
            );
            spin.grab_focus();
            return;
        }
        let pending_start = start_picker.has_pending_input();
        let pending_end = end_picker.has_pending_input();
        syncing.set(true);
        start_picker.update();
        end_picker.update();
        if pending_start && !pending_end {
            end_picker.set_time(start_picker.time() + chrono::Duration::minutes(duration.get()));
        }
        syncing.set(false);
        if let Some(index) = number.selected().checked_sub(1)
            && let Some(bell) = bells.get(index as usize)
            && (start_picker.time() != bell.start || end_picker.time() != bell.end)
        {
            number.set_selected(0);
        }
        if end_picker.time() <= start_picker.time() {
            notify(
                &time_error,
                "Конец урока должен быть позже начала в тот же день",
            );
            end_picker.hour.grab_focus();
            return;
        }
        let date = selected_date(&date_picker);
        let result = Lesson::new(
            title.text().as_str(),
            date,
            start_picker.time(),
            end_picker.time(),
            room.text().as_str(),
        )
        .and_then(|mut lesson| {
            lesson.homework = homework_value.trim().to_string();
            lesson.note = note_value.trim().to_string();
            lesson.options = LessonOptions {
                replacement: replacement_value.trim().to_string(),
                cancelled: cancelled.is_active(),
                work: match work.selected() {
                    1 => LessonWork::Test,
                    2 => LessonWork::Independent,
                    _ => LessonWork::Regular,
                },
            };

            if let Some(previous) = &existing {
                if title_value == previous.title {
                    lesson.title.clone_from(&previous.title);
                }
                if room_value == previous.room {
                    lesson.room.clone_from(&previous.room);
                }
                if homework_value == previous.homework {
                    lesson.homework.clone_from(&previous.homework);
                }
                lesson.homework_completed =
                    previous.homework_completed && previous.homework == lesson.homework;
            }
            items.store_lesson(lesson, id)
        });

        match result {
            Ok(attached) => {
                let focus = id.map(|_| start_picker.time());
                let list = weak_list.upgrade();
                if date != selected_date(&calendar) {
                    if let Some(list) = list {
                        list.focus_on_date_change.set(focus);
                    }
                    select_date(&calendar, date);
                } else if let Some(list) = list {
                    render_lessons(&list, &items, &calendar, &status, focus, undo.as_ref());
                }
                let message = if attached.is_empty() {
                    if id.is_some() {
                        "Урок сохранён".to_string()
                    } else {
                        "Урок добавлен".to_string()
                    }
                } else {
                    items.attachment_notice(&attached)
                };
                notify(&status, &message);
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

fn work_index(work: LessonWork) -> u32 {
    match work {
        LessonWork::Regular => 0,
        LessonWork::Test => 1,
        LessonWork::Independent => 2,
    }
}

fn connect_subject_suggestions(
    title: &gtk::Entry,
    room: &gtk::Entry,
    suggestions: &gtk::ListBox,
    lessons: &Lessons,
) {
    use gtk::gdk::Key;
    use gtk::glib::Propagation;

    let mut known = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for lesson in lessons.borrow().iter().rev() {
        let name = lesson.title.trim();
        let key = name.to_lowercase();
        if !name.is_empty() && seen.insert(key.clone()) {
            known.push((name.to_owned(), lesson.room.clone(), key));
        }
    }

    let matches = Rc::new(RefCell::new(Vec::<(String, String)>::new()));
    {
        let list = suggestions.clone();
        let matches = Rc::clone(&matches);
        title.connect_changed(move |title| {
            while let Some(child) = list.first_child() {
                list.remove(&child);
            }
            matches.borrow_mut().clear();
            let query = title.text().trim().to_lowercase();
            if query.is_empty() {
                list.set_visible(false);
                return;
            }
            let found: Vec<_> = known
                .iter()
                .filter(|(_, _, key)| key.starts_with(&query))
                .take(6)
                .map(|(name, room, _)| (name.clone(), room.clone()))
                .collect();
            for (name, room) in &found {
                let description = if room.trim().is_empty() {
                    name.clone()
                } else {
                    format!("{name} · {room}")
                };
                let label = text(&description, "section");
                margins(&label, 10);
                list.append(&label);
            }
            let has_matches = !found.is_empty();
            *matches.borrow_mut() = found;
            list.select_row(list.row_at_index(0).as_ref());
            list.set_visible(has_matches);
        });
    }
    {
        let title = title.downgrade();
        let room = room.downgrade();
        let matches = Rc::clone(&matches);
        suggestions.connect_row_activated(move |list, row| {
            let chosen = matches.borrow().get(row.index() as usize).cloned();
            let (Some((name, cabinet)), Some(title), Some(room)) =
                (chosen, title.upgrade(), room.upgrade())
            else {
                return;
            };
            title.set_text(&name);
            title.set_position(-1);
            if room.text().trim().is_empty() {
                room.set_text(&cabinet);
            }
            list.set_visible(false);
            title.grab_focus();
        });
    }

    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let list = suggestions.clone();
    keys.connect_key_pressed(move |_, key, _, _| {
        if !list.is_visible() {
            return Propagation::Proceed;
        }
        match key {
            Key::Down | Key::Up => {
                let count = matches.borrow().len() as i32;
                if count > 0 {
                    let current = list.selected_row().map_or(0, |row| row.index());
                    let step = if key == Key::Down { 1 } else { -1 };
                    let next = (current + step).clamp(0, count - 1);
                    list.select_row(list.row_at_index(next).as_ref());
                }
                Propagation::Stop
            }
            Key::Return | Key::KP_Enter => {
                if let Some(row) = list.selected_row() {
                    list.emit_by_name::<()>("row-activated", &[&row]);
                }
                Propagation::Stop
            }
            Key::Escape => {
                list.set_visible(false);
                Propagation::Stop
            }
            Key::Tab | Key::ISO_Left_Tab => {
                list.set_visible(false);
                Propagation::Proceed
            }
            _ => Propagation::Proceed,
        }
    });
    title.add_controller(keys);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lessons::parse_lesson;

    #[test]
    fn suggests_a_free_interval_between_lessons_before_the_first_bell() {
        let items = [
            parse_lesson("Первый", "06.11.2026", "00:00", "01:00", "1").unwrap(),
            parse_lesson("Второй", "06.11.2026", "08:00", "23:55", "1").unwrap(),
        ];
        let bells = crate::bells::BellSchedule::default().slots().unwrap();
        let (slot, start, end) = suggested_time(&items, &bells, 40);
        assert_eq!(slot, None);
        assert_eq!(start, NaiveTime::from_hms_opt(1, 0, 0).unwrap());
        assert_eq!(end, NaiveTime::from_hms_opt(1, 40, 0).unwrap());
    }
}
