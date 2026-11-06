use adw::prelude::*;

use crate::lessons::Lessons;
use crate::tutorial::Anchor;
use crate::{column, margins, notify, page, primary, text};

pub fn homework_page(status: &gtk::Label, lessons: Lessons) -> gtk::Box {
    let page = page("Домашние задания");
    let list = column(0);
    let add = primary("Добавить ДЗ");
    Anchor::AddHomework.mark(&add);

    let items_for_add = lessons.clone();
    let status_for_add = status.clone();
    let list_for_add = list.downgrade();

    add.connect_clicked(move |_| {
        if let Some(list) = list_for_add.upgrade() {
            on_edit_homework(&status_for_add, &items_for_add, &list, None);
        }
    });

    page.append(&add);

    page.append(&list);

    render_homework(&list, &lessons, status);

    let weak_list = list.downgrade();
    let status = status.clone();

    page.connect_map(move |_| {
        if let Some(list) = weak_list.upgrade() {
            render_homework(&list, &lessons, &status);
        }
    });

    page
}

fn render_homework(list: &gtk::Box, lessons: &Lessons, status: &gtk::Label) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }

    let tasks: Vec<_> = lessons
        .borrow()
        .iter()
        .enumerate()
        .filter(|(_, lesson)| !lesson.homework.trim().is_empty())
        .map(|(index, lesson)| (index, lesson.clone()))
        .collect();

    if tasks.is_empty() {
        list.append(&text("Заданий пока нет. Нажмите «Добавить ДЗ».", "muted"));
        return;
    }

    for (index, lesson) in tasks {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        row.add_css_class("panel");

        let checkbox = gtk::CheckButton::new();
        Anchor::CompleteHomework.mark(&checkbox);
        checkbox.set_valign(gtk::Align::Start);
        checkbox.set_active(lesson.homework_completed);

        let details = column(6);
        details.set_hexpand(true);
        details.append(&text(
            &format!("{} · {}", lesson.title, lesson.date.format("%d.%m.%Y"),),
            "section",
        ));
        details.append(&text(&lesson.homework, "muted"));

        let edit = gtk::Button::from_icon_name("document-edit-symbolic");
        edit.add_css_class("flat");
        edit.set_tooltip_text(Some("Изменить ДЗ"));
        edit.set_valign(gtk::Align::Start);

        let items_for_edit = lessons.clone();
        let status_for_edit = status.clone();
        let list_for_edit = list.downgrade();

        edit.connect_clicked(move |_| {
            if let Some(list) = list_for_edit.upgrade() {
                on_edit_homework(&status_for_edit, &items_for_edit, &list, Some(index));
            }
        });

        let delete = gtk::Button::from_icon_name("user-trash-symbolic");
        delete.add_css_class("flat");
        delete.set_tooltip_text(Some("Удалить ДЗ"));
        delete.set_valign(gtk::Align::Start);
        let items_for_delete = lessons.clone();
        let status_for_delete = status.clone();
        let list_for_delete = list.downgrade();
        delete.connect_clicked(move |_| {
            let items = items_for_delete.clone();
            let feedback = status_for_delete.clone();
            let list = list_for_delete.clone();
            crate::confirm_delete(
                &status_for_delete,
                "Удалить домашнее задание?",
                "Сам урок останется в расписании.",
                move || {
                    let result = items.delete_homework(index);
                    match result {
                        Ok(()) => {
                            if let Some(list) = list.upgrade() {
                                render_homework(&list, &items, &feedback);
                            }
                            notify(&feedback, "Домашнее задание удалено");
                        }
                        Err(error) => notify(&feedback, &format!("Не удалось удалить ДЗ: {error}")),
                    }
                },
            );
        });

        let items = lessons.clone();
        let status = status.clone();

        checkbox.connect_toggled(move |checkbox| {
            let Some(previous) = items
                .borrow()
                .get(index)
                .map(|lesson| lesson.homework_completed)
            else {
                notify(&status, "Урок не найден. Откройте вкладку заново.");
                return;
            };

            let active = checkbox.is_active();

            if previous == active {
                return;
            }

            let result = items.set_homework_completed(index, active);

            match result {
                Ok(()) => notify(&status, "Сохранено"),
                Err(error) => {
                    checkbox.set_active(previous);
                    notify(&status, &format!("Не удалось сохранить: {error}"));
                }
            }
        });

        row.append(&checkbox);
        row.append(&details);
        row.append(&edit);
        row.append(&delete);
        list.append(&row);
    }
}

