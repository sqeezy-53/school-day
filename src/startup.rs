use adw::prelude::*;
use gtk::gdk::prelude::GdkCairoContextExt;
use gtk::{gio, glib};

pub const WORKSPACE: &str = "workspace";

pub fn show(stack: &gtk::Stack) -> bool {
    if !animations_enabled(stack) {
        return false;
    }
    let svg = glib::Bytes::from_static(include_bytes!(
        "../packaging/icons/io.github.sqeezy_53.school-day.svg"
    ));
    let stream = gio::MemoryInputStream::from_bytes(&svg);
    let Ok(pixbuf) = gtk::gdk_pixbuf::Pixbuf::from_stream_at_scale(
        &stream,
        240,
        240,
        true,
        None::<&gio::Cancellable>,
    ) else {
        return false;
    };
    let logo = gtk::DrawingArea::new();
    logo.set_content_width(120);
    logo.set_content_height(120);
    logo.set_halign(gtk::Align::Center);
    logo.set_opacity(0.0);
    logo.set_draw_func(move |logo, cr, width, height| {
        let size = f64::from(width.min(height)) * (0.90 + 0.10 * logo.opacity());
        cr.translate(
            (f64::from(width) - size) / 2.0,
            (f64::from(height) - size) / 2.0,
        );
        let scale = size / f64::from(pixbuf.width());
        cr.scale(scale, scale);
        cr.set_source_pixbuf(&pixbuf, 0.0, 0.0);
        let _ = cr.paint();
    });
    let title = gtk::Label::new(Some("Школьный день"));
    title.add_css_class("startup-title");
    title.set_opacity(0.0);

    let composition = crate::column(14);
    composition.set_halign(gtk::Align::Center);
    composition.set_valign(gtk::Align::Center);
    let spacer = crate::column(0);
    let balance = gtk::SizeGroup::new(gtk::SizeGroupMode::Vertical);
    balance.add_widget(&spacer);
    balance.add_widget(&title);
    composition.append(&spacer);
    composition.append(&logo);
    composition.append(&title);
    let scene = gtk::CenterBox::builder()
        .center_widget(&composition)
        .build();
    scene.add_css_class("startup-scene");
    stack.add_named(&scene, Some("startup"));

    let stack = stack.downgrade();
    let first = std::cell::Cell::new(None);
    let exit = std::cell::Cell::new(None);
    scene.add_tick_callback(move |scene, clock| {
        let Some(stack) = stack.upgrade() else {
            return glib::ControlFlow::Break;
        };
        let now = clock.frame_time();
        let start = first.get().unwrap_or_else(|| {
            first.set(Some(now));
            now
        });
        let elapsed = (now - start) as f64 / 1000.0;
        let workspace = stack.child_by_name(WORKSPACE);
        if !animations_enabled(&stack) && workspace.is_some() {
            stack.set_visible_child_name(WORKSPACE);
            stack.remove(scene);
            return glib::ControlFlow::Break;
        }
        if title.opacity() < 1.0 {
            let book = phase(elapsed, 0.0, 300.0);
            if logo.opacity() != book {
                logo.set_opacity(book);
                logo.queue_draw();
            }
            title.set_opacity(phase(elapsed, 250.0, 300.0));
        }
        if elapsed >= 800.0
            && let Some(workspace) = workspace
        {
            let start = exit.get().unwrap_or_else(|| {
                exit.set(Some(now));
                now
            });
            let fade = ((now - start) as f64 / 150_000.0).clamp(0.0, 1.0);
            if fade >= 1.0 {
                workspace.set_opacity(0.12);
                stack.set_visible_child(&workspace);
                stack.remove(scene);
                workspace.add_tick_callback(move |workspace, clock| {
                    let elapsed = (clock.frame_time() - now) as f64 / 1000.0;
                    let fade = if animations_enabled(workspace) {
                        phase(elapsed, 0.0, 150.0)
                    } else {
                        1.0
                    };
                    workspace.set_opacity(0.12 + 0.88 * fade);
                    if fade >= 1.0 {
                        glib::ControlFlow::Break
                    } else {
                        glib::ControlFlow::Continue
                    }
                });
                return glib::ControlFlow::Break;
            }
            composition.set_opacity(1.0 - fade);
        }
        glib::ControlFlow::Continue
    });
    true
}

fn phase(elapsed: f64, start: f64, duration: f64) -> f64 {
    let progress = ((elapsed - start) / duration).clamp(0.0, 1.0);
    1.0 - (1.0 - progress).powi(2)
}

fn animations_enabled(widget: &impl IsA<gtk::Widget>) -> bool {
    let settings = widget.settings();
    let property = "gtk-interface-reduced-motion";
    settings.is_gtk_enable_animations()
        && !(settings.find_property(property).is_some()
            && glib::EnumValue::from_value(&settings.property_value(property))
                .is_some_and(|(_, value)| value.nick() == "reduce"))
}
