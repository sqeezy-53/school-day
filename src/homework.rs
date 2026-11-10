use adw::prelude::*;
use chrono::{Datelike, NaiveDate};
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use crate::forms::{self, HOMEWORK_LIMIT, SUBJECT_LIMIT, length_error};
use crate::lessons::{Homework, HomeworkId, Lessons, RecordId, same_subject};
use crate::schedule::{date_button, select_date, selected_date};
use crate::settings::{Settings, load_settings};
use crate::tutorial::Anchor;
use crate::undo::UndoHandle;
use crate::{column, field, margins, notify, page, primary, text};

pub fn homework_page(status: &gtk::Label, lessons: Lessons, undo: Option<&UndoHandle>) -> gtk::Box {
    let page = page("Домашние задания");
    let add = primary("Добавить ДЗ");
    Anchor::AddHomework.mark(&add);
    page.append(&add);

    let open = column(0);
    page.append(&text("К выполнению", "settings-title"));
    page.append(&open);
    let done = column(0);
    let completed = gtk::Expander::new(None);
    completed.set_child(Some(&done));
    page.append(&completed);

    let list = Rc::new(HomeworkList {
        lessons,
        status: status.clone(),
        add: add.clone(),
        open,
        done,
        completed,
        undo: undo.cloned(),
    });
    let weak_list = Rc::downgrade(&list);
    add.connect_clicked(move |_| {
        if let Some(list) = weak_list.upgrade() {
            list.edit(None);
        }
    });

    if undo.is_some() {
        let weak_list = Rc::downgrade(&list);
        list.lessons.connect_restored(move |_, removed| {
            if let (Some(list), Some(id)) = (weak_list.upgrade(), removed.homework_id())
                && list.open.is_mapped()
            {
                list.render(Some(RowFocus::Edit(id)));
            }
        });
    }
    list.render(None);
    page.connect_map(move |_| list.render(None));
    page
}

#[derive(Clone, Copy)]
enum RowFocus {
    Check(HomeworkId),
    Edit(HomeworkId),
    Add,
}

struct HomeworkList {
    lessons: Lessons,
    status: gtk::Label,
    add: gtk::Button,
    open: gtk::Box,
    done: gtk::Box,
    completed: gtk::Expander,
    undo: Option<UndoHandle>,
}

impl HomeworkList {
    fn render(self: &Rc<Self>, focus: Option<RowFocus>) {
        for group in [&self.open, &self.done] {
            while let Some(row) = group.first_child() {
                group.remove(&row);
            }
        }

        let tasks = self.lessons.homework();
        let done = tasks.iter().filter(|task| task.completed).count();
        self.completed
            .set_label(Some(&format!("Выполнено ({done})")));
        if done == 0 {
            self.completed.set_expanded(false);
        }
        self.completed.set_sensitive(done > 0);

        if tasks.len() == done {
            let message = if done == 0 {
                "Заданий пока нет."
            } else {
                "Все задания выполнены."
            };
            self.open.append(&text(message, "muted"));
        }

        for task in tasks {
            self.append_task(task, focus);
        }
        if matches!(focus, Some(RowFocus::Add)) {
            self.add.grab_focus();
        }
    }

