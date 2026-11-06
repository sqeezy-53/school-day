use adw::prelude::*;
use gtk::gdk::{Key, ModifierType};
use gtk::glib::Propagation;

pub const SUBJECT_LIMIT: usize = 64;
pub const ROOM_LIMIT: usize = 12;
pub const CLASS_LIMIT: usize = 12;
pub const HOMEWORK_LIMIT: usize = 2000;

pub fn length_error(value: &str, previous: &str, limit: usize) -> Option<String> {
    if value == previous {
        return None;
    }
    let count = value.chars().count();
    (count > limit).then(|| format!("Максимум {limit} символов. Сейчас {count}."))
}

pub fn error_label(form: &gtk::Box, widget: &impl IsA<gtk::Widget>) -> gtk::Label {
    let label = crate::text("", "form-error");
    label.set_visible(false);
    form.insert_child_after(&label, Some(widget));
    label
}

pub fn homework_editor(height: i32) -> (gtk::ScrolledWindow, gtk::TextView) {
    let editor = gtk::TextView::new();
    editor.set_wrap_mode(gtk::WrapMode::WordChar);
    editor.set_accepts_tab(false);
    editor.set_top_margin(10);
    editor.set_bottom_margin(10);
    editor.set_left_margin(10);
    editor.set_right_margin(10);
    let scroll = gtk::ScrolledWindow::new();
    scroll.add_css_class("homework-editor");
    scroll.set_min_content_height(height);
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_child(Some(&editor));
    (scroll, editor)
}

pub fn dialog_keys(window: &adw::Window, cancel: &gtk::Button, save: Option<&gtk::Button>) {
    let escape = gtk::EventControllerKey::new();
    let cancel = cancel.downgrade();
    escape.connect_key_pressed(move |_, key, _, _| {
        if key == Key::Escape
            && let Some(cancel) = cancel.upgrade()
        {
            cancel.emit_clicked();
            return Propagation::Stop;
        }
        Propagation::Proceed
    });
    window.add_controller(escape);

    if let Some(save) = save {
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let save = save.downgrade();
        keys.connect_key_pressed(move |_, key, _, modifiers| {
            if matches!(key, Key::Return | Key::KP_Enter)
                && modifiers.contains(ModifierType::CONTROL_MASK)
                && !modifiers.intersects(
                    ModifierType::SHIFT_MASK | ModifierType::ALT_MASK | ModifierType::SUPER_MASK,
                )
                && let Some(save) = save.upgrade().filter(|save| save.is_sensitive())
            {
                save.emit_clicked();
                return Propagation::Stop;
            }
            Propagation::Proceed
        });
        window.add_controller(keys);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_count_unicode_and_allow_unchanged_legacy_values() {
        let limit = ROOM_LIMIT;
        let exact = "я".repeat(limit);
        let legacy = format!("{exact}я");
        assert!(length_error(&exact, "", limit).is_none());
        assert!(length_error(&legacy, "", limit).is_some());
        assert!(length_error(&legacy, &legacy, limit).is_none());
    }
}
