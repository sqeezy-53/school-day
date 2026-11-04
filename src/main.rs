mod homework;
mod lessons;
mod schedule;
mod settings;

use adw::prelude::*;
use schedule::schedule_page;
use std::cell::RefCell;
use std::rc::Rc;

// оформление
const CSS: &str = r#"
window { background: @window_bg_color; color: @window_fg_color; }
headerbar { background: @headerbar_bg_color; box-shadow: none; border-bottom: 1px solid alpha(@window_fg_color, 0.10); min-height: 48px; }
.brand { font-size: 12px; font-weight: 600; opacity: 0.65; }
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
"#;

fn main() -> gtk::glib::ExitCode {
    let app = adw::Application::builder()
        .application_id("io.github.sqeezy.SchoolAppLearning")
        .build();
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
    let css = gtk::CssProvider::new();
    css.load_from_data(CSS);
    gtk::style_context_add_provider_for_display(
        &gtk::gdk::Display::default().expect("Нужен графический сеанс"),
        &css,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    let root = column(0);
    let header = adw::HeaderBar::new();
    let brand = text("Школьный день", "brand");
    brand.set_wrap(false);
    brand.set_margin_start(10);
    header.pack_start(&brand);
    root.append(&header);

    let pages = gtk::Stack::new();
    pages.set_transition_type(gtk::StackTransitionType::Crossfade);
    pages.set_transition_duration(120);
    pages.set_vhomogeneous(false);
    let status = text("", "status");
    status.set_visible(false);
    // Обе вкладки получают ссылки на один список уроков и домашних заданий.
    match lessons::load_lessons() {
        Ok(loaded) => {
            let items = Rc::new(RefCell::new(loaded));
            pages.add_named(&schedule_page(&status, Rc::clone(&items)), Some("schedule"));
            pages.add_named(&homework::homework_page(&status, items), Some("homework"));
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
        }
    }
    pages.add_named(&settings::settings_page(&status, &brand), Some("settings"));
    let navigation = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    header.set_title_widget(Some(&navigation));
    let mut first: Option<gtk::ToggleButton> = None;
    for (name, title, icon) in [
        ("schedule", "Расписание", "x-office-calendar-symbolic"),
        ("homework", "ДЗ", "document-edit-symbolic"),
        ("settings", "Настройки", "emblem-system-symbolic"),
    ] {
        let button = gtk::ToggleButton::new();
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        row.append(&gtk::Image::from_icon_name(icon));
        row.append(&text(title, "section"));
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
    }
    let workspace = column(18);
    margins(&workspace, 32);
    workspace.set_valign(gtk::Align::Center);
    workspace.set_vexpand(true);
    workspace.append(&pages);
    workspace.append(&status);
    let clamp = adw::Clamp::new();
    clamp.set_maximum_size(680);
    clamp.set_tightening_threshold(500);
    clamp.set_child(Some(&workspace));
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_hexpand(true);
    scroll.set_vexpand(true);
    scroll.set_child(Some(&clamp));
    root.append(&scroll);
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Школьный день")
        .default_width(1040)
        .default_height(740)
        .content(&root)
        .build();
    window.present();
}

fn column(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Vertical, spacing)
}

fn text(value: &str, style: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(value));
    label.set_xalign(0.0);
    label.set_wrap(true);
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