    fn append_task(self: &Rc<Self>, task: Homework, focus: Option<RowFocus>) {
        let id = task.id;
        let task_is_done = task.completed;
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        row.add_css_class("panel");

        let checkbox = gtk::CheckButton::new();
        Anchor::CompleteHomework.mark(&checkbox);
        checkbox.set_tooltip_text(Some(&format!("ДЗ выполнено: {}", task.title)));
        checkbox.set_valign(gtk::Align::Start);
        checkbox.set_active(task.completed);

        let details = column(6);
        details.set_hexpand(true);
        let mut title = task.title;
        if let Some(date) = task.date {
            title.push_str(&format!(" · {}", date.format("%d.%m.%Y")));
        }
        details.append(&text(&title, "section"));
        if let HomeworkId::Linked(lesson) = id {
            if let Some(lesson) = self.lessons.borrow().iter().find(|item| item.id == lesson) {
                details.append(&text(
                    &format!("Урок в {}", lesson.time.format("%H:%M")),
                    "muted",
                ));
            }
        } else {
            details.append(&text("Без урока", "section"));
        }
        details.append(&text(&task.body, "muted"));

        let edit = gtk::Button::from_icon_name("document-edit-symbolic");
        edit.add_css_class("flat");
        edit.set_tooltip_text(Some("Изменить ДЗ"));
        edit.set_valign(gtk::Align::Start);
        let weak_list = Rc::downgrade(self);
        edit.connect_clicked(move |_| {
            if let Some(list) = weak_list.upgrade() {
                list.edit(Some(id));
            }
        });

        let delete = gtk::Button::from_icon_name("user-trash-symbolic");
        delete.add_css_class("flat");
        delete.set_tooltip_text(Some("Удалить ДЗ"));
        delete.set_valign(gtk::Align::Start);
        let weak_list = Rc::downgrade(self);
        delete.connect_clicked(move |_| {
            let Some(list) = weak_list.upgrade() else {
                return;
            };
            let body = match id {
                HomeworkId::Linked(_) => "Сам урок останется в расписании.",
                HomeworkId::Pending(_) => "Задание будет удалено из списка.",
            };
            let status = list.status.clone();
            crate::confirm_delete(
                &status,
                "Удалить домашнее задание?",
                body,
                move || {
                    let mut tasks = list
                        .lessons
                        .homework()
                        .into_iter()
                        .filter(|task| task.completed == task_is_done);
                    let mut previous = None;
                    while let Some(task) = tasks.next() {
                        if task.id == id {
                            previous = tasks.next().map(|task| task.id).or(previous);
                            break;
                        }
                        previous = Some(task.id);
                    }
                    match list.lessons.delete_homework(id) {
                        Ok(removed) => {
                            list.render(Some(previous.map_or(RowFocus::Add, RowFocus::Edit)));
                            if let Some(undo) = &list.undo {
                                undo.offer(&list.lessons, removed);
                            } else {
                                notify(&list.status, "Домашнее задание удалено");
                            }
                        }
                        Err(error) => {
                            notify(&list.status, &format!("Не удалось удалить ДЗ: {error}"))
                        }
                    }
                },
            );
        });

        let weak_list = Rc::downgrade(self);
        let rolling_back = std::cell::Cell::new(false);
        checkbox.connect_toggled(move |checkbox| {
            let Some(list) = weak_list.upgrade() else {
                return;
            };
            if rolling_back.get() {
                return;
            }
            let active = checkbox.is_active();
            match list.lessons.set_homework_completed(id, active) {
                Ok(()) => {
                    list.render(Some(RowFocus::Check(id)));
                    notify(&list.status, "Сохранено");
                }
                Err(error) => {
                    rolling_back.set(true);
                    checkbox.set_active(!active);
                    rolling_back.set(false);
                    notify(&list.status, &format!("Не удалось сохранить: {error}"));
                }
            }
        });

        row.append(&checkbox);
        row.append(&details);
        row.append(&edit);
        row.append(&delete);
        let group = if task.completed {
            &self.done
        } else {
            &self.open
        };
        group.append(&row);
        match focus {
            Some(RowFocus::Check(wanted) | RowFocus::Edit(wanted)) if wanted == id => {
                if task.completed && !self.completed.is_expanded() {
                    self.completed.grab_focus();
                } else if matches!(focus, Some(RowFocus::Check(_))) {
                    checkbox.grab_focus();
                } else {
                    edit.grab_focus();
                }
            }
            _ => {}
        }
    }

