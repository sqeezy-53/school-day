use adw::prelude::*;
use school_day_core::lan::{Cancel, Outcome, Profile, Review};
use school_day_core::lessons::sync::Side;
use std::net::{IpAddr, Ipv4Addr, SocketAddrV4};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender, TryRecvError};
use std::time::Duration;

use crate::sync_host::{Host, Mode};
use crate::{column, margins, notify, primary, storage, text};

pub static WRITING_SYNC: AtomicBool = AtomicBool::new(false);

enum Event {
    Invite(String),
    PairCode(String, Sender<bool>),
    Review(Review, Option<Vec<Side>>, Sender<Option<Vec<Side>>>),
    State {
        paired: bool,
        last_sync: Option<i64>,
    },
    Done(String, Option<Result<crate::lessons::SchoolData, String>>),
}

pub fn populate(
    group: &adw::PreferencesGroup,
    lessons: &crate::lessons::Lessons,
    status: &gtk::Label,
    host: &Host,
) {
    let modes = gtk::StringList::new(&["Автоматическая", "Вручную", "Отключена"]);
    let mode = adw::ComboRow::builder()
        .title("Режим синхронизации")
        .subtitle("После первого обмена включите автоматический режим на обоих устройствах")
        .model(&modes)
        .selected(host.mode().index())
        .build();
    mode.set_widget_name("sync-mode");
    group.add(&mode);

    let background = adw::SwitchRow::builder()
        .title("Работать при закрытом окне")
        .subtitle("Продолжать обмен до выхода из приложения")
        .active(host.background())
        .sensitive(host.mode() == Mode::Automatic)
        .build();
    if Host::background_available() {
        group.add(&background);
        let quit = adw::ActionRow::builder()
            .title("Завершить работу")
            .activatable(true)
            .build();
        quit.add_suffix(&gtk::Image::from_icon_name("application-exit-symbolic"));
        quit.connect_activated(|row| {
            if let Some(app) = row
                .root()
                .and_downcast::<gtk::Window>()
                .and_then(|w| w.application())
            {
                app.quit();
            }
        });
        group.add(&quit);
    }
    let background_ref = background.downgrade();
    let config = host.clone();
    let feedback = status.downgrade();
    mode.connect_selected_notify(move |row| {
        let Some(choice) = Mode::ALL.get(row.selected() as usize).copied() else {
            return;
        };
        if choice != config.mode()
            && let Err(error) = config.set_mode(choice)
        {
            row.set_selected(config.mode().index());
            if let Some(feedback) = feedback.upgrade() {
                notify(&feedback, &error);
            }
        }
        if let Some(background) = background_ref.upgrade() {
            background.set_sensitive(config.mode() == Mode::Automatic);
            background.set_active(config.background());
        }
    });
    let config = host.clone();
    let feedback = status.downgrade();
    background.connect_active_notify(move |row| {
        if config.mode() == Mode::Automatic
            && row.is_active() != config.background()
            && let Err(error) = config.set_background(row.is_active())
        {
            row.set_active(config.background());
            if let Some(feedback) = feedback.upgrade() {
                notify(&feedback, &error);
            }
        }
    });
    let open = adw::ActionRow::builder()
        .title("Устройства")
        .subtitle("Сопряжение, статус и ручной обмен")
        .activatable(true)
        .build();
    open.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    group.add(&open);
    let state = text("", "muted");
    host.bind_status(&state);
    group.add(&state);
    let items = lessons.clone();
    let status = status.downgrade();
    let host = host.clone();
    open.connect_activated(move |row| {
        if let (Some(window), Some(status)) =
            (row.root().and_downcast::<gtk::Window>(), status.upgrade())
        {
            device_dialog(&window, items.clone(), &status, host.clone());
        }
    });
}