fn on_edit_homework(status: &gtk::Label, lessons: &Lessons, list: &gtk::Box, index: Option<usize>) {
    let Some(parent) = status
        .root()
        .and_then(|root| root.downcast::<gtk::Window>().ok())
    else {
        return;
    };

    let choices: Vec<_> = lessons
        .borrow()
        .iter()
        .enumerate()
        .filter(|(lesson_index, lesson)| match index {
            Some(wanted) => *lesson_index == wanted,
            None => lesson.homework.trim().is_empty(),
        })
        .map(|(index, lesson)| (index, lesson.clone()))
        .collect();

    if choices.is_empty() {
        let message = if index.is_some() {
            "Урок не найден. Откройте вкладку заново."
        } else if lessons.borrow().is_empty() {
            "Сначала добавьте урок в расписании."
        } else {
            "У всех уроков уже есть ДЗ. Измените задание кнопкой с карандашом."
        };
        notify(status, message);
        return;
    }

    let labels: Vec<String> = choices
        .iter()
        .map(|(_, lesson)| {
            format!(
                "{} · {} в {}",
                lesson.title,
                lesson.date.format("%d.%m.%Y"),
                lesson.time.format("%H:%M"),
            )
        })
        .collect();
    let strings: Vec<&str> = labels.iter().map(String::as_str).collect();
    let selector = gtk::DropDown::from_strings(&strings);
    selector.set_enable_search(true);
    selector.set_sensitive(choices.len() > 1);

    let heading = if index.is_some() {
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
    if lessons.is_practice() {
        form.append(&text(
            "Учебное ДЗ исчезнет после выхода из обучения.",
            "muted",
        ));
    }
    form.append(&text("К какому уроку", "section"));
    form.append(&selector);
    form.append(&text("Задание", "section"));

    let editor = gtk::TextView::new();
    editor.set_wrap_mode(gtk::WrapMode::WordChar);
    editor.set_top_margin(10);
    editor.set_bottom_margin(10);
    editor.set_left_margin(10);
    editor.set_right_margin(10);
    editor.buffer().set_text(&choices[0].1.homework);

    let scroll = gtk::ScrolledWindow::new();
    scroll.set_min_content_height(140);
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_child(Some(&editor));
    form.append(&scroll);

    let feedback = text("", "muted");
    feedback.set_visible(false);
    form.append(&feedback);
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    actions.set_halign(gtk::Align::End);
    let cancel = gtk::Button::with_label("Отмена");
    let save = primary("Сохранить");
    actions.append(&cancel);
    actions.append(&save);
    form.append(&actions);
    root.append(&form);

    let window = adw::Window::builder()
        .title(heading)
        .default_width(520)
        .default_height(440)
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

    let items = lessons.clone();
    let status = status.clone();
    let weak_list = list.downgrade();
    let weak_window = window.downgrade();
    let buffer = editor.buffer();
    save.connect_clicked(move |_| {
        // Позиция в выпадающем списке соответствует паре с исходным индексом урока.
        let Some((index, _)) = choices.get(selector.selected() as usize) else {
            notify(&feedback, "Выберите урок");
            return;
        };
        let value = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
        let result = items.set_homework_text(*index, &value);
        match result {
            Ok(()) => {
                if let Some(list) = weak_list.upgrade() {
                    render_homework(&list, &items, &status);
                }
                notify(&status, "Домашнее задание сохранено");
                if let Some(window) = weak_window.upgrade() {
                    window.close();
                }
            }
            Err(error) => notify(&feedback, &error),
        }
    });

    window.present();
    editor.grab_focus();
}
