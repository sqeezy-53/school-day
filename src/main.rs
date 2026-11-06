#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod bells;
mod homework;
mod lessons;
mod schedule;
mod settings;
mod storage;
mod tutorial;
#[cfg(target_os = "windows")]
mod windows;

use adw::prelude::*;
use schedule::schedule_page;

const APP_ID: &str = "io.github.sqeezy_53.school-day";

// оформление
const CSS: &str = r#"
window { background: @window_bg_color; color: @window_fg_color; }
headerbar { background: @headerbar_bg_color; box-shadow: none; border-bottom: 1px solid alpha(@window_fg_color, 0.10); min-height: 48px; }
.brand { font-size: 12px; font-weight: 600; opacity: 0.65; }
.brand-logo { color: @headerbar_fg_color; background: alpha(@headerbar_fg_color, 0.12); border-radius: 5px; }
.title { font-size: 23px; font-weight: 600; letter-spacing: -0.4px; }
.muted { opacity: 0.60; }
.section { font-size: 13px; font-weight: 500; }
.nav { padding: 7px 12px; border-radius: 6px; color: alpha(@window_fg_color, 0.65); }
.nav:checked { background: alpha(@window_fg_color, 0.10); color: @window_fg_color; }
.nav:hover { background: alpha(@window_fg_color, 0.07); }
.panel { padding: 20px 0; border-top: 1px solid alpha(@window_fg_color, 0.10); }
.empty-icon { opacity: 0.55; }
button { box-shadow: none; }
button.primary { background: alpha(@window_fg_color, 0.10); color: @window_fg_color; border: 1px solid alpha(@window_fg_color, 0.18); padding: 9px 16px; border-radius: 6px; }
button.primary:hover { background: alpha(@window_fg_color, 0.15); }
entry { background: @view_bg_color; color: @view_fg_color; border: 1px solid alpha(@window_fg_color, 0.15); padding: 8px 10px; border-radius: 6px; }
entry:focus-within { border-color: alpha(@window_fg_color, 0.55); }
.status { opacity: 0.80; padding-top: 12px; border-top: 1px solid alpha(@window_fg_color, 0.10); }
.settings-section { padding-top: 20px; border-top: 1px solid alpha(@window_fg_color, 0.10); }
.settings-title { font-size: 16px; font-weight: 600; }
.bell-table { padding: 12px; background: @card_bg_color; border: 1px solid alpha(@window_fg_color, 0.10); border-radius: 10px; }
"#;

fn main() -> gtk::glib::ExitCode {
    #[cfg(target_os = "windows")]
    let _running = match windows::running_guard() {
        Ok(guard) => guard,
        Err(error) => {
            windows::show_error(&error);
            return gtk::glib::ExitCode::FAILURE;
        }
    };
    #[cfg(target_os = "windows")]
    if let Err(error) = windows::prepare_runtime() {
        windows::show_error(&error);
        return gtk::glib::ExitCode::FAILURE;
    }
    gtk::glib::set_application_name("Школьный день");
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_activate(|app| {
        // Повторный запуск показывает существующее окно с теми же настройками.
        if let Some(window) = app.active_window() {
            window.present();
        } else {
            build_window(app);
        }
    });
    app.run()
}

