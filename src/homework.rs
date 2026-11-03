use adw::prelude::*;
use std::rc::Rc;

use crate::lessons::{Lessons, set_homework_completed};
use crate::{column, notify, page, text};

pub fn homework_page(status: &gtk::Label, lessons: Lessons) -> gtk::Box {
    let page = page("Домашние задания");
    let list = column(0);
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
        list.append(&text("Добавь задание в форме урока", "muted"));
        return;
    }

    for (index, lesson) in tasks {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        row.add_css_class("panel");

        let checkbox = gtk::CheckButton::new();
        checkbox.set_valign(gtk::Align::Start);
        checkbox.set_active(lesson.homework_completed);

        let details = column(6);
        details.set_hexpand(true);
        details.append(&text(
            &format!("{} · {}", lesson.title, lesson.date.format("%d.%m.%Y"),),
            "section",
        ));
        details.append(&text(&lesson.homework, "muted"));

        let items = Rc::clone(lessons);
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

            let result = set_homework_completed(&mut items.borrow_mut(), index, active);

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
        list.append(&row);
    }
}
