use crate::lessons::{Lessons, SchoolData};
use crate::{notify, storage};
use adw::prelude::*;
use gtk::{gio, glib};

fn show_last(row: &adw::ActionRow) {
    let last = storage::latest_backup()
        .map(|date| date.format("%d.%m.%Y %H:%M").to_string())
        .unwrap_or_else(|| "Копий пока нет".into());
    row.set_subtitle(&last);
}

fn action(group: &adw::PreferencesGroup, title: &str, handler: impl Fn(&adw::ActionRow) + 'static) {
    let row = adw::ActionRow::builder()
        .title(title)
        .activatable(true)
        .build();
    row.connect_activated(handler);
    group.add(&row);
}

pub fn populate(group: &adw::PreferencesGroup, items: &Lessons, status: &gtk::Label) {
    let last = adw::ActionRow::builder()
        .title("Последняя резервная копия")
        .build();
    last.connect_map(show_last);
    group.add(&last);

    let (model, feedback, recent) = (items.clone(), status.downgrade(), last.downgrade());
    action(
        group,
        "Создать резервную копию",
        move |_| {
            if let Some(status) = feedback.upgrade() {
                match storage::backup(&model.data()) {
                    Ok(_) => {
                        if let Some(last) = recent.upgrade() {
                            show_last(&last);
                        }
                        notify(&status, "Резервная копия создана");
                    }
                    Err(error) => notify(&status, &error),
                }
            }
        },
    );

    let (model, feedback) = (items.clone(), status.downgrade());
    action(
        group,
        "Экспорт расписания и ДЗ",
        move |row| {
            if let (Some(parent), Some(status)) =
                (row.root().and_downcast::<gtk::Window>(), feedback.upgrade())
            {
                export(&parent, &model, &status);
            }
        },
    );
    for (title, backups) in [
        ("Восстановить из копии", true),
        ("Импорт расписания и ДЗ", false),
    ] {
        let (model, feedback, recent) = (items.clone(), status.downgrade(), last.downgrade());
        action(group, title, move |row| {
            if let (Some(parent), Some(status)) =
                (row.root().and_downcast::<gtk::Window>(), feedback.upgrade())
            {
                choose_import(
                    &parent,
                    &status,
                    Some(model.clone()),
                    recent.clone(),
                    backups,
                );
            }
        });
    }
}

pub fn about(group: &adw::PreferencesGroup) {
    group.add(
        &adw::ActionRow::builder()
            .title("Школьный день")
            .subtitle(format!("Версия {}", env!("CARGO_PKG_VERSION")))
            .build(),
    );
    let project = adw::ActionRow::builder()
        .title("Проект")
        .subtitle("GPL-3.0-only")
        .build();
    let link = gtk::LinkButton::with_label("https://github.com/sqeezy-53/school-day", "GitHub");
    link.set_valign(gtk::Align::Center);
    project.add_suffix(&link);
    project.set_activatable_widget(Some(&link));
    group.add(&project);
    action(group, "Техническая информация", |row| {
        let Some(parent) = row.root().and_downcast::<gtk::Window>() else {
            return;
        };
        let info = storage::device_id()
            .map(|id| format!("Идентификатор устройства: {id}"))
            .unwrap_or_else(|error| error);
        let dialog = adw::AlertDialog::builder()
            .heading("Техническая информация")
            .body(info)
            .build();
        dialog.add_response("close", "Закрыть");
        dialog.set_close_response("close");
        dialog.present(Some(&parent));
    });
}

pub fn system_requirements() -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("Системные требования")
        .build();
    for (platform, os) in [
        ("Windows", "Windows 10/11, 64-бит"),
        ("Linux", "64-битный Linux с поддержкой Flatpak"),
    ] {
        group.add(
            &adw::ActionRow::builder()
                .title(platform)
                .subtitle(format!(
                    "{os}\nx86-64, 2 ядра · ОЗУ от 4 ГБ (ориентировочный минимум)"
                ))
                .build(),
        );
    }
    group
}

fn file_dialog(title: &str) -> gtk::FileDialog {
    let filter = gtk::FileFilter::new();
    filter.add_pattern("*.json");
    filter.set_name(Some("Расписание School Day (*.json)"));
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    gtk::FileDialog::builder()
        .title(title)
        .filters(&filters)
        .build()
}