fn build_window(app: &adw::Application) {
    let data_guard = match storage::lock_data() {
        Ok(guard) => guard,
        Err(error) => {
            let content = column(18);
            margins(&content, 24);
            content.append(&text("Не удалось открыть данные", "title"));
            content.append(&text(&error, "section"));
            let close = primary("Закрыть");
            content.append(&close);
            let window = adw::ApplicationWindow::builder()
                .application(app)
                .title("Школьный день")
                .default_width(520)
                .default_height(220)
                .content(&content)
                .build();
            let weak_window = window.downgrade();
            close.connect_clicked(move |_| {
                if let Some(window) = weak_window.upgrade() {
                    window.close();
                }
            });
            window.present();
            return;
        }
    };
    gtk::Window::set_default_icon_name(APP_ID);
    let css = gtk::CssProvider::new();
    css.load_from_data(CSS);
    gtk::style_context_add_provider_for_display(
        &gtk::gdk::Display::default().expect("Нужен графический сеанс"),
        &css,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    let root = column(0);
    let header = adw::HeaderBar::new();
    if let Some(layout) =
        gtk::Settings::default().and_then(|settings| settings.gtk_decoration_layout())
    {
        let layout = layout
            .split(':')
            .map(|side| {
                side.split(',')
                    .filter(|item| !matches!(item.trim(), "icon" | "menu"))
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .collect::<Vec<_>>()
            .join(":");
        header.set_decoration_layout(Some(&layout));
    }
    let brand = text("Школьный день", "brand");
    brand.set_wrap(false);
    brand.set_ellipsize(gtk::pango::EllipsizeMode::End);
    brand.set_max_width_chars(24);
    brand.set_tooltip_text(Some("Школьный день"));
    let identity = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    identity.set_margin_start(10);
    identity.append(&brand_logo());
    identity.append(&brand);
    brand
        .bind_property("tooltip-text", &identity, "tooltip-text")
        .sync_create()
        .build();
    header.pack_start(&identity);
    root.append(&header);

    let pages = gtk::Stack::new();
    pages.set_transition_type(gtk::StackTransitionType::Crossfade);
    pages.set_transition_duration(120);
    pages.set_vhomogeneous(false);
    let status = text("", "status");
    status.set_visible(false);
    // Обе вкладки получают ссылки на один список уроков и домашних заданий.
    let lessons_loaded = match lessons::load_lessons() {
        Ok(loaded) => {
            let items = lessons::Lessons::new(loaded);
            pages.add_named(&schedule_page(&status, items.clone()), Some("schedule"));
            pages.add_named(&homework::homework_page(&status, items), Some("homework"));
            true
        }
        Err(error) => {
            // При ошибке загрузки не создаём пустой список для записи поверх файла.
            for (name, title) in [("schedule", "Расписание"), ("homework", "ДЗ")] {
                let content = page(title);
                content.append(&text(
                    &format!("Не удалось загрузить уроки: {error}"),
                    "muted",
                ));
                pages.add_named(&content, Some(name));
            }
            false
        }
    };
    pages.add_named(&settings::settings_page(&status, &brand), Some("settings"));
    let navigation = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    header.set_title_widget(Some(&navigation));
    let compact = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
        adw::BreakpointConditionLengthType::MaxWidth,
        640.0,
        adw::LengthUnit::Px,
    ));
    compact.add_setter(&brand, "visible", Some(&false.to_value()));
    let mut first: Option<gtk::ToggleButton> = None;
    let tabs = [
        ("schedule", "Расписание", "x-office-calendar-symbolic"),
        ("homework", "ДЗ", "document-edit-symbolic"),
        ("settings", "Настройки", "emblem-system-symbolic"),
    ]
    .map(|(name, title, icon)| {
        let button = gtk::ToggleButton::new();
        button.set_tooltip_text(Some(title));
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        row.append(&gtk::Image::from_icon_name(icon));
        let label = text(title, "section");
        label.set_wrap(false);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        compact.add_setter(&label, "visible", Some(&false.to_value()));
        row.append(&label);
        button.set_child(Some(&row));
        button.add_css_class("flat");
        button.add_css_class("nav");
        if let Some(ref leader) = first {
            button.set_group(Some(leader));
        } else {
            button.set_active(true);
            first = Some(button.clone());
        }
        // clone создаёт ещё одну ссылку на объект GTK; move сохраняет её в обработчике.
        let (pages, status) = (pages.clone(), status.clone());
        button.connect_toggled(move |button| {
            if button.is_active() {
                pages.set_visible_child_name(name);
                status.set_visible(false);
            }
        });
        navigation.append(&button);
        button
    });
    let workspace = column(18);
    margins(&workspace, 32);
    for margin in ["margin-start", "margin-end", "margin-top", "margin-bottom"] {
        compact.add_setter(&workspace, margin, Some(&16.to_value()));
    }
    workspace.set_valign(gtk::Align::Center);
    workspace.set_vexpand(true);
    workspace.append(&pages);
    workspace.append(&status);
    let clamp = adw::Clamp::new();
    clamp.set_maximum_size(680);
    clamp.set_tightening_threshold(500);
    clamp.set_child(Some(&workspace));
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
    scroll.set_hexpand(true);
    scroll.set_vexpand(true);
    scroll.set_child(Some(&clamp));
    let weak_scroll = scroll.downgrade();
    pages.connect_visible_child_notify(move |_| {
        if let Some(scroll) = weak_scroll.upgrade() {
            scroll.vadjustment().set_value(0.0);
            scroll.hadjustment().set_value(0.0);
        }
    });
    root.append(&scroll);
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&root));
    compact.add_setter(
        &overlay,
        "css-classes",
        Some(&gtk::glib::StrV::from(["compact-layout"]).to_value()),
    );
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Школьный день")
        .default_width(1040)
        .default_height(740)
        .width_request(360)
        .height_request(320)
        .content(&overlay)
        .build();
    window.add_breakpoint(compact);
    window.connect_destroy(move |_| {
        let _ = &data_guard;
    });
    tutorial::install(
        &window,
        &overlay,
        &scroll,
        &pages,
        tabs,
        &status,
        lessons_loaded,
    );
    window.present();
}

