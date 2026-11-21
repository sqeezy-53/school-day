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
            view.offer(lessons, UndoAction::Single(removed));
        }
    }

    pub fn offer_batch(&self, lessons: &Lessons, refresh: impl Fn() + 'static) {
        if let Some(view) = self.0.upgrade() {
            view.offer(lessons, UndoAction::Batch(Box::new(refresh)));
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
    generation: u64,
    revision: u64,
    lessons: Lessons,
    action: UndoAction,
}

enum UndoAction {
    Single(Removed),
    Batch(Box<dyn Fn()>),
}

impl Entry {
    fn message(&self) -> &'static str {
        match &self.action {
            UndoAction::Single(removed) if removed.lesson_slot().is_some() => "Урок удалён",
            UndoAction::Single(_) => "Домашнее задание удалено",
            UndoAction::Batch(_) => "Выполненные задания удалены",
        }
    }

    fn restored_message(&self) -> &'static str {
        match &self.action {
            UndoAction::Single(removed) if removed.lesson_slot().is_some() => "Урок восстановлен",
            UndoAction::Single(_) => "Домашнее задание восстановлено",
            UndoAction::Batch(_) => "Выполненные задания восстановлены",
        }
    }

    fn active(&self, now: Instant) -> bool {
        self.expires > now
            && self.generation == self.lessons.generation()
            && (!matches!(self.action, UndoAction::Batch(_))
                || self.revision == self.lessons.revision())
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

    pub fn clear(&self) {
        self.0.entries.borrow_mut().clear();
        self.0.refresh();
    }
}

impl UndoView {
    fn offer(self: &Rc<Self>, lessons: &Lessons, action: UndoAction) {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        self.entries.borrow_mut().push(Entry {
            id,
            expires: Instant::now() + WINDOW,
            generation: lessons.generation(),
            revision: lessons.revision(),
            lessons: lessons.clone(),
            action,
        });
        if let Some(status) = self.status.upgrade() {
            status.set_visible(false);
        }
        self.refresh();
        let weak = Rc::downgrade(self);
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

    fn refresh(&self) {
        let now = Instant::now();
        let mut entries = self.entries.borrow_mut();
        entries.retain(|entry| entry.active(now));
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
            entries.retain(|entry| entry.active(now));
            entries.pop()
        };
        let Some(entry) = entry else {
            self.refresh();
            return false;
        };
        let result = if entry.revision == entry.lessons.revision() {
            entry.lessons.travel(false).and_then(|changed| {
                if changed {
                    match &entry.action {
                        UndoAction::Single(removed) => entry.lessons.notify_restored(removed),
                        UndoAction::Batch(refresh) => refresh(),
                    }
                    Ok(())
                } else {
                    Err("История очищена".into())
                }
            })
        } else if let UndoAction::Single(removed) = &entry.action {
            entry.lessons.restore(removed)
        } else {
            Err("После очистки появились новые изменения".into())
        };
        match result {
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
