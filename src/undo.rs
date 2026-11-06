use adw::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

use crate::lessons::{Lessons, Removed};
use crate::{notify, text};

const WINDOW: Duration = Duration::from_secs(8);

#[derive(Clone)]
pub struct UndoBar(Rc<UndoView>);

#[derive(Clone)]
pub struct UndoHandle(Weak<UndoView>);

impl UndoHandle {
    pub fn offer(&self, lessons: &Lessons, removed: Removed) {
        if let Some(view) = self.0.upgrade() {
            UndoBar(view).offer(lessons, removed);
        }
    }
}

struct UndoView {
    row: gtk::Box,
    message: gtk::Label,
    status: gtk::glib::WeakRef<gtk::Label>,
    entries: RefCell<Vec<Entry>>,
    next_id: Cell<u64>,
}

struct Entry {
    id: u64,
    expires: Instant,
    lessons: Lessons,
    removed: Removed,
}

impl Entry {
    fn message(&self) -> &'static str {
        if self.removed.lesson_slot().is_some() {
            "Урок удалён"
        } else {
            "Домашнее задание удалено"
        }
    }

    fn restored_message(&self) -> &'static str {
        if self.removed.lesson_slot().is_some() {
            "Урок восстановлен"
        } else {
            "Домашнее задание восстановлено"
        }
    }
}

impl UndoBar {
    pub fn new(status: &gtk::Label) -> Self {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        row.add_css_class("undo-bar");
        row.set_visible(false);
        let message = text("", "section");
        message.set_hexpand(true);
        let button = gtk::Button::with_label("Вернуть");
        button.set_tooltip_text(Some("Вернуть (Ctrl+Z)"));
        button.set_valign(gtk::Align::Center);
        row.append(&message);
        row.append(&button);
        let inner = Rc::new(UndoView {
            row,
            message,
            status: status.downgrade(),
            entries: RefCell::new(Vec::new()),
            next_id: Cell::new(0),
        });
        let weak = Rc::downgrade(&inner);
        button.connect_clicked(move |_| {
            if let Some(inner) = weak.upgrade() {
                inner.restore();
            }
        });
        Self(inner)
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.0.row
    }

    pub fn handle(&self) -> UndoHandle {
        UndoHandle(Rc::downgrade(&self.0))
    }

    fn offer(&self, lessons: &Lessons, removed: Removed) {
        let id = self.0.next_id.get();
        self.0.next_id.set(id + 1);
        self.0.entries.borrow_mut().push(Entry {
            id,
            expires: Instant::now() + WINDOW,
            lessons: lessons.clone(),
            removed,
        });
        if let Some(status) = self.0.status.upgrade() {
            status.set_visible(false);
        }
        self.0.refresh();
        let weak = Rc::downgrade(&self.0);
        gtk::glib::timeout_add_local_once(WINDOW, move || {
            if let Some(inner) = weak.upgrade() {
                let mut entries = inner.entries.borrow_mut();
                let was_current = entries.last().is_some_and(|entry| entry.id == id);
                entries.retain(|entry| entry.id != id);
                drop(entries);
                if was_current {
                    inner.refresh();
                }
            }
        });
    }

    pub fn restore(&self) -> bool {
        self.0.restore()
    }
}

impl UndoView {
    fn refresh(&self) {
        let now = Instant::now();
        let mut entries = self.entries.borrow_mut();
        entries.retain(|entry| entry.expires > now);
        if let Some(entry) = entries.last() {
            self.message.set_text(entry.message());
            self.row.set_visible(true);
        } else {
            self.row.set_visible(false);
        }
    }

    fn restore(&self) -> bool {
        let now = Instant::now();
        let entry = {
            let mut entries = self.entries.borrow_mut();
            entries.retain(|entry| entry.expires > now);
            entries.pop()
        };
        let Some(entry) = entry else {
            self.refresh();
            return false;
        };
        match entry.lessons.restore(&entry.removed) {
            Ok(()) => {
                if let Some(status) = self.status.upgrade() {
                    notify(&status, entry.restored_message());
                }
                self.refresh();
            }
            Err(error) => {
                self.entries.borrow_mut().push(entry);
                self.message
                    .set_text(&format!("Не удалось вернуть: {error}"));
            }
        }
        true
    }
}
