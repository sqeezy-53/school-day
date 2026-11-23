#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod bells;
mod forms;
mod homework;
mod lessons;
mod renderer;
mod schedule;
mod search;
mod seasonal;
mod settings;
mod startup;
mod storage;
mod sync_host;
mod sync_ui;
mod transfer;
mod tutorial;
mod undo;
mod updates;
#[cfg(target_os = "windows")]
mod windows;

use adw::prelude::*;
use schedule::schedule_page;

const APP_ID: &str = "io.github.sqeezy_53.school-day";

const CSS: &str = include_str!("style.css");

fn main() -> gtk::glib::ExitCode {
    if let Err(error) = renderer::prepare() {
        eprintln!("Не удалось запустить School Day с Cairo: {error}");
        return gtk::glib::ExitCode::FAILURE;
    }
    #[cfg(target_os = "windows")]
    let _running = match windows::initialize() {
        Ok(guard) => guard,
        Err(error) if error == windows::ALREADY_RUNNING && windows::show_running_window() => {
            return gtk::glib::ExitCode::SUCCESS;
        }
        Err(error) => {
            windows::show_error(&error);
            return gtk::glib::ExitCode::FAILURE;
        }
    };

    gtk::glib::set_application_name("Школьный день");
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_activate(|app| {
        if let Some(window) = app
            .active_window()
            .or_else(|| app.windows().into_iter().next())
        {
            window.present();
        } else {
            build_window(app);
        }
    });
    let result = app.run_with_args_os(&std::env::args_os().collect::<Vec<_>>());
    #[cfg(windows)]
    {
        drop(app);
        drop(_running);
        if let Err(error) = updates::install_pending() {
            windows::show_error(&error);
            return gtk::glib::ExitCode::FAILURE;
        }
    }
    result
}