fn device_dialog(
    window: &gtk::Window,
    items: crate::lessons::Lessons,
    status: &gtk::Label,
    host: Host,
) {
    let dialog = adw::Dialog::builder()
        .title("Синхронизация")
        .content_width(460)
        .content_height(250)
        .build();
    let body = column(12);
    margins(&body, 18);
    let tools = adw::ToolbarView::new();
    tools.add_top_bar(&adw::HeaderBar::new());
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_child(Some(&body));
    tools.set_content(Some(&scroll));
    dialog.set_child(Some(&tools));

    let info = text("Открываем настройки синхронизации…", "section");
    let controls = column(8);
    let stage = column(10);
    body.append(&info);
    body.append(&controls);
    body.append(&stage);

    let address = gtk::Entry::new();
    address.set_placeholder_text(Some("Локальный адрес компьютера"));
    let adapter_settings = column(6);
    let adapters = local_adapters();
    if let Some((_, default)) = adapters.first() {
        address.set_text(&default.to_string());
    }
    if adapters.len() > 1 {
        let labels = adapters
            .iter()
            .map(|(name, ip)| format!("{name} · {ip}"))
            .collect::<Vec<_>>();
        let dropdown =
            gtk::DropDown::from_strings(&labels.iter().map(String::as_str).collect::<Vec<_>>());
        let weak = address.downgrade();
        dropdown.connect_selected_notify(move |dropdown| {
            if let Some((_, ip)) = adapters.get(dropdown.selected() as usize)
                && let Some(address) = weak.upgrade()
            {
                address.set_text(&ip.to_string());
            }
        });
        adapter_settings.append(&dropdown);
    }
    adapter_settings.append(&address);
    let change_address = gtk::Button::with_label("Обновить адрес");
    adapter_settings.append(&change_address);
    adapter_settings.append(&text(
        "Если IP компьютера изменился, укажите новый адрес и на телефоне.",
        "muted",
    ));
    let network = gtk::Expander::new(Some("Параметры сети"));
    network.set_child(Some(&adapter_settings));
    let pair = primary("Подключить устройство");
    let exchange = primary("Синхронизировать");
    exchange.set_sensitive(host.mode() != Mode::Off);
    let revoke = gtk::Button::with_label("Отключить устройство");
    revoke.add_css_class("flat");
    controls.append(&pair);
    controls.append(&exchange);
    controls.append(&revoke);
    controls.append(&network);
    controls.set_sensitive(false);
    let active = Rc::new(std::cell::RefCell::new(None::<Cancel>));

    let draw_state = {
        let (pair, change_address, exchange, revoke, info) = (
            pair.downgrade(),
            change_address.downgrade(),
            exchange.downgrade(),
            revoke.downgrade(),
            info.downgrade(),
        );
        move |paired: bool, timestamp: Option<i64>| {
            let (Some(pair), Some(change_address), Some(exchange), Some(revoke), Some(info)) = (
                pair.upgrade(),
                change_address.upgrade(),
                exchange.upgrade(),
                revoke.upgrade(),
                info.upgrade(),
            ) else {
                return;
            };
            pair.set_visible(!paired);
            change_address.set_visible(paired);
            exchange.set_visible(paired);
            revoke.set_visible(paired);
            let stamp = timestamp
                .and_then(|value| chrono::DateTime::from_timestamp(value, 0))
                .map(|value| {
                    value
                        .with_timezone(&chrono::Local)
                        .format("%d.%m.%Y %H:%M")
                        .to_string()
                })
                .unwrap_or_else(|| "обмена ещё не было".into());
            info.set_text(&if paired {
                format!("Телефон подключён. Последний обмен: {stamp}.")
            } else {
                "Подключите телефон в той же локальной сети.".into()
            });
        }
    };
    let draw_state = Rc::new(draw_state);
    let dir = storage::data_dir();
    let profile_dir = dir.clone();
    let draw_initial = draw_state.clone();
    let weak_controls = controls.downgrade();
    gtk::glib::spawn_future_local(async move {
        let result = gtk::gio::spawn_blocking(move || {
            let profile = Profile::open(&profile_dir).map_err(|err| err.to_string())?;
            Ok::<_, String>((
                profile.peer().map_err(|err| err.to_string())?.is_some(),
                profile.last_sync().map_err(|err| err.to_string())?,
            ))
        })
        .await;
        if let Some(controls) = weak_controls.upgrade() {
            match result {
                Ok(Ok((paired, last))) => {
                    draw_initial(paired, last);
                    controls.set_sensitive(true);
                }
                Ok(Err(problem)) => info.set_text(&problem),
                Err(_) => info.set_text("Не удалось открыть настройки синхронизации."),
            }
        }
    });

    let launch = {
        let active = active.clone();
        let status = status.downgrade();
        let dialog = dialog.downgrade();
        let controls = controls.downgrade();
        let stage = stage.downgrade();
        let host = host.clone();
        move |action: Action| {
            let (Some(controls), Some(stage)) = (controls.upgrade(), stage.upgrade()) else {
                return;
            };
            let sync = matches!(action, Action::Sync);
            if sync && WRITING_SYNC.swap(true, Ordering::AcqRel) {
                return;
            }
            host.pause();
            let cancel = Cancel::default();
            *active.borrow_mut() = Some(cancel.clone());
            controls.set_sensitive(false);
            clear(&stage);
            stage.append(&text("Ожидаем подключение…", "section"));
            let abort = gtk::Button::with_label("Отменить обмен");
            stage.append(&abort);
            let stop = cancel.clone();
            abort.connect_clicked(move |button| {
                stop.stop();
                button.set_sensitive(false);
            });

            let (tx, rx) = mpsc::channel();
            let root = dir.clone();
            let bells = items.data().bells.clone();
            let data_guard = host.data_guard();
            std::thread::spawn(move || {
                let _data_guard = data_guard;
                let message = run(&root, action, &tx, &cancel);
                let schedule =
                    sync.then(|| crate::lessons::read_schedule(bells).map(|(data, _)| data));
                let _ = tx.send(Event::Done(message, schedule));
            });
            let stage = stage.downgrade();
            let controls = controls.downgrade();
            let status = status.clone();
            let window = dialog.clone();
            let state = draw_state.clone();
            let active = active.clone();
            let items = items.clone();
            let host = host.clone();
            gtk::glib::timeout_add_local(Duration::from_millis(35), move || {
                let stage = stage.upgrade();
                match rx.try_recv() {
                    Ok(Event::Invite(code)) => {
                        if let Some(dialog) = window.upgrade() {
                            dialog.set_content_height(445);
                        }
                        if let Some(stage) = stage.as_ref() {
                            show_invite(stage, &code);
                        }
                    }
                    Ok(Event::PairCode(code, answer)) => {
                        if let Some(dialog) = window.upgrade() {
                            dialog.set_content_height(290);
                        }
                        if let Some(stage) = stage.as_ref() {
                            clear(stage);
                            stage.append(&text("Сравните код с телефоном", "settings-title"));
                            stage.append(&text(&code, "title-2"));
                            answer_buttons(stage, answer, true, false, "Код совпадает");
                        }
                    }
                    Ok(Event::Review(review, proposed, answer)) => {
                        if let Some(dialog) = window.upgrade() {
                            dialog.set_content_height(if review.conflicts.is_empty() {
                                330
                            } else {
                                480
                            });
                        }
                        if let Some(stage) = stage.as_ref() {
                            show_review(stage, review, proposed, answer);
                        }
                    }
                    Ok(Event::State { paired, last_sync }) => state(paired, last_sync),
                    Ok(Event::Done(mut message, schedule)) => {
                        match schedule {
                            Some(Ok(data)) => {
                                items.adopt_durable_schedule(data);
                                WRITING_SYNC.store(false, Ordering::Release);
                            }
                            Some(Err(_)) => {
                                message = "Перезапустите приложение, чтобы открыть сохранённое расписание.".into();
                            }
                            None => {}
                        }
                        active.borrow_mut().take();
                        host.resume();
                        if let Some(dialog) = window.upgrade() {
                            dialog.set_content_height(250);
                        }
                        if let Some(stage) = stage.as_ref() {
                            clear(stage);
                            stage.append(&text(&message, "section"));
                        }
                        if let Some(controls) = controls.upgrade() {
                            controls.set_sensitive(true);
                        }
                        if let Some(label) = status.upgrade() {
                            notify(&label, &message);
                        }
                        return gtk::glib::ControlFlow::Break;
                    }
                    Err(TryRecvError::Empty) => {}
                    Err(TryRecvError::Disconnected) => {
                        host.resume();
                        if let Some(label) = status.upgrade() {
                            notify(
                                &label,
                                "Обмен не завершён. Проверьте расписание после перезапуска.",
                            );
                        }
                        return gtk::glib::ControlFlow::Break;
                    }
                }
                gtk::glib::ControlFlow::Continue
            });
        }
    };
    let launch = Rc::new(launch);
    let start = launch.clone();
    let pair_address = address.downgrade();
    pair.connect_clicked(move |_| {
        if let Some(address) = pair_address.upgrade() {
            start(Action::Pair(address.text().trim().to_owned()));
        }
    });
    let start = launch.clone();
    change_address.connect_clicked(move |_| {
        start(Action::Address(address.text().trim().to_owned()));
    });
    let start = launch.clone();
    exchange.connect_clicked(move |_| start(Action::Sync));
    let weak = dialog.downgrade();
    revoke.connect_clicked(move |_| {
        if let Some(dialog) = weak.upgrade() {
            let confirm = adw::AlertDialog::builder()
                .heading("Отключить устройство?")
                .body("Телефон потеряет доступ. Для нового обмена потребуется новое сопряжение.")
                .build();
            confirm.add_response("cancel", "Отмена");
            confirm.add_response("ok", "Отключить");
            confirm.set_response_appearance("ok", adw::ResponseAppearance::Destructive);
            confirm.set_close_response("cancel");
            let launch = launch.clone();
            confirm.connect_response(Some("ok"), move |_, _| launch(Action::Revoke));
            confirm.present(Some(&dialog));
        }
    });
    dialog.connect_closed(move |_| {
        if let Some(cancel) = active.borrow().as_ref() {
            cancel.stop();
        }
    });
    dialog.present(Some(window));
}

