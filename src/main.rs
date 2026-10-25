mod lessons;
mod schedule;

use adw::prelude::*;
use schedule::schedule_page;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Reminder {
    title: String,
    date: chrono::NaiveDate,
    time: chrono::NaiveTime,
    completed: bool,
}

fn save_reminders(reminders: &[Reminder]) -> Result<(), String> {
    let json = serde_json::to_string_pretty(reminders).map_err(|error| error.to_string())?;

    std::fs::create_dir_all("data").map_err(|error| error.to_string())?;

    std::fs::write("data/reminders.json.tmp", json).map_err(|error| error.to_string())?;

    std::fs::rename("data/reminders.json.tmp", "data/reminders.json")
        .map_err(|error| error.to_string())?;

    Ok(())
}

fn load_reminders() -> Result<Vec<Reminder>, String> {
    let json = match std::fs::read_to_string("data/reminders.json") {
        Ok(contents) => contents,

        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Vec::new());
        }

        Err(error) => {
            return Err(error.to_string());
        }
    };

    serde_json::from_str(&json).map_err(|error| error.to_string())
}

// оформление
const CSS: &str = r#"
window { background: #222225; color: #dedee1; }
headerbar { background: #28282b; box-shadow: none; border-bottom: 1px solid #303033; min-height: 48px; }
.brand { font-size: 12px; font-weight: 600; color: #9b9ba2; }
.title { font-size: 23px; font-weight: 600; letter-spacing: -0.4px; }
.muted { color: #919197; }
.section { font-size: 13px; font-weight: 500; }
.nav { padding: 7px 12px; border-radius: 6px; color: #99999f; }
.nav:checked { background: #38383d; color: #ededf0; }
.nav:hover { background: #323236; }
.panel { padding: 20px 0; border-top: 1px solid #36363b; }
.empty-icon { color: #77777e; }
button { box-shadow: none; }
button.primary { background: #38383e; color: #ededf0; border: 1px solid #4b4b52; padding: 9px 16px; border-radius: 6px; }
button.primary:hover { background: #424248; }
entry { background: #262629; border: 1px solid #3d3d43; padding: 8px 10px; border-radius: 6px; }
entry:focus-within { border-color: #8a8a95; }
.status { color: #bcbcc4; padding-top: 12px; border-top: 1px solid #36363b; }
"#;

fn main() -> gtk::glib::ExitCode {
    let app = adw::Application::builder()
        .application_id("io.github.sqeezy.SchoolAppLearning")
        .build();
    app.connect_activate(build_window);
    app.run()
}

fn build_window(app: &adw::Application) {
    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceDark);
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
    pages.add_named(&schedule_page(&status), Some("schedule"));
    pages.add_named(&import_page(&status), Some("import"));
    pages.add_named(&reminders_page(&status), Some("reminders"));
    pages.add_named(&settings_page(&status), Some("settings"));
    let navigation = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    header.set_title_widget(Some(&navigation));
    let mut first: Option<gtk::ToggleButton> = None;
    for (name, title, icon) in [
        ("schedule", "Расписание", "x-office-calendar-symbolic"),
        ("import", "Импорт фото", "image-x-generic-symbolic"),
        ("reminders", "Напоминания", "alarm-symbolic"),
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

fn import_page(status: &gtk::Label) -> gtk::Box {
    let page = page("Импорт фото");
    let empty = empty_state(
        "image-x-generic-symbolic",
        "Фотография расписания",
        "Выберите изображение, затем выделите колонку своего класса.",
    );
    empty.append(&action("Выбрать файл", status, on_import_photo));
    page.append(&empty);
    page
}

fn reminders_page(status: &gtk::Label) -> gtk::Box {
    let page = page("Напоминания");

    let loaded = match load_reminders() {
        Ok(items) => items,
        Err(error) => {
            page.append(&text(
                &format!("Не получилось загрузить напоминание: {error}"),
                "muted",
            ));
            return page;
        }
    };

    let reminders = Rc::new(RefCell::new(loaded));

    let form = column(16);
    form.add_css_class("panel");

    let title = field(&form, "Название", "Что нужно сделать");
    let date = field(&form, "Дата", "ДД.ММ.ГГГГ");
    let time = field(&form, "Время", "ЧЧ:ММ");
    let button = primary("Добавить напоминание");

    let list = column(16);
    render_reminders(&list, &reminders, status);

    let list_for_button = list.clone();
    let status = status.clone();

    button.connect_clicked(move |_| {
        on_add_reminder(
            title.text().as_str(),
            date.text().as_str(),
            time.text().as_str(),
            &status,
            &reminders,
        );

        render_reminders(&list_for_button, &reminders, &status);
    });

    form.append(&button);
    page.append(&form);
    page.append(&list);
    page
}

//отображение напоминаний
fn render_reminders(list: &gtk::Box, reminders: &Rc<RefCell<Vec<Reminder>>>, status: &gtk::Label) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }

    let items = reminders.borrow();

    if items.is_empty() {
        list.append(&text("Нету напоминаний", "muted"));
        return;
    }

    for (index, reminder) in items.iter().enumerate() {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        row.add_css_class("panel");
        let checkbox = gtk::CheckButton::new();
        checkbox.set_valign(gtk::Align::Start);
        checkbox.set_active(reminder.completed);

        let details = column(6);
        details.set_hexpand(true);

        details.append(&text(&reminder.title, "section"));

        let when = format!(
            "{} в {}",
            reminder.date.format("%d.%m.%Y"),
            reminder.time.format("%H:%M"),
        );

        details.append(&text(&when, "muted"));

        let delete_button = gtk::Button::from_icon_name("user-trash");
        delete_button.add_css_class("flat");
        delete_button.set_tooltip_text(Some("Удалить напоминание"));
        delete_button.set_valign(gtk::Align::Start);

        let reminders_for_delete = Rc::clone(reminders);
        let status_for_delete = status.clone();
        let list_for_delete = list.downgrade();
        delete_button.connect_clicked(move |_| {
            let Some(list) = list_for_delete.upgrade() else {
                return;
            };
            if on_delete_remander(Some(index), &reminders_for_delete, &status_for_delete) {
                render_reminders(&list, &reminders_for_delete, &status_for_delete);
            }
        });

        let reminders_for_checkbox = Rc::clone(reminders);
        let status = status.clone();
        checkbox.connect_toggled(move |checkbox| {
            let mut items = reminders_for_checkbox.borrow_mut();
            if let Some(reminder) = items.get_mut(index) {
                let previous = reminder.completed;
                let active = checkbox.is_active();
                if previous == active {
                    return;
                }
                reminder.completed = active;

                match save_reminders(&items) {
                    Ok(()) => notify(&status, "Сохранено"),

                    Err(eror) => {
                        items[index].completed = previous;
                        // Освобождаем список перед set_active: он снова вызывает toggled.
                        drop(items);
                        checkbox.set_active(previous);
                        notify(&status, &format!("Ошибка:(: {eror}"));
                    }
                }
            }
        });

        row.append(&checkbox);
        row.append(&details);
        row.append(&delete_button);
        list.append(&row);
    }

    let clear_button = gtk::Button::with_label("Удалить выполненные");
    clear_button.add_css_class("flat");
    clear_button.set_halign(gtk::Align::Start);

    let reminders_for_clear = Rc::clone(reminders);
    let status_for_clear = status.clone();
    let list_for_clear = list.downgrade();

    clear_button.connect_clicked(move |_| {
        let Some(list) = list_for_clear.upgrade() else {
            return;
        };
        if on_delete_remander(None, &reminders_for_clear, &status_for_clear) {
            render_reminders(&list, &reminders_for_clear, &status_for_clear);
        }
    });

    list.append(&clear_button);
}

fn settings_page(status: &gtk::Label) -> gtk::Box {
    let page = page("Настройки");
    let form = column(16);
    form.add_css_class("panel");
    let class = field(&form, "Класс", "Например, 7А");
    let button = primary("Сохранить");
    let status = status.clone();
    button.connect_clicked(move |_| on_save_settings(class.text().as_str(), &status));
    form.append(&button);
    page.append(&form);
    page
}

fn on_import_photo(status: &gtk::Label) {
    // Открой выбор файла, затем покажи изображение для кадрирования.
    notify(status, "Импорт фотографий пока недоступен.");
}

fn on_add_reminder(
    title: &str,
    date: &str,
    time: &str,
    status: &gtk::Label,
    reminders: &RefCell<Vec<Reminder>>,
) {
    let title = title.trim();
    if title.is_empty() {
        notify(status, "Введите название напоминания");
        return;
    }

    let date = match chrono::NaiveDate::parse_from_str(date.trim(), "%d.%m.%Y") {
        Ok(parsed_date) if parsed_date.format("%d.%m.%Y").to_string() == date.trim() => parsed_date,
        _ => {
            notify(status, "Введите существующую дату в формате ДД.ММ.ГГГГ");
            return;
        }
    };

    let time = match chrono::NaiveTime::parse_from_str(time.trim(), "%H:%M") {
        Ok(parsed_time) => parsed_time,
        Err(_) => {
            notify(status, "Введите время");
            return;
        }
    };

    //запрет дубля

    let exists = reminders.borrow().iter().any(|reminder| {
        reminder.title.to_lowercase() == title.to_lowercase()
            && reminder.date == date
            && reminder.time == time
    });

    if exists {
        notify(status, "Нельзя дублировать напоминания");
        return;
    }

    let reminder = Reminder {
        title: title.to_string(),
        date,
        time,
        completed: false,
    };

    let message = format!(
        "{} - {} в {}. Сохранено.",
        reminder.title,
        reminder.date.format("%d.%m.%Y"),
        reminder.time.format("%H:%M"),
    );

    let mut items = reminders.borrow_mut();
    items.push(reminder);

    if let Err(error) = save_reminders(&items) {
        items.pop();
        notify(
            status,
            &format!("Напоминание не добавлено, произошла ошибка:( : {error}"),
        );
        return;
    }

    notify(status, &message);
}

fn on_delete_remander(
    index: Option<usize>,
    reminders: &RefCell<Vec<Reminder>>,
    status: &gtk::Label,
) -> bool {
    let mut updated = reminders.borrow().clone();
    let old_count = updated.len();

    match index {
        Some(index) => {
            if index >= updated.len() {
                notify(status, "Напоминание не найдено:(");
                return false;
            }

            updated.remove(index);
        }

        None => {
            updated.retain(|reminder| !reminder.completed);
        }
    }

    let removed_count = old_count - updated.len();

    if removed_count == 0 {
        notify(status, "Выполните хотя бы одно напоминание");
        return false;
    }

    if let Err(error) = save_reminders(&updated) {
        notify(status, &format!("Ошибка:( {error}"));
        return false;
    }

    *reminders.borrow_mut() = updated;
    notify(status, &format!("Было удалено {removed_count} напоминаний"));
    true
}

fn on_save_settings(class: &str, status: &gtk::Label) {
    let _ = class;
    notify(status, "Сохранение настроек пока не сделано")
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

fn action(title: &str, status: &gtk::Label, handler: fn(&gtk::Label)) -> gtk::Button {
    let button = primary(title);
    let status = status.clone();
    button.connect_clicked(move |_| handler(&status));
    button
}

fn notify(status: &gtk::Label, message: &str) {
    status.set_text(message);
    status.set_visible(true);
}