    fn edit(self: &Rc<Self>, id: Option<HomeworkId>) {
        let Some(parent) = self
            .status
            .root()
            .and_then(|root| root.downcast::<gtk::Window>().ok())
        else {
            return;
        };
        let existing = id.and_then(|id| {
            self.lessons
                .homework()
                .into_iter()
                .find(|task| task.id == id)
                .map(Rc::new)
        });
        if id.is_some() && existing.is_none() {
            notify(&self.status, "Задание уже удалено");
            return;
        }
        let settings = if self.lessons.is_practice() {
            Settings::default()
        } else {
            match load_settings() {
                Ok(settings) => settings,
                Err(error) => {
                    notify(
                        &self.status,
                        &format!("Не удалось загрузить настройки: {error}. Показаны все уроки."),
                    );
                    Settings::default()
                }
            }
        };
        let today = chrono::Local::now().date_naive();
        let initial_date =
            initial_lesson_date(&self.lessons, existing.as_deref(), &settings, today);
        let calendar = gtk::Calendar::new();
        select_date(&calendar, initial_date);
        let choices = lesson_choices(
            &self.lessons,
            existing.as_deref(),
            &settings,
            today,
            false,
            Some(initial_date),
        );
        if choices.is_empty() {
            notify(&self.status, "Сначала добавьте урок в расписании.");
            return;
        }
        let labels: Vec<_> = choices.iter().map(|(_, label)| label.as_str()).collect();
        let selector = gtk::DropDown::from_strings(&labels);
        selector.set_enable_search(choices.len() > 5);
        selector.set_sensitive(choices.len() > 1);
        let choices = Rc::new(RefCell::new(choices));

        let heading = if id.is_some() {
            "Изменить ДЗ"
        } else {
            "Добавить ДЗ"
        };
        let root = column(0);
        let header = adw::HeaderBar::new();
        header.set_title_widget(Some(&text(heading, "section")));
        root.append(&header);

        let form = column(24);
        margins(&form, 20);
        let date_group = column(8);
        date_group.append(&text("Дата урока", "section"));
        let dates = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let date_menu = date_button(&calendar);
        date_menu.set_hexpand(true);
        for (icon, offset, label) in [
            ("go-previous-symbolic", -1, "Предыдущий день"),
            ("go-next-symbolic", 1, "Следующий день"),
        ] {
            let button = gtk::Button::from_icon_name(icon);
            button.set_tooltip_text(Some(label));
            let weak_calendar = calendar.downgrade();
            button.connect_clicked(move |_| {
                if let Some(calendar) = weak_calendar.upgrade()
                    && let Ok(date) = calendar.date().add_days(offset)
                {
                    calendar.select_day(&date);
                }
            });
            dates.append(&button);
            if offset == -1 {
                dates.append(&date_menu);
            }
        }
        let current_day = gtk::Button::with_label("Сегодня");
        let weak_calendar = calendar.downgrade();
        current_day.connect_clicked(move |_| {
            if let Some(calendar) = weak_calendar.upgrade() {
                select_date(&calendar, chrono::Local::now().date_naive());
            }
        });
        dates.append(&current_day);
        dates.set_sensitive(!matches!(id, Some(HomeworkId::Linked(_))));
        date_group.append(&dates);
        form.append(&date_group);
        let lesson_group = column(8);
        let selection_heading = adw::WrapBox::new();
        selection_heading.set_child_spacing(16);
        selection_heading.set_line_spacing(8);
        let selector_label = text("Урок в этот день", "section");
        selector_label.set_hexpand(true);
        selector_label.set_mnemonic_widget(Some(&selector));
        selection_heading.append(&selector_label);
        let show_all = gtk::CheckButton::with_label("Показать все уроки");
        let has_filter = settings.hide_old_lessons && !matches!(id, Some(HomeworkId::Linked(_)));
        show_all.set_visible(has_filter);
        selection_heading.append(&show_all);
        lesson_group.append(&selection_heading);
        lesson_group.append(&selector);
        let has_lessons = choices.borrow().iter().any(|(id, _)| id.is_some());
        let empty_day = text("Урока ещё нет", "section");
        empty_day.set_visible(!has_lessons);
        selector.set_visible(has_lessons);
        selector_label.set_visible(has_lessons);
        selection_heading.set_visible(has_lessons || has_filter);
        lesson_group.append(&empty_day);
        form.append(&lesson_group);

        let standalone = column(8);
        let subject = field(&standalone, "Предмет", "Например, алгебра");
        let subject_error = forms::error_label(&standalone, &subject);
        let date_details = column(8);
        let due = gtk::Entry::builder().placeholder_text("ДД.ММ.ГГГГ").build();
        date_details.append(&due);
        let due_error = forms::error_label(&date_details, &due);
        let optional_date = gtk::Expander::new(Some("Дата (необязательно)"));
        optional_date.set_child(Some(&date_details));
        optional_date.set_expanded(existing.as_ref().is_some_and(|task| task.date.is_some()));
        let weak_optional_date = optional_date.downgrade();
        due.connect_changed(move |due| {
            if let Some(expander) = weak_optional_date.upgrade() {
                let value = due.text();
                let label = if value.trim().is_empty() {
                    "Дата (необязательно)".to_string()
                } else {
                    format!("Дата: {}", value.trim())
                };
                expander.set_label(Some(&label));
            }
        });
        standalone.set_visible(choices.borrow()[0].0.is_none());
        optional_date.set_visible(standalone.get_visible());
        let selected_choices = Rc::clone(&choices);
        let standalone_section = standalone.clone();
        let optional_date_section = optional_date.clone();
        selector.connect_selected_item_notify(move |selector| {
            let unlinked = selected_choices
                .borrow()
                .get(selector.selected() as usize)
                .is_some_and(|(id, _)| id.is_none());
            standalone_section.set_visible(unlinked);
            optional_date_section.set_visible(unlinked);
        });
        let all_choices = Rc::clone(&choices);
        let lesson_list = self.lessons.clone();
        let selected_task = existing.clone();
        let lesson_selector = selector.downgrade();
        let weak_empty_day = empty_day.downgrade();
        let weak_selector_label = selector_label.downgrade();
        let weak_selection_heading = selection_heading.downgrade();
        let rebuild: Rc<dyn Fn(NaiveDate, bool)> = Rc::new(move |date, show_all| {
            let Some(lesson_selector) = lesson_selector.upgrade() else {
                return;
            };
            let selected = all_choices
                .borrow()
                .get(lesson_selector.selected() as usize)
                .map(|(id, _)| *id);
            let updated = lesson_choices(
                &lesson_list,
                selected_task.as_deref(),
                &settings,
                chrono::Local::now().date_naive(),
                show_all,
                Some(date),
            );
            let position = updated
                .iter()
                .position(|(id, _)| Some(*id) == selected)
                .unwrap_or(0) as u32;
            let has_lessons = updated.iter().any(|(id, _)| id.is_some());
            lesson_selector.set_visible(has_lessons);
            if let Some(label) = weak_empty_day.upgrade() {
                label.set_visible(!has_lessons);
            }
            if let Some(label) = weak_selector_label.upgrade() {
                label.set_visible(has_lessons);
            }
            if let Some(heading) = weak_selection_heading.upgrade() {
                heading.set_visible(has_lessons || has_filter);
            }
            let labels: Vec<_> = updated.iter().map(|(_, label)| label.as_str()).collect();
            let model = gtk::StringList::new(&labels);
            lesson_selector.set_enable_search(updated.len() > 5);
            lesson_selector.set_sensitive(updated.len() > 1);
            *all_choices.borrow_mut() = updated;
            lesson_selector.set_model(Some(&model));
            lesson_selector.set_selected(position);
        });
        let callback = Rc::clone(&rebuild);
        let weak_show_all = show_all.downgrade();
        calendar.connect_day_selected(move |calendar| {
            let show_all = weak_show_all
                .upgrade()
                .is_some_and(|button| button.is_active());
            callback(selected_date(calendar), show_all);
        });
        let weak_calendar = calendar.downgrade();
        show_all.connect_toggled(move |button| {
            if let Some(calendar) = weak_calendar.upgrade() {
                rebuild(selected_date(&calendar), button.is_active());
            }
        });
        form.append(&standalone);

        let editor_group = column(8);
        editor_group.set_vexpand(true);
        let (scroll, editor) = forms::homework_editor(112);
        scroll.set_vexpand(true);
        let buffer = editor.buffer();
        if let Some(task) = &existing {
            buffer.set_text(&task.body);
            if matches!(task.id, HomeworkId::Pending(_)) {
                subject.set_text(&task.title);
                if let Some(date) = task.date {
                    due.set_text(&date.format("%d.%m.%Y").to_string());
                }
            }
        }
        let editor_label = text("Задание", "section");
        editor_label.set_mnemonic_widget(Some(&editor));
        editor_group.append(&editor_label);
        editor_group.append(&scroll);
        let feedback = forms::error_label(&editor_group, &scroll);
        form.append(&editor_group);
        form.append(&optional_date);

        for (entry, error) in [(&subject, &subject_error), (&due, &due_error)] {
            let error = error.clone();
            entry.connect_changed(move |_| error.set_visible(false));
        }
        let editor_feedback = feedback.clone();
        buffer.connect_changed(move |_| editor_feedback.set_visible(false));
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        actions.set_halign(gtk::Align::End);
        let cancel = gtk::Button::with_label("Отмена");
        let save = primary("Сохранить");
        actions.append(&cancel);
        actions.append(&save);
        let form_scroll = gtk::ScrolledWindow::new();
        form_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        form_scroll.set_vexpand(true);
        form_scroll.set_child(Some(&form));
        root.append(&form_scroll);
        actions.set_margin_start(24);
        actions.set_margin_end(24);
        actions.set_margin_top(12);
        actions.set_margin_bottom(16);
        root.append(&actions);

        let window = adw::Window::builder()
            .title(heading)
            .default_width(520)
            .default_height(600)
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

        let initial_focus: gtk::Widget = if selector.is_sensitive() {
            selector.clone().upcast()
        } else if standalone.get_visible() {
            subject.clone().upcast()
        } else {
            editor.clone().upcast()
        };
        let weak_list = Rc::downgrade(self);
        let weak_window = window.downgrade();
        save.connect_clicked(move |_| {
            let Some(list) = weak_list.upgrade() else {
                return;
            };
            for label in [&subject_error, &due_error, &feedback] {
                label.set_visible(false);
            }
            let Some(target) = choices
                .borrow()
                .get(selector.selected() as usize)
                .map(|(target, _)| *target)
            else {
                notify(&feedback, "Выберите урок");
                selector.grab_focus();
                return;
            };
            let value = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
            let body_error = if value.trim().is_empty() {
                Some("Напишите домашнее задание".to_string())
            } else {
                length_error(
                    value.trim(),
                    existing.as_ref().map_or("", |task| task.body.trim()),
                    HOMEWORK_LIMIT,
                )
            };
            if let Some(error) = body_error {
                notify(&feedback, &error);
                editor.grab_focus();
                return;
            }

            let result = match (target, id) {
                (Some(target), Some(HomeworkId::Pending(id))) => {
                    let task = existing.as_ref().expect("Открыто существующее задание");
                    list.lessons.save_pending(
                        Some(id),
                        &task.title,
                        task.date,
                        &value,
                        Some(target),
                    )
                }
                (Some(target), _) => list
                    .lessons
                    .set_homework_text(target, &value, id.is_none())
                    .map(|()| HomeworkId::Linked(target)),
                (None, _) => {
                    let title = subject.text();
                    let previous = existing.as_ref().map_or("", |task| task.title.trim());
                    let error = if title.trim().is_empty() {
                        Some("Введите название предмета".to_string())
                    } else {
                        length_error(title.trim(), previous, SUBJECT_LIMIT)
                    };
                    if let Some(error) = error {
                        notify(&subject_error, &error);
                        subject.grab_focus();
                        return;
                    }
                    let date = due.text();
                    let date = if date.trim().is_empty() {
                        None
                    } else {
                        match NaiveDate::parse_from_str(date.trim(), "%d.%m.%Y") {
                            Ok(date) if (1..=9999).contains(&date.year()) => Some(date),
                            _ => {
                                notify(&due_error, "Укажите дату в формате ДД.ММ.ГГГГ");
                                optional_date.set_expanded(true);
                                due.grab_focus();
                                return;
                            }
                        }
                    };
                    let pending = match id {
                        Some(HomeworkId::Pending(id)) => Some(id),
                        _ => None,
                    };
                    list.lessons
                        .save_pending(pending, &title, date, &value, None)
                }
            };
            match result {
                Ok(saved) => {
                    list.render(id.map(|_| RowFocus::Edit(saved)));
                    let attached = matches!(saved, HomeworkId::Linked(_))
                        && (target.is_none() || matches!(id, Some(HomeworkId::Pending(_))));
                    let message = if attached && let HomeworkId::Linked(lesson) = saved {
                        list.lessons.attachment_notice(&[lesson])
                    } else {
                        "Домашнее задание сохранено".to_string()
                    };
                    notify(&list.status, &message);
                    if let Some(window) = weak_window.upgrade() {
                        window.close();
                    }
                }
                Err(error) => notify(&feedback, &error),
            }
        });
        window.present();
        initial_focus.grab_focus();
    }
}