enum Action {
    Pair(String),
    Address(String),
    Sync,
    Revoke,
}

fn run(root: &std::path::Path, action: Action, tx: &Sender<Event>, cancel: &Cancel) -> String {
    let result = (|| {
        let profile = Profile::open(root).map_err(|error| error.to_string())?;
        let result = match action {
            Action::Pair(address) => {
                let ip = address
                    .parse()
                    .map_err(|_| "Укажите локальный IPv4-адрес компьютера")?;
                let offer = profile
                    .invite(SocketAddrV4::new(ip, 0))
                    .map_err(|error| error.to_string())?;
                let _ = tx.send(Event::Invite(offer.invitation().encode()));
                profile
                    .accept_pair(offer, cancel.clone(), |code| {
                        let (answer, reply) = mpsc::channel();
                        tx.send(Event::PairCode(code.to_owned(), answer)).is_ok()
                            && wait(reply, cancel).unwrap_or(false)
                    })
                    .map(|()| "Телефон подключён".to_owned())
            }
            Action::Sync => {
                let peer = profile
                    .peer()
                    .map_err(|error| error.to_string())?
                    .ok_or("Подключите устройство")?;
                let listener = crate::sync_host::bind(peer.endpoint(), cancel).map_err(
                    |_| "Не удалось открыть локальный сеанс. Проверьте адрес ПК и брандмауэр.",
                )?;
                profile
                    .serve(listener, cancel.clone(), |review, proposed| {
                        let (answer, reply) = mpsc::channel();
                        tx.send(Event::Review(
                            review.clone(),
                            proposed.map(<[Side]>::to_vec),
                            answer,
                        ))
                        .ok()?;
                        wait(reply, cancel).flatten()
                    })
                    .map(|outcome| {
                        match outcome {
                            Outcome::Synchronized | Outcome::Resumed => "Синхронизировано",
                            Outcome::Unchanged => "Все изменения синхронизированы",
                            Outcome::ReviewRequired => {
                                "Для проверки изменений откройте ручной обмен на телефоне"
                            }
                            Outcome::AwaitingReceipt => {
                                "Изменения сохранены. Ожидается подтверждение"
                            }
                            Outcome::Cancelled => "Обмен отменён",
                        }
                        .to_owned()
                    })
            }
            Action::Revoke => after_listener_stops(cancel, || profile.revoke())
                .map(|()| "Устройство отключено".to_owned()),
            Action::Address(value) => {
                let address = value
                    .parse()
                    .map_err(|_| "Укажите локальный IPv4-адрес компьютера")?;
                if !local_adapters().iter().any(|(_, ip)| *ip == address) {
                    return Err("Выберите адрес сетевого адаптера этого компьютера".to_owned());
                }
                after_listener_stops(cancel, || profile.change_address(address))
                    .map(|()| "Адрес обновлён. Укажите его на телефоне".to_owned())
            }
        };
        if let (Ok(peer), Ok(last_sync)) = (profile.peer(), profile.last_sync()) {
            let _ = tx.send(Event::State {
                paired: peer.is_some(),
                last_sync,
            });
        }
        result.map_err(|reason| reason.to_string())
    })();
    result.unwrap_or_else(|reason: String| format!("Обмен не завершён. {reason}"))
}

