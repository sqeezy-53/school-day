use adw::prelude::*;
use chrono::{Datelike, NaiveDate};
use std::rc::Rc;

use crate::forms::{self, HOMEWORK_LIMIT, SUBJECT_LIMIT, length_error};
use crate::lessons::{Homework, HomeworkId, Lesson, Lessons, same_subject};
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
                "Заданий пока нет. Нажмите «Добавить ДЗ»."
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
        if matches!(id, HomeworkId::Pending(_)) {
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
        });
        if id.is_some() && existing.is_none() {
            notify(&self.status, "Задание уже удалено");
            return;
        }
        let mut choices: Vec<_> = self
            .lessons
            .borrow()
            .iter()
            .filter(|lesson| match &existing {
                Some(task) => match task.id {
                    HomeworkId::Linked(id) => lesson.id == id,
                    HomeworkId::Pending(_) => {
                        lesson.homework.trim().is_empty()
                            && same_subject(&lesson.title, &task.title)
                    }
                },
                None => lesson.homework.trim().is_empty(),
            })
            .map(|lesson| (Some(lesson.id), lesson_label(lesson)))
            .collect();

        if !self.lessons.is_practice() && !matches!(id, Some(HomeworkId::Linked(_))) {
            if id.is_some() {
                choices.insert(0, (None, "Оставить без урока".to_string()));
            } else {
                choices.push((None, "Урока ещё нет".to_string()));
            }
        }
        if choices.is_empty() {
            notify(&self.status, "Сначала добавьте урок в расписании.");
            return;
        }
        let labels: Vec<_> = choices.iter().map(|(_, label)| label.as_str()).collect();
        let selector = gtk::DropDown::from_strings(&labels);
        selector.set_enable_search(choices.len() > 5);
        selector.set_sensitive(choices.len() > 1);

        let heading = if id.is_some() {
            "Изменить ДЗ"
        } else {
            "Добавить ДЗ"
        };
        let root = column(0);
        let header = adw::HeaderBar::new();
        header.set_title_widget(Some(&text(heading, "section")));
        root.append(&header);

        let form = column(16);
        margins(&form, 24);
        if self.lessons.is_practice() {
            form.append(&text(
                "Учебное ДЗ исчезнет после выхода из обучения.",
                "muted",
            ));
        }
        let selector_label = text("К какому уроку", "section");
        selector_label.set_mnemonic_widget(Some(&selector));
        form.append(&selector_label);
        form.append(&selector);

        let standalone = column(12);
        let subject = field(&standalone, "Предмет", "Например, алгебра");
        let subject_error = forms::error_label(&standalone, &subject);
        let due = field(&standalone, "Дата (необязательно)", "ДД.ММ.ГГГГ");
        let due_error = forms::error_label(&standalone, &due);
        standalone.append(&text(
            "С датой задание сможет привязаться к уроку автоматически.",
            "muted",
        ));
        standalone.set_visible(choices[0].0.is_none());
        let has_no_lesson: Vec<_> = choices.iter().map(|(id, _)| id.is_none()).collect();
        let standalone_section = standalone.clone();
        selector.connect_selected_notify(move |selector| {
            standalone_section.set_visible(
                has_no_lesson
                    .get(selector.selected() as usize)
                    .copied()
                    .unwrap_or(false),
            );
        });
        form.append(&standalone);

        let (scroll, editor) = forms::homework_editor(140);
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
        form.append(&editor_label);
        form.append(&scroll);
        let feedback = forms::error_label(&form, &scroll);

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
            .default_height(490)
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
            let Some((target, _)) = choices.get(selector.selected() as usize) else {
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

            let result = match (*target, id) {
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
                    notify(
                        &list.status,
                        if attached {
                            "Задание прикреплено к уроку"
                        } else {
                            "Домашнее задание сохранено"
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
        initial_focus.grab_focus();
    }
}

fn lesson_label(lesson: &Lesson) -> String {
    format!(
        "{} · {} в {}",
        lesson.title,
        lesson.date.format("%d.%m.%Y"),
        lesson.time.format("%H:%M")
    )
}