fn brand_logo() -> gtk::DrawingArea {
    let logo = gtk::DrawingArea::new();
    logo.set_content_width(22);
    logo.set_content_height(22);
    logo.set_valign(gtk::Align::Center);
    logo.add_css_class("brand-logo");
    logo.set_draw_func(|logo, context, width, height| {
        let color = logo.style_context().color();
        context.set_source_rgba(
            color.red().into(),
            color.green().into(),
            color.blue().into(),
            color.alpha().into(),
        );
        let scale = f64::from(width.min(height)) / 320.0;
        context.translate(f64::from(width) / 2.0, f64::from(height) / 2.0);
        context.scale(scale, scale);
        context.translate(-256.0, -256.0);
        context.set_line_width(22.0);
        context.set_line_cap(gtk::cairo::LineCap::Round);
        context.set_line_join(gtk::cairo::LineJoin::Round);
        context.move_to(144.0, 208.0);
        context.curve_to(181.0, 181.0, 218.0, 181.0, 256.0, 208.0);
        context.curve_to(294.0, 181.0, 331.0, 181.0, 368.0, 208.0);
        context.line_to(368.0, 328.0);
        context.curve_to(331.0, 301.0, 294.0, 301.0, 256.0, 328.0);
        context.curve_to(218.0, 301.0, 181.0, 301.0, 144.0, 328.0);
        context.close_path();
        context.move_to(256.0, 208.0);
        context.line_to(256.0, 328.0);
        let _ = context.stroke();
    });
    logo
}

fn column(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Vertical, spacing)
}

fn text(value: &str, style: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(value));
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.add_css_class(style);
    label
}

fn margins(widget: &impl IsA<gtk::Widget>, size: i32) {
    widget.set_margin_top(size);
    widget.set_margin_bottom(size);
    widget.set_margin_start(size);
    widget.set_margin_end(size);
}

fn page(title: &str) -> gtk::Box {
    let page = column(24);
    page.append(&text(title, "title"));
    page
}

fn empty_state(icon: &str, title: &str, description: &str) -> gtk::Box {
    let panel = column(12);
    panel.add_css_class("panel");
    let image = gtk::Image::from_icon_name(icon);
    image.set_pixel_size(24);
    image.add_css_class("empty-icon");
    image.set_margin_top(12);
    panel.append(&image);
    for (value, style) in [(title, "section"), (description, "muted")] {
        let label = text(value, style);
        label.set_justify(gtk::Justification::Center);
        label.set_halign(gtk::Align::Center);
        panel.append(&label);
    }
    panel
}

fn field(form: &gtk::Box, title: &str, placeholder: &str) -> gtk::Entry {
    form.append(&text(title, "section"));
    let entry = gtk::Entry::builder().placeholder_text(placeholder).build();
    form.append(&entry);
    entry
}

fn primary(title: &str) -> gtk::Button {
    let button = gtk::Button::with_label(title);
    button.add_css_class("primary");
    button.set_halign(gtk::Align::Center);
    button
}

fn notify(status: &gtk::Label, message: &str) {
    status.set_text(message);
    status.set_visible(true);
}

fn confirm_delete(status: &gtk::Label, heading: &str, body: &str, confirm: impl Fn() + 'static) {
    let Some(parent) = status
        .root()
        .and_then(|root| root.downcast::<gtk::Window>().ok())
    else {
        return;
    };
    let content = column(18);
    margins(&content, 24);
    content.append(&text(heading, "settings-title"));
    content.append(&text(body, "section"));
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    actions.set_halign(gtk::Align::End);
    let cancel = gtk::Button::with_label("Отмена");
    let delete = gtk::Button::with_label("Удалить");
    delete.add_css_class("destructive-action");
    actions.append(&cancel);
    actions.append(&delete);
    content.append(&actions);
    let window = adw::Window::builder()
        .title(heading)
        .default_width(420)
        .resizable(false)
        .modal(true)
        .transient_for(&parent)
        .content(&content)
        .build();
    window.set_destroy_with_parent(true);
    let weak = window.downgrade();
    cancel.connect_clicked(move |_| {
        if let Some(window) = weak.upgrade() {
            window.close();
        }
    });
    let weak = window.downgrade();
    delete.connect_clicked(move |_| {
        confirm();
        if let Some(window) = weak.upgrade() {
            window.close();
        }
    });
    let keys = gtk::EventControllerKey::new();
    let weak = window.downgrade();
    keys.connect_key_pressed(move |_, key, _, _| {
        if key == gtk::gdk::Key::Escape {
            if let Some(window) = weak.upgrade() {
                window.close();
            }
            gtk::glib::Propagation::Stop
        } else {
            gtk::glib::Propagation::Proceed
        }
    });
    window.add_controller(keys);
    window.present();
    cancel.grab_focus();
}