fn after_listener_stops(
    cancel: &Cancel,
    mut operation: impl FnMut() -> Result<(), school_day_core::lan::Error>,
) -> Result<(), school_day_core::lan::Error> {
    use school_day_core::lan::Error;
    let until = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        match operation() {
            Err(Error::ExchangePending) if std::time::Instant::now() < until => {
                std::thread::sleep(Duration::from_millis(70));
            }
            result => return result,
        }
    }
}

fn wait<T>(reply: mpsc::Receiver<T>, cancel: &Cancel) -> Option<T> {
    let until = std::time::Instant::now() + Duration::from_secs(105);
    while !cancel.is_cancelled() && std::time::Instant::now() < until {
        match reply.recv_timeout(Duration::from_millis(150)) {
            Ok(value) => return Some(value),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return None,
        }
    }
    None
}

fn show_invite(stage: &gtk::Box, code: &str) {
    let Ok(qr) = qrcode::QrCode::new(code.as_bytes()) else {
        return;
    };
    stage.append(&text(
        "Отсканируйте QR-код в School Day на телефоне. Код действует две минуты.",
        "section",
    ));
    let view = gtk::DrawingArea::builder()
        .content_width(256)
        .content_height(256)
        .halign(gtk::Align::Center)
        .build();
    view.set_draw_func(move |_, cr, width, height| {
        let modules = qr.width();
        let size = (width.min(height) / (modules as i32 + 8)).max(1);
        let offset = (width.min(height) - size * modules as i32) / 2;
        cr.set_source_rgb(1.0, 1.0, 1.0);
        let _ = cr.paint();
        cr.set_source_rgb(0.0, 0.0, 0.0);
        for row in 0..modules {
            for col in 0..modules {
                if qr[(col, row)] == qrcode::Color::Dark {
                    cr.rectangle(
                        f64::from(offset + size * col as i32),
                        f64::from(offset + size * row as i32),
                        f64::from(size),
                        f64::from(size),
                    );
                }
            }
        }
        let _ = cr.fill();
    });
    stage.append(&view);
    let manual = gtk::Expander::new(Some("Показать код для ручного ввода"));
    let field = text(code, "section");
    field.set_selectable(true);
    manual.set_child(Some(&field));
    stage.append(&manual);
}