fn lesson_choices(
    lessons: &Lessons,
    existing: Option<&Homework>,
    settings: &Settings,
    today: NaiveDate,
    show_all: bool,
    date: Option<NaiveDate>,
) -> Vec<(Option<RecordId>, String)> {
    let linked = existing.is_some_and(|task| matches!(task.id, HomeworkId::Linked(_)));
    let mut choices: Vec<_> = lessons
        .borrow()
        .iter()
        .filter(|lesson| match existing {
            Some(task) => match task.id {
                HomeworkId::Linked(id) => lesson.id == id,
                HomeworkId::Pending(_) => {
                    lesson.homework.trim().is_empty() && same_subject(&lesson.title, &task.title)
                }
            },
            None => lesson.homework.trim().is_empty(),
        })
        .filter(|lesson| date.is_none_or(|date| lesson.date == date))
        .filter(|lesson| {
            linked
                || show_all
                || !settings.hide_old_lessons
                || today.signed_duration_since(lesson.date).num_days()
                    <= i64::from(settings.old_lesson_days)
        })
        .map(|lesson| {
            let label = if date.is_some() {
                format!("{} · {}", lesson.title, lesson.time.format("%H:%M"))
            } else {
                lesson.label()
            };
            (Some(lesson.id), label)
        })
        .collect();
    if !lessons.is_practice() && !linked {
        if existing.is_some() {
            choices.insert(0, (None, "Урока ещё нет".to_string()));
        } else {
            choices.push((None, "Урока ещё нет".to_string()));
        }
    }
    choices
}