fn build_window(app: &adw::Application) {
    gtk::Window::set_default_icon_name(APP_ID);
    let css = gtk::CssProvider::new();
    css.load_from_data(&format!("{CSS}\n{}", seasonal::css()));
    gtk::style_context_add_provider_for_display(
        &gtk::gdk::Display::default().expect("Нужен графический сеанс"),
        &css,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    let stack = gtk::Stack::new();
    stack.set_hhomogeneous(false);
    stack.set_vhomogeneous(false);
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Школьный день")
        .default_width(1040)
        .default_height(700)
        .width_request(360)
        .height_request(320)
        .content(&stack)
        .build();
    gtk::glib::spawn_future_local(async move {
        let locked = gtk::gio::spawn_blocking(|| {
            let guard = storage::lock_data()?;
            Ok::<_, String>((guard, settings::load_settings()))
        })
        .await
        .unwrap_or_else(|_| Err("Не удалось открыть данные".into()));
        let (guard, config) = match locked {
            Ok(startup) => startup,
            Err(error) => {
                show_startup_error(&window, &error);
                return;
            }
        };
        if let Ok(config) = &config {
            settings::apply_color_scheme(config);
            settings::set_display(config.display);
        }
        let animated = startup::show(&stack);
        if animated {
            window.present();
        }
        let weak = window.downgrade();
        drop(window);
        let bells = config.as_ref().ok().map(|value| value.bells.clone());
        let loaded = gtk::gio::spawn_blocking(move || {
            let sync_error = school_day_core::lan::recover(&storage::data_dir()).err();
            let schedule = lessons::read_schedule(bells);
            (guard, schedule, sync_error)
        })
        .await;
        let Some(window) = weak.upgrade() else {
            return;
        };
        let (guard, schedule, sync_error) = match loaded {
            Ok(loaded) => loaded,
            Err(_) => {
                show_startup_error(&window, "Не удалось загрузить расписание.");
                return;
            }
        };
        let workspace = build_workspace(&window, guard, config, schedule, sync_error);
        stack.add_named(&workspace, Some(startup::WORKSPACE));
        if !animated {
            window.present();
        }
    });
}

fn show_startup_error(window: &adw::ApplicationWindow, error: &str) {
    let content = column(18);
    margins(&content, 24);
    content.append(&text("Не удалось открыть данные", "page-title"));
    content.append(&text(error, "section"));
    let close = primary("Закрыть");
    content.append(&close);
    let weak = window.downgrade();
    close.connect_clicked(move |_| {
        if let Some(window) = weak.upgrade() {
            window.close();
        }
    });
    window.set_default_size(520, 220);
    window.set_content(Some(&content));
    window.present();
}

fn build_workspace(
    window: &adw::ApplicationWindow,
    data_guard: std::fs::File,
    config: Result<settings::Settings, String>,
    schedule: Result<(lessons::SchoolData, Option<std::path::PathBuf>), String>,
    sync_error: Option<school_day_core::lan::Error>,
) -> gtk::Overlay {
    let app = window
        .application()
        .and_downcast::<adw::Application>()
        .unwrap();
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
    let logo = brand_logo();
    identity.append(&logo);
    identity.append(&brand);
    brand
        .bind_property("tooltip-text", &identity, "tooltip-text")
        .sync_create()
        .build();
    header.pack_start(&identity);
    root.append(&header);

    let pages = gtk::Stack::new();
    pages.set_vhomogeneous(false);
    let status = text("", "status");
    status.set_visible(false);
    let undo = undo::UndoBar::new(&status);
    let undo_handle = undo.handle();
    let data_guard = std::sync::Arc::new(data_guard);
    let local_changes = sync_host::LocalChanges::default();
    let mut model = None;
    let schedule_calendar = match schedule {
        Ok((data, migration)) => {
            let changes = local_changes.clone();
            let items = lessons::Lessons::with_writer(data, move |updated| {
                storage::save_schedule(updated)?;
                changes.saved();
                Ok(())
            });
            if let Some(path) = migration {
                notify(
                    &status,
                    &format!(
                        "Расписание переведено в новый формат. Исходная копия: {}",
                        path.display()
                    ),
                );
            }
            let (schedule, calendar) = schedule_page(&status, items.clone(), Some(&undo_handle));
            model = Some(items.clone());
            pages.add_named(&schedule, Some("schedule"));
            let homework = homework::homework_page(&status, items.clone(), Some(&undo_handle));
            pages.add_named(&homework, Some("homework"));
            header.pack_end(&search::button(&items, &schedule, &homework));
            reminders(&app, items);
            Some(calendar)
        }
        Err(error) => {
            for (name, title) in [("schedule", "Расписание"), ("homework", "ДЗ")] {
                let content = page(title);
                content.append(&text(
                    &format!("Не удалось загрузить уроки: {error}"),
                    "form-error",
                ));
                content.append(&transfer::recovery_button(&status));
                pages.add_named(&content, Some(name));
            }
            None
        }
    };
    if let Some(error) = sync_error {
        notify(&status, &format!("Обмен не завершён. {error}"));
    }
    let service = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    service.add_css_class("service-bar");
    let sync_host = model
        .as_ref()
        .map(|items| sync_host::Host::new(items, &local_changes, data_guard.clone()));
    if let Some(host) = sync_host.clone() {
        window.connect_close_request(move |window| {
            if host.background() {
                #[cfg(target_os = "windows")]
                window.minimize();
                #[cfg(not(target_os = "windows"))]
                window.set_visible(false);
                gtk::glib::Propagation::Stop
            } else {
                gtk::glib::Propagation::Proceed
            }
        });
    }
    let settings_page = settings::settings_page(
        model.as_ref(),
        &status,
        &brand,
        &service,
        config,
        sync_host.as_ref(),
    );
    pages.add_named(&settings_page, Some("settings"));
    let settings_page = settings_page.downgrade();
    pages.connect_visible_child_notify(move |pages| {
        if pages.visible_child_name().as_deref() != Some("settings")
            && let Some(page) = settings_page.upgrade()
        {
            let _ = page.activate_action("school-settings.home", None);
        }
    });
    let navigation = gtk::Box::new(gtk::Orientation::Horizontal, 3);
    navigation.add_css_class("navigation");
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
        button.add_css_class("nav");
        if let Some(ref leader) = first {
            button.set_group(Some(leader));
        } else {
            button.set_active(true);
            first = Some(button.clone());
        }
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
    let nav = tabs.each_ref().map(|b| b.downgrade());
    pages.connect_visible_child_notify(move |pages| {
        if let Some(name) = pages.visible_child_name()
            && let Some(i) = ["schedule", "homework", "settings"]
                .iter()
                .position(|n| *n == name.as_str())
            && let Some(button) = nav[i].upgrade()
        {
            button.set_active(true);
        }
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
    let update_button = service
        .first_child()
        .and_downcast::<gtk::MenuButton>()
        .map(|button| button.downgrade());
    let weak_scroll = scroll.downgrade();
    pages.connect_visible_child_notify(move |_| {
        if let Some(button) = update_button.as_ref().and_then(|button| button.upgrade()) {
            button.popdown();
        }
        if let Some(scroll) = weak_scroll.upgrade() {
            scroll.vadjustment().set_value(0.0);
            scroll.hadjustment().set_value(0.0);
        }
    });
    root.append(&scroll);
    root.append(undo.widget());
    root.append(&service);
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&root));
    compact.add_setter(
        &overlay,
        "css-classes",
        Some(&gtk::glib::StrV::from(["compact-layout"]).to_value()),
    );
    window.add_breakpoint(compact);
    if let Some(calendar) = &schedule_calendar {
        seasonal::attach(window, &overlay, calendar);
    }
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let shortcut_tabs = tabs.each_ref().map(|tab| tab.downgrade());
    let weak_root = root.downgrade();
    let weak_window = window.downgrade();
    let history_status = status.downgrade();
    let history_pages = pages.downgrade();
    keys.connect_key_pressed(move |_, key, _, modifiers| {
        use gtk::gdk::{Key, ModifierType};
        if !modifiers.contains(ModifierType::CONTROL_MASK)
            || modifiers.intersects(
                ModifierType::SHIFT_MASK | ModifierType::ALT_MASK | ModifierType::SUPER_MASK,
            )
            || !weak_root
                .upgrade()
                .is_some_and(|root| root.is_mapped() && root.can_focus())
        {
            return gtk::glib::Propagation::Proceed;
        }
        let redo = matches!(key, Key::y | Key::Y | Key::Cyrillic_en | Key::Cyrillic_EN);
        if redo || matches!(key, Key::z | Key::Z | Key::Cyrillic_ya | Key::Cyrillic_YA) {
            let editing = weak_window
                .upgrade()
                .and_then(|w| gtk::prelude::GtkWindowExt::focus(&w))
                .is_some_and(|w| w.is::<gtk::Editable>() || w.is::<gtk::TextView>());
            if !editing
                && let (Some(model), Some(status), Some(pages)) = (
                    model.as_ref(),
                    history_status.upgrade(),
                    history_pages.upgrade(),
                )
            {
                match model.travel(redo) {
                    Ok(true) => {
                        undo.clear();
                        refresh_records(&pages);
                        notify(
                            &status,
                            if redo {
                                "Действие повторено"
                            } else {
                                "Действие отменено"
                            },
                        );
                    }
                    Ok(false) => return gtk::glib::Propagation::Proceed,
                    Err(e) => notify(&status, &e),
                }
                return gtk::glib::Propagation::Stop;
            }
        }
        let index = match key {
            Key::_1 | Key::KP_1 => 0,
            Key::_2 | Key::KP_2 => 1,
            Key::_3 | Key::KP_3 => 2,
            _ => return gtk::glib::Propagation::Proceed,
        };
        if let Some(tab) = shortcut_tabs[index].upgrade() {
            tab.grab_focus();
            tab.set_active(true);
        }
        gtk::glib::Propagation::Stop
    });
    window.add_controller(keys);
    window.connect_destroy(move |_| {
        let _ = &data_guard;
    });
    let tutorial = std::cell::Cell::new(Some(tabs));
    let weak_window = window.downgrade();
    let offer_tutorial = schedule_calendar.is_some();
    overlay.connect_map(move |overlay| {
        if let (Some(window), Some(tabs)) = (weak_window.upgrade(), tutorial.take()) {
            tutorial::install(
                &window,
                overlay,
                &scroll,
                &pages,
                tabs,
                &status,
                offer_tutorial,
            );
        }
    });
    overlay
}

fn brand_logo() -> gtk::DrawingArea {
    let logo = gtk::DrawingArea::new();
    logo.set_content_width(22);
    logo.set_content_height(22);
    logo.set_valign(gtk::Align::Center);
    logo.add_css_class("brand-logo");
    logo.set_draw_func(|logo, context, width, height| {
        let color = logo.color();
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
    let page = column(32);
    page.append(&text(title, "page-title"));
    page
}

fn empty_state(icon: &str, title: &str) -> gtk::Box {
    let panel = column(12);
    panel.add_css_class("panel");
    let image = gtk::Image::from_icon_name(icon);
    image.set_pixel_size(24);
    image.add_css_class("empty-icon");
    image.set_margin_top(12);
    panel.append(&image);
    let label = text(title, "section");
    label.set_justify(gtk::Justification::Center);
    label.set_halign(gtk::Align::Center);
    panel.append(&label);
    panel
}

fn field(form: &gtk::Box, title: &str, placeholder: &str) -> gtk::Entry {
    let entry = gtk::Entry::builder().placeholder_text(placeholder).build();
    let label = text(title, "section");
    label.set_mnemonic_widget(Some(&entry));
    form.append(&label);
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

fn refresh_records(pages: &gtk::Stack) {
    for name in ["schedule", "homework"] {
        if let Some(page) = pages.child_by_name(name) {
            let _ = page.activate_action("records.refresh", None);
        }
    }
}

fn confirm_delete(status: &gtk::Label, heading: &str, body: &str, confirm: impl Fn() + 'static) {
    if !settings::display().confirm_delete {
        confirm();
        return;
    }
    confirm_removal(status, heading, body, confirm);
}

fn confirm_removal(status: &gtk::Label, heading: &str, body: &str, confirm: impl Fn() + 'static) {
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
    forms::dialog_keys(&window, &cancel, None);
    window.present();
    cancel.grab_focus();
}

fn reminders(app: &adw::Application, items: lessons::Lessons) {
    let weak = app.downgrade();
    let mut previous = chrono::Local::now().naive_local();
    let mut sent = std::collections::HashSet::new();
    gtk::glib::timeout_add_seconds_local(30, move || {
        let Some(app) = weak.upgrade().filter(|app| app.active_window().is_some()) else {
            return gtk::glib::ControlFlow::Break;
        };
        let now = chrono::Local::now().naive_local();
        if now.date() != previous.date() {
            sent.clear();
        }
        if settings::display().reminders
            && now > previous
            && now - previous < chrono::Duration::seconds(90)
        {
            for item in items
                .on_date(now.date())
                .iter()
                .filter(|l| !l.options.cancelled)
            {
                let event = item.date.and_time(item.time) - chrono::Duration::minutes(5);
                if event > previous && event <= now && sent.insert(item.id) {
                    let message = gtk::gio::Notification::new("Урок через 5 минут");
                    let subject = if item.options.replacement.is_empty() {
                        &item.title
                    } else {
                        &item.options.replacement
                    };
                    message.set_body(Some(subject));
                    app.send_notification(Some(&item.id.to_string()), &message);
                }
            }
        }
        previous = now;
        gtk::glib::ControlFlow::Continue
    });
}