fn show_review(
    stage: &gtk::Box,
    review: Review,
    proposed: Option<Vec<Side>>,
    answer: Sender<Option<Vec<Side>>>,
) {
    clear(stage);
    stage.append(&text(
        if proposed.is_some() {
            "Телефон предложил другой вариант"
        } else {
            "Проверьте изменения"
        },
        "settings-title",
    ));
    stage.append(&text(&format!(
        "На ПК: новых {}, изменённых {}, удалённых {}.\nНа телефон: новых {}, изменённых {}, удалённых {}.",
        review.incoming.added, review.incoming.changed, review.incoming.deleted,
        review.outgoing.added, review.outgoing.changed, review.outgoing.deleted,
    ), "section"));
    let mut choices = Vec::new();
    for (index, conflict) in review.conflicts.into_iter().enumerate() {
        let group = column(6);
        group.add_css_class("settings-link");
        let desktop = gtk::CheckButton::with_label("Оставить вариант с ПК");
        let phone = gtk::CheckButton::with_label("Оставить вариант с Android");
        phone.set_group(Some(&desktop));
        if let Some(side) = proposed.as_ref().and_then(|choices| choices.get(index)) {
            desktop.set_active(*side == Side::Local);
            phone.set_active(*side == Side::Incoming);
            desktop.set_sensitive(false);
            phone.set_sensitive(false);
        }
        group.append(&desktop);
        group.append(&text(&describe(conflict.desktop.as_ref()), "muted"));
        group.append(&phone);
        group.append(&text(&describe(conflict.android.as_ref()), "muted"));
        stage.append(&group);
        choices.push((desktop, phone));
    }
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let accept = primary(if proposed.is_some() {
        "Подтвердить"
    } else {
        "Применить"
    });
    let cancel = gtk::Button::with_label("Отмена");
    actions.append(&accept);
    actions.append(&cancel);
    stage.append(&actions);
    let pending = Rc::new(std::cell::Cell::new(Some(answer)));
    let reject = pending.clone();
    cancel.connect_clicked(move |button| {
        if let Some(answer) = reject.take() {
            let _ = answer.send(None);
        }
        button.set_sensitive(false);
    });
    let weak = stage.downgrade();
    accept.connect_clicked(move |button| {
        let picked = choices
            .iter()
            .map(|(desktop, phone)| {
                if desktop.is_active() {
                    Some(Side::Local)
                } else if phone.is_active() {
                    Some(Side::Incoming)
                } else {
                    None
                }
            })
            .collect::<Option<Vec<_>>>();
        if let Some(picked) = picked {
            if let Some(answer) = pending.take() {
                let _ = answer.send(Some(picked));
            }
            button.set_sensitive(false);
            if let Some(stage) = weak.upgrade() {
                clear(&stage);
                stage.append(&text(
                    if proposed.is_some() {
                        "Сохраняем изменения…"
                    } else {
                        "Ожидаем подтверждение на телефоне…"
                    },
                    "section",
                ));
            }
        } else if let Some(stage) = weak.upgrade() {
            stage.append(&text(
                "Выберите версию каждой конфликтующей записи.",
                "form-error",
            ));
        }
    });
}