fn export(parent: &gtk::Window, model: &Lessons, status: &gtk::Label) {
    let dialog = file_dialog("Экспорт расписания и ДЗ");
    dialog.set_initial_name(Some("school-day.json"));
    let (parent, model, feedback) = (parent.downgrade(), model.clone(), status.downgrade());
    glib::spawn_future_local(async move {
        let result = dialog.save_future(parent.upgrade().as_ref()).await;
        let Some(status) = feedback.upgrade() else {
            return;
        };
        let file = match result {
            Ok(file) => file,
            Err(e) => {
                file_error(&status, e);
                return;
            }
        };
        let bytes = match serde_json::to_vec(&*model.data()) {
            Ok(bytes) => bytes,
            Err(e) => {
                notify(&status, &e.to_string());
                return;
            }
        };
        match file
            .replace_contents_future(bytes, None, false, gio::FileCreateFlags::PRIVATE)
            .await
        {
            Ok(_) => notify(&status, "Расписание и ДЗ экспортированы"),
            Err((_, e)) => file_error(&status, e),
        }
    });
}

fn file_error(status: &gtk::Label, error: glib::Error) {
    if !error.matches(gtk::DialogError::Dismissed) && !error.matches(gio::IOErrorEnum::Cancelled) {
        notify(status, &error.to_string());
    }
}

fn choose_import(
    parent: &gtk::Window,
    status: &gtk::Label,
    model: Option<Lessons>,
    recent: glib::WeakRef<adw::ActionRow>,
    backups: bool,
) {
    let dialog = file_dialog(if backups {
        "Открыть резервную копию"
    } else {
        "Импорт расписания и ДЗ"
    });
    if backups {
        dialog.set_initial_folder(Some(&gio::File::for_path(
            storage::data_dir().join("backups"),
        )));
    }
    let (owner, feedback) = (parent.downgrade(), status.downgrade());
    glib::spawn_future_local(async move {
        let selected = dialog.open_future(owner.upgrade().as_ref()).await;
        let Some(status) = feedback.upgrade() else {
            return;
        };
        let file = match selected {
            Ok(file) => file,
            Err(e) => {
                file_error(&status, e);
                return;
            }
        };
        let Some(path) = file.path() else {
            notify(&status, "Выберите локальный файл");
            return;
        };
        let data = match gio::spawn_blocking(move || storage::read_import(&path)).await {
            Ok(Ok(data)) => data,
            Ok(Err(e)) => {
                notify(&status, &e);
                return;
            }
            Err(_) => {
                notify(&status, "Не удалось прочитать файл");
                return;
            }
        };
        let Some(parent) = owner.upgrade() else {
            return;
        };
        let (lessons, homework, days) = data.counts();
        let confirm = adw::AlertDialog::builder().heading("Заменить расписание?")
            .body(format!("Уроков: {lessons}, заданий: {homework}, особых дней: {days}. Текущие данные будут сохранены в защитной копии.")).build();
        confirm.add_responses(&[("cancel", "Отмена"), ("replace", "Заменить")]);
        confirm.set_close_response("cancel");
        confirm.set_default_response(Some("cancel"));
        confirm.set_response_appearance("replace", adw::ResponseAppearance::Destructive);
        if confirm.choose_future(Some(&parent)).await != "replace" {
            return;
        }
        match restore(model.as_ref(), data) {
            Ok(msg) => {
                if let Some(row) = recent.upgrade() {
                    show_last(&row);
                }
                notify(&status, msg);
            }
            Err(e) => notify(&status, &e),
        }
    });
}

fn restore(model: Option<&Lessons>, data: SchoolData) -> Result<&'static str, String> {
    if let Some(model) = model {
        storage::backup(&model.data())?;
        model.import(data)?;
        Ok("Расписание и ДЗ восстановлены")
    } else {
        storage::reset_json("lessons.json", &data)?;
        Ok("Расписание восстановлено. Перезапустите приложение.")
    }
}

pub fn recovery_button(status: &gtk::Label) -> gtk::Button {
    let button = gtk::Button::with_label("Восстановить расписание из файла");
    let feedback = status.downgrade();
    button.connect_clicked(move |b| {
        if let (Some(parent), Some(status)) =
            (b.root().and_downcast::<gtk::Window>(), feedback.upgrade())
        {
            choose_import(&parent, &status, None, glib::WeakRef::new(), true);
        }
    });
    button
}
