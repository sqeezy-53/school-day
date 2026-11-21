use jni::JNIEnv;
use jni::objects::{JObject, JString};
use jni::sys::{jboolean, jlong, jstring};
use school_day_core::lan::{self, Cancel, Invitation, Outcome, Profile, SessionMode};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Mutex, OnceLock};

fn operations() -> &'static Mutex<HashMap<jlong, Cancel>> {
    static OPERATIONS: OnceLock<Mutex<HashMap<jlong, Cancel>>> = OnceLock::new();
    OPERATIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_sqeezy53_schoolday_Core_lanInviteValid(
    mut env: JNIEnv,
    _: JObject,
    code: JString,
) -> jboolean {
    let Some(code): Option<String> = env.get_string(&code).ok().map(Into::into) else {
        return 0;
    };
    u8::from(Invitation::decode(&code).is_ok())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_sqeezy53_schoolday_Core_lanBegin(
    _: JNIEnv,
    _: JObject,
) -> jlong {
    static NEXT: AtomicI64 = AtomicI64::new(1);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut map) = operations().lock() {
        map.insert(id, Cancel::default());
        id
    } else {
        0
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_sqeezy53_schoolday_Core_lanCancel(
    _: JNIEnv,
    _: JObject,
    id: jlong,
) {
    if let Ok(map) = operations().lock()
        && let Some(cancel) = map.get(&id)
    {
        cancel.stop();
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_sqeezy53_schoolday_Core_lanEnd(
    _: JNIEnv,
    _: JObject,
    id: jlong,
) {
    if let Ok(mut map) = operations().lock() {
        map.remove(&id);
    }
}

fn ask(env: &mut JNIEnv<'_>, observer: &JObject<'_>, event: Value) -> Option<String> {
    let event = env.new_string(event.to_string()).ok()?;
    let answer = env
        .call_method(
            observer,
            "respond",
            "(Ljava/lang/String;)Ljava/lang/String;",
            &[(&event).into()],
        )
        .and_then(|value| value.l());
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
        return None;
    }
    let answer = answer.ok()?;
    if answer.is_null() {
        return None;
    }
    Some(env.get_string(&JString::from(answer)).ok()?.into())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_sqeezy53_schoolday_Core_lanCommand(
    mut env: JNIEnv,
    _: JObject,
    directory: JString,
    action: JString,
    payload: JString,
    operation: jlong,
    observer: JObject,
) -> jstring {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let dir: String = env
            .get_string(&directory)
            .map_err(|_| "Папка данных недоступна")?
            .into();
        let action: String = env
            .get_string(&action)
            .map_err(|_| "Неизвестное действие")?
            .into();
        let directory = Path::new(&dir);

        // Normal offline CRUD does not create a key or open a socket.
        match action.as_str() {
            "recover" => {
                lan::recover(directory).map_err(|err| err.to_string())?;
                return Ok(json!({}));
            }
            "guard" => {
                lan::ensure_writable(directory).map_err(|err| err.to_string())?;
                return Ok(json!({}));
            }
            _ => {}
        }

        let profile = Profile::open(directory).map_err(|err| err.to_string())?;
        let mut status = None;
        let mut result = None;
        match action.as_str() {
            "info" => {}
            "address" => {
                let value: String = env
                    .get_string(&payload)
                    .map_err(|_| "Укажите адрес ПК")?
                    .into();
                let address = value
                    .trim()
                    .parse()
                    .map_err(|_| "Укажите локальный IPv4-адрес компьютера")?;
                profile
                    .change_address(address)
                    .map_err(|err| err.to_string())?;
                status = Some("Адрес компьютера обновлён");
            }
            "revoke" => {
                profile.revoke().map_err(|err| err.to_string())?;
                status = Some("Устройство отключено");
            }
            "pair" | "sync" | "auto" => {
                let cancel = operations()
                    .lock()
                    .ok()
                    .and_then(|map| map.get(&operation).cloned())
                    .ok_or("Обмен отменён")?;
                if action == "pair" {
                    let code: String = env.get_string(&payload).map_err(|_| "Неверный код")?.into();
                    let invite = Invitation::decode(&code).map_err(|err| err.to_string())?;
                    profile
                        .connect_pair(invite, cancel, |code| {
                            ask(&mut env, &observer, json!({"type":"pair","code":code})).as_deref()
                                == Some("true")
                        })
                        .map_err(|err| err.to_string())?;
                    status = Some("Компьютер подключён");
                } else {
                    let mode = if action == "auto" {
                        SessionMode::Automatic
                    } else {
                        SessionMode::Interactive
                    };
                    let outcome = profile
                        .join_in(cancel, mode, |review, choices| {
                            let answer = ask(
                                &mut env,
                                &observer,
                                json!({"type":"review","review":review,"choices":choices}),
                            )?;
                            serde_json::from_str(&answer).ok()
                        })
                        .map_err(|err| err.to_string())?;
                    let (kind, message) = match outcome {
                        Outcome::Synchronized | Outcome::Resumed => ("applied", "Синхронизировано"),
                        Outcome::Unchanged => ("unchanged", "Все изменения синхронизированы"),
                        Outcome::ReviewRequired => {
                            ("review", "Требуется ручная проверка изменений")
                        }
                        Outcome::AwaitingReceipt => {
                            ("receipt", "Изменения сохранены. Ожидается подтверждение")
                        }
                        Outcome::Cancelled => ("cancelled", "Обмен отменён"),
                    };
                    result = Some(kind);
                    status = Some(message);
                }
            }
            _ => return Err("Неизвестное действие".into()),
        }
        Ok::<_, String>(json!({
            "paired": profile.peer().map_err(|err| err.to_string())?.is_some(),
            "last_sync": profile.last_sync().map_err(|err| err.to_string())?,
            "queued": if result == Some("unchanged") { false }
                else { profile.needs_sync().map_err(|err| err.to_string())? },
            "result": result,
            "message": status,
        }))
    }));
    let response = match result {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => json!({"error":error}),
        Err(_) => json!({"error":"Обмен не завершён"}),
    };
    env.new_string(response.to_string())
        .map_or(std::ptr::null_mut(), |text| text.into_raw())
}