fn initial_lesson_date(
    lessons: &Lessons,
    existing: Option<&Homework>,
    settings: &Settings,
    today: NaiveDate,
) -> NaiveDate {
    if let Some(date) = existing.and_then(|task| task.date) {
        return date;
    }
    let available: HashSet<_> = lesson_choices(lessons, existing, settings, today, false, None)
        .into_iter()
        .filter_map(|(id, _)| id)
        .collect();
    let items = lessons.borrow();
    items
        .iter()
        .find(|lesson| lesson.date >= today && available.contains(&lesson.id))
        .or_else(|| {
            if lessons.is_practice() {
                items
                    .iter()
                    .rev()
                    .find(|lesson| available.contains(&lesson.id))
            } else {
                None
            }
        })
        .map_or(today, |lesson| lesson.date)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lessons::{SchoolData, parse_lesson};

    #[test]
    fn lesson_choices_respect_calendar_boundaries_and_temporary_override() -> Result<(), String> {
        let today = NaiveDate::from_ymd_opt(2026, 11, 9).ok_or("Неверная дата теста")?;
        let lessons = Lessons::practice(|| {});
        for age in [-1, 0, 1, 2, 14, 15, 180, 181] {
            let date = today - chrono::Duration::days(age);
            lessons.store_lesson(
                parse_lesson(
                    "Физика",
                    &date.format("%d.%m.%Y").to_string(),
                    "08:00",
                    "08:40",
                    "23",
                )?,
                None,
            )?;
        }
        let original = lessons.borrow().clone();
        let all_ids: Vec<_> = original.iter().map(|lesson| Some(lesson.id)).collect();
        for (enabled, days, count) in [(false, 1, 8), (true, 1, 3), (true, 14, 5), (true, 180, 7)] {
            let settings = Settings {
                hide_old_lessons: enabled,
                old_lesson_days: days,
                ..Settings::default()
            };
            let selected: Vec<_> = lesson_choices(&lessons, None, &settings, today, false, None)
                .into_iter()
                .map(|(id, _)| id)
                .collect();
            assert_eq!(selected, all_ids[all_ids.len() - count..]);
            let shown: Vec<_> = lesson_choices(&lessons, None, &settings, today, true, None)
                .into_iter()
                .map(|(id, _)| id)
                .collect();
            assert_eq!(shown, all_ids);
        }
        assert_eq!(*lessons.borrow(), original);
        Ok(())
    }

    #[test]
    fn old_linked_homework_stays_selectable_and_pending_choices_keep_subject_rules()
    -> Result<(), String> {
        let today = NaiveDate::from_ymd_opt(2026, 11, 9).ok_or("Неверная дата теста")?;
        let lessons = Lessons::practice(|| {});
        for (title, date) in [
            ("Физика", "01.01.2026"),
            ("ФИЗИКА", "02.01.2026"),
            ("Физика", "09.11.2026"),
            ("Математика", "10.11.2026"),
        ] {
            lessons.store_lesson(parse_lesson(title, date, "08:00", "08:40", "23")?, None)?;
        }
        let old_id = lessons.borrow()[0].id;
        lessons.set_homework_text(old_id, "Старое задание", true)?;
        lessons.set_homework_completed(HomeworkId::Linked(old_id), true)?;
        let pending_id = lessons.save_pending(None, "Физика", None, "Без урока", None)?;
        lessons.set_homework_completed(pending_id, true)?;
        let tasks = lessons.homework();
        let linked = tasks
            .iter()
            .find(|task| task.id == HomeworkId::Linked(old_id))
            .ok_or("Нет связанного ДЗ")?;
        let pending = tasks
            .iter()
            .find(|task| task.id == pending_id)
            .ok_or("Нет отдельного ДЗ")?;
        let original = lessons.borrow().clone();
        let settings = Settings {
            hide_old_lessons: true,
            old_lesson_days: 1,
            ..Settings::default()
        };
        for show_all in [false, true] {
            let choices = lesson_choices(&lessons, Some(linked), &settings, today, show_all, None);
            assert_eq!(choices.len(), 1);
            assert_eq!(choices[0].0, Some(old_id));
        }
        let filtered = lesson_choices(&lessons, Some(pending), &settings, today, false, None);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].0, Some(original[2].id));
        let shown = lesson_choices(&lessons, Some(pending), &settings, today, true, None);
        assert_eq!(shown.len(), 2);
        assert_eq!(shown[0].0, Some(original[1].id));
        assert_eq!(shown[1].0, Some(original[2].id));
        assert_eq!(*lessons.borrow(), original);
        for (before, after) in tasks.iter().zip(lessons.homework()) {
            assert_eq!(before.id, after.id);
            assert_eq!(before.title, after.title);
            assert_eq!(before.date, after.date);
            assert_eq!(before.body, after.body);
            assert_eq!(before.completed, after.completed);
        }
        Ok(())
    }

    #[test]
    fn initial_date_can_reach_a_past_practice_lesson_without_changing_normal_defaults()
    -> Result<(), String> {
        let today = NaiveDate::from_ymd_opt(2026, 11, 10).unwrap();
        let settings = Settings::default();
        let practice = Lessons::practice(|| {});
        practice.store_lesson(
            parse_lesson("Алгебра", "09.11.2026", "08:00", "08:40", "12")?,
            None,
        )?;
        assert_eq!(
            initial_lesson_date(&practice, None, &settings, today),
            today.pred_opt().unwrap()
        );
        let data: SchoolData = serde_json::from_value(
            serde_json::json!({ "lessons": practice.borrow().as_slice(), "pending_homework": [] }),
        )
        .map_err(|error| error.to_string())?;
        let normal = Lessons::new(data);
        assert_eq!(initial_lesson_date(&normal, None, &settings, today), today);
        let task = Homework {
            id: HomeworkId::Pending(99),
            title: "Алгебра".into(),
            date: today.pred_opt(),
            body: "Задание".into(),
            completed: false,
        };
        assert_eq!(
            initial_lesson_date(&normal, Some(&task), &settings, today),
            today.pred_opt().unwrap()
        );
        practice.store_lesson(
            parse_lesson("Алгебра", "11.11.2026", "08:00", "08:40", "12")?,
            None,
        )?;
        assert_eq!(
            initial_lesson_date(&practice, None, &settings, today),
            today.succ_opt().unwrap()
        );
        Ok(())
    }

    #[test]
    fn date_selection_never_includes_lessons_from_other_days() -> Result<(), String> {
        let lessons = Lessons::practice(|| {});
        for date in ["01.01.2026", "10.11.2026", "11.11.2026"] {
            lessons.store_lesson(parse_lesson("Физика", date, "08:00", "08:40", "23")?, None)?;
        }
        let today = NaiveDate::from_ymd_opt(2026, 11, 10).unwrap();
        let settings = Settings {
            hide_old_lessons: true,
            old_lesson_days: 1,
            ..Settings::default()
        };
        for (day, override_filter, count) in [
            (today, false, 1),
            (NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(), false, 0),
            (NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(), true, 1),
        ] {
            let choices =
                lesson_choices(&lessons, None, &settings, today, override_filter, Some(day));
            assert_eq!(choices.len(), count);
            for (id, _) in choices {
                assert!(
                    lessons
                        .borrow()
                        .iter()
                        .any(|lesson| Some(lesson.id) == id && lesson.date == day)
                );
            }
        }
        Ok(())
    }

    #[test]
    fn all_hidden_lessons_still_allow_standalone_homework() -> Result<(), serde_json::Error> {
        let data: SchoolData = serde_json::from_str(
            r#"{"lessons":[{"title":"Физика","date":"2026-01-01","time":"08:00:00","end":"08:40:00","room":"23"}],"pending_homework":[]}"#,
        )?;
        let lessons = Lessons::new(data);
        let settings = Settings {
            hide_old_lessons: true,
            old_lesson_days: 14,
            ..Settings::default()
        };
        let today = lessons.borrow()[0].date + chrono::Duration::days(15);
        let hidden = lesson_choices(&lessons, None, &settings, today, false, None);
        assert_eq!(hidden, vec![(None, "Урока ещё нет".to_string())]);
        let shown = lesson_choices(&lessons, None, &settings, today, true, None);
        assert_eq!(shown.len(), 2);
        assert_eq!(shown[0].0, Some(lessons.borrow()[0].id));
        assert_eq!(shown[1].0, None);
        Ok(())
    }
}
