use crate::lessons::{HomeworkId, Lessons};
use adw::prelude::*;

pub fn button(items: &Lessons, schedule: &gtk::Box, homework: &gtk::Box) -> gtk::MenuButton {
    let button = gtk::MenuButton::builder()
        .icon_name("system-search-symbolic")
        .tooltip_text("Найти урок, заметку или ДЗ")
        .build();
    let pop = gtk::Popover::new();
    let content = crate::column(6);
    let entry = gtk::SearchEntry::builder()
        .placeholder_text("Предмет, задание, заметка или дата")
        .build();
    let results = crate::column(4);
    content.append(&entry);
    let scroll = gtk::ScrolledWindow::builder()
        .min_content_width(230)
        .max_content_height(310)
        .propagate_natural_height(true)
        .child(&results)
        .build();
    content.append(&scroll);
    pop.set_child(Some(&content));
    button.set_popover(Some(&pop));
    let (items, schedule, homework, popover) = (
        items.clone(),
        schedule.downgrade(),
        homework.downgrade(),
        pop.downgrade(),
    );
    entry.connect_search_changed(move |entry| {
        while let Some(row) = results.first_child() {
            results.remove(&row);
        }
        let q = entry.text().trim().to_lowercase();
        if q.is_empty() {
            return;
        }
        let mut count = 0;
        let mut add = |label: String,
                       target: &gtk::glib::WeakRef<gtk::Box>,
                       action: &'static str,
                       id: String| {
            let row = gtk::Button::with_label(&label);
            row.add_css_class("flat");
            if let Some(label) = row.child().and_downcast::<gtk::Label>() {
                label.set_wrap(true);
                label.set_lines(2);
                label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            }
            let (target, pop) = (target.clone(), popover.clone());
            row.connect_clicked(move |_| {
                if let Some(pop) = pop.upgrade() {
                    pop.popdown();
                }
                if let Some(target) = target.upgrade() {
                    if let Some(stack) = target.parent().and_downcast::<gtk::Stack>() {
                        stack.set_visible_child(&target);
                    }
                    let _ = target.activate_action(action, Some(&id.to_variant()));
                }
            });
            results.append(&row);
            count += 1;
        };
        for lesson in items
            .borrow()
            .iter()
            .filter(|l| {
                [&l.title, &l.options.replacement, &l.note]
                    .iter()
                    .any(|s| s.to_lowercase().contains(&q))
                    || l.date.format("%d.%m.%Y").to_string().contains(&q)
            })
            .take(12)
        {
            add(
                lesson.label(),
                &schedule,
                "records.edit",
                lesson.id.to_string(),
            );
        }
        for task in items
            .homework()
            .into_iter()
            .filter(|h| h.title.to_lowercase().contains(&q) || h.body.to_lowercase().contains(&q))
            .take(12)
        {
            let id = match task.id {
                HomeworkId::Linked(id) => format!("l{id}"),
                HomeworkId::Pending(id) => format!("p{id}"),
            };
            add(
                format!("ДЗ · {} · {}", task.title, task.body),
                &homework,
                "records.edit",
                id,
            );
        }
        if count == 0 {
            results.append(&crate::text("Совпадений нет", "muted"));
        }
    });
    pop.connect_show(move |_| {
        entry.set_text("");
        entry.grab_focus();
    });
    button
}
pub fn actions(widget: &gtk::Box, edit: impl Fn(&str) + 'static, refresh: impl Fn() + 'static) {
    let group = gtk::gio::SimpleActionGroup::new();
    let action = gtk::gio::SimpleAction::new("edit", Some(gtk::glib::VariantTy::STRING));
    action.connect_activate(move |_, v| {
        if let Some(s) = v.and_then(|v| v.str()) {
            edit(s);
        }
    });
    group.add_action(&action);
    let action = gtk::gio::SimpleAction::new("refresh", None);
    action.connect_activate(move |_, _| refresh());
    group.add_action(&action);
    widget.insert_action_group("records", Some(&group));
}
