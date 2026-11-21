mod lan;

use chrono::NaiveDate;
use jni::JNIEnv;
use jni::objects::{JByteArray, JObject};
use jni::sys::jbyteArray;
use school_day_core::lessons::{
    DayOptions, HomeworkId, Lesson, Lessons, MAX_BYTES, RecordId, SchoolData,
};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Command {
    Open,
    SaveLesson {
        lesson: Lesson,
        id: Option<RecordId>,
    },
    DeleteLesson {
        id: RecordId,
    },
    Day {
        date: NaiveDate,
        options: DayOptions,
    },
    SaveTask {
        id: Option<RecordId>,
        title: String,
        date: Option<NaiveDate>,
        body: String,
        target: Option<RecordId>,
    },
    LinkedText {
        id: RecordId,
        body: String,
    },
    Complete {
        id: RecordId,
        linked: bool,
        done: bool,
    },
    ClearCompleted {
        expected: usize,
    },
    SetBells {
        bells: school_day_core::bells::BellSchedule,
    },
    DeleteTask {
        id: RecordId,
        linked: bool,
    },
}
fn task_id(id: RecordId, linked: bool) -> HomeworkId {
    if linked {
        HomeworkId::Linked(id)
    } else {
        HomeworkId::Pending(id)
    }
}

fn execute(document: &[u8], command: &[u8]) -> Result<Value, String> {
    if command.len() > MAX_BYTES {
        return Err("Слишком большой запрос".into());
    }
    let (data, migrated) = SchoolData::decode(document)?;
    let lessons = Lessons::new(data);
    match serde_json::from_slice(command).map_err(|e| e.to_string())? {
        Command::Open => {}
        Command::SaveLesson { mut lesson, id } => {
            if let Some(id) = id {
                let items = lessons.borrow();
                let old = items.iter().find(|l| l.id == id).ok_or("Урок удалён")?;
                lesson.homework.clone_from(&old.homework);
                lesson.homework_completed = old.homework_completed;
            }
            lessons.store_lesson(lesson, id)?;
        }
        Command::DeleteLesson { id } => {
            lessons.delete_lesson(id)?;
        }
        Command::Day { date, options } => lessons.set_day_options(date, options)?,
        Command::SaveTask {
            id,
            title,
            date,
            body,
            target,
        } => {
            lessons.save_pending(id, &title, date, &body, target)?;
        }
        Command::LinkedText { id, body } => lessons.set_homework_text(id, &body, false)?,
        Command::Complete { id, linked, done } => {
            lessons.set_homework_completed(task_id(id, linked), done)?
        }
        Command::ClearCompleted { expected } => lessons.clear_completed_homework(expected)?,
        Command::SetBells { bells } => lessons.set_bells(bells)?,
        Command::DeleteTask { id, linked } => {
            lessons.delete_homework(task_id(id, linked))?;
        }
    }
    let tasks:Vec<_> = lessons.homework().iter().map(|h| {
        let (id,linked) = match h.id {HomeworkId::Linked(id)=>(id,true),HomeworkId::Pending(id)=>(id,false)};
        json!({"id":id,"linked":linked,"title":h.title,"body":h.body,"date":h.date,"completed":h.completed})
    }).collect();
    let data = lessons.data();
    let encoded = serde_json::to_vec(&*data).map_err(|e| e.to_string())?;
    if encoded.len() > MAX_BYTES {
        return Err("Расписание превышает 32 МиБ".into());
    }
    let bells = data.bells.clone().unwrap_or_default();
    let slots = bells
        .slots()?
        .into_iter()
        .map(|b| json!({"time":b.start,"end":b.end}))
        .collect::<Vec<_>>();
    let short_ends: std::collections::BTreeMap<_, _> = lessons
        .borrow()
        .iter()
        .filter_map(|lesson| {
            let end = lessons.day_options(lesson.date).end_time(lesson);
            (end != lesson.end).then_some((lesson.id, end))
        })
        .collect();
    Ok(
        json!({"document":*data,"tasks":tasks,"migrated":migrated,"bells":slots,"bells_config":bells,"short_ends":short_ends}),
    )
}

fn bounded_array(env: &JNIEnv<'_>, array: JByteArray<'_>) -> Result<Vec<u8>, &'static str> {
    let size = env
        .get_array_length(&array)
        .map_err(|_| "Ошибка чтения данных")?;
    if size as usize > MAX_BYTES {
        return Err("Файл или запрос превышает 32 МиБ");
    }
    env.convert_byte_array(array)
        .map_err(|_| "Ошибка чтения данных")
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_sqeezy53_schoolday_Core_execute(
    env: JNIEnv,
    _this: JObject,
    document: JByteArray,
    command: JByteArray,
) -> jbyteArray {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let bytes = bounded_array(&env, document)?;
        let command = bounded_array(&env, command)?;
        execute(&bytes, &command)
    }));
    let reply = match result {
        Ok(Ok(data)) => data,
        Ok(Err(error)) => json!({"error":error}),
        Err(_) => json!({"error":"Ошибка обработки данных"}),
    };
    env.byte_array_from_slice(reply.to_string().as_bytes())
        .map_or(std::ptr::null_mut(), |v| v.into_raw())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mobile_crud_survives_reopen() {
        let lesson = json!({"title":"Физика","date":"2027-01-10","time":"08:00:00","end":"08:40:00","homework":"Тест"});
        let get = |doc: &[u8], cmd: Value| execute(doc, cmd.to_string().as_bytes()).unwrap();
        let first = get(b"[]", json!({"op":"save_lesson","lesson":lesson}));
        let bytes = first["document"].to_string();
        let id = &first["tasks"][0]["id"];
        let second = get(
            bytes.as_bytes(),
            json!({"op":"complete","id":id,"linked":true,"done":true}),
        );
        let open = get(
            second["document"].to_string().as_bytes(),
            json!({"op":"open"}),
        );
        assert_eq!(open["tasks"][0]["completed"], true);
        assert_eq!(open["tasks"][0]["id"], *id);
        assert!(execute(bytes.as_bytes(), br#"{"op":"unknown"}"#).is_err());
    }
}