fn describe(value: Option<&serde_json::Value>) -> String {
    let Some(value) = value else {
        return "Запись удалена".into();
    };
    let mut details = Vec::new();
    for (key, label) in [
        ("title", ""),
        ("date", ""),
        ("time", "Начало"),
        ("end", "Конец"),
        ("room", "Кабинет"),
        ("note", "Заметка"),
        ("homework", "ДЗ"),
        ("body", "Задание"),
    ] {
        if let Some(text) = value[key].as_str().filter(|text| !text.is_empty()) {
            let limited = text.chars().take(950).collect::<String>();
            details.push(if label.is_empty() {
                limited
            } else {
                format!("{label} · {limited}")
            });
        }
    }
    if value["remote"].as_bool() == Some(true) {
        details.push("Дистант".into());
    }
    if let Some(minutes) = value["short_minutes"].as_u64() {
        details.push(format!("{minutes} мин на урок"));
    }
    if value["options"]["cancelled"].as_bool() == Some(true) {
        details.push("Урок отменён".into());
    }
    if let Some(replacement) = value["options"]["replacement"]
        .as_str()
        .filter(|text| !text.is_empty())
    {
        details.push(format!("Замена · {replacement}"));
    }
    if let Some(work) = value["options"]["work"].as_str() {
        match work {
            "test" => details.push("Контрольная".into()),
            "independent" => details.push("Самостоятельная".into()),
            _ => {}
        }
    }
    for key in ["completed", "homework_completed"] {
        if let Some(done) = value[key].as_bool() {
            details.push(
                if done {
                    "ДЗ выполнено"
                } else {
                    "ДЗ к выполнению"
                }
                .into(),
            );
            break;
        }
    }
    if details.is_empty() {
        "Обычный день".into()
    } else {
        details.join("\n")
    }
}

fn answer_buttons<T: Clone + 'static>(
    parent: &gtk::Box,
    answer: Sender<T>,
    yes: T,
    no: T,
    action: &str,
) {
    let controls = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let confirm = primary(action);
    let cancel = gtk::Button::with_label("Отмена");
    controls.append(&confirm);
    controls.append(&cancel);
    parent.append(&controls);
    for (button, value) in [(confirm, yes), (cancel, no)] {
        let answer = answer.clone();
        let controls = controls.downgrade();
        button.connect_clicked(move |_| {
            if let Some(controls) = controls.upgrade() {
                controls.set_sensitive(false);
            }
            let _ = answer.send(value.clone());
        });
    }
}

fn clear(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

fn local_adapters() -> Vec<(String, Ipv4Addr)> {
    let mut choices = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|adapter| {
            let IpAddr::V4(ip) = adapter.ip() else {
                return None;
            };
            if !adapter.is_oper_up() || !(ip.is_private() || ip.is_link_local()) {
                return None;
            }
            let physical = cfg!(target_os = "linux")
                && std::path::Path::new("/sys/class/net")
                    .join(&adapter.name)
                    .join("device")
                    .exists();
            let rank = (!physical, adapter.is_p2p(), ip.is_link_local());
            Some((rank, adapter.name, ip))
        })
        .collect::<Vec<_>>();
    choices.sort_unstable();
    choices
        .into_iter()
        .map(|(_, name, ip)| (name, ip))
        .collect()
}
