use adw::prelude::*;
use gtk::gdk::{Key, ModifierType};
use gtk::glib::Propagation;

pub use school_day_core::forms::*;

pub fn error_label(form: &gtk::Box, widget: &impl IsA<gtk::Widget>) -> gtk::Label {
    let label = crate::text("", "form-error");
    label.set_visible(false);
    form.insert_child_after(&label, Some(widget));
    label
}

pub fn time_input(maximum: f64, value: u32, label: &str) -> gtk::SpinButton {
    let spin = gtk::SpinButton::with_range(0.0, maximum, 1.0);
    crate::forms::guard_wheel(&spin);
    spin.set_numeric(true);
    spin.set_wrap(true);
    spin.set_width_chars(2);
    spin.set_tooltip_text(Some(label));
    spin.set_update_policy(gtk::SpinButtonUpdatePolicy::IfValid);
    spin.set_value(value as f64);
    spin.connect_output(|spin| {
        spin.set_text(&format!("{:02}", spin.value_as_int()));
        gtk::glib::Propagation::Stop
    });
    spin.set_text(&format!("{value:02}"));
    spin
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
    crate::seasonal::inherit_palette(window);
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

type ScrollController = (
    gtk::glib::WeakRef<gtk::EventControllerScroll>,
    gtk::PropagationPhase,
);
thread_local! {
    static WHEEL: std::cell::RefCell<Vec<ScrollController>> = const { std::cell::RefCell::new(Vec::new()) };
}

pub fn set_wheel(allow: bool) {
    WHEEL.with_borrow_mut(|items| {
        items.retain(|(weak, phase)| {
            if let Some(controller) = weak.upgrade() {
                controller.set_propagation_phase(if allow {
                    *phase
                } else {
                    gtk::PropagationPhase::None
                });
                true
            } else {
                false
            }
        })
    });
}

pub fn guard_wheel(widget: &impl IsA<gtk::Widget>) {
    for control in widget
        .observe_controllers()
        .iter::<gtk::glib::Object>()
        .flatten()
    {
        if let Ok(scroll) = control.downcast::<gtk::EventControllerScroll>() {
            let phase = scroll.propagation_phase();
            WHEEL.with_borrow_mut(|items| {
                items.retain(|(w, _)| w.upgrade().is_some());
                items.push((scroll.downgrade(), phase));
            });
            if !crate::settings::display().wheel {
                scroll.set_propagation_phase(gtk::PropagationPhase::None);
            }
        }
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
