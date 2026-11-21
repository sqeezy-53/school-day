use school_day_core::lessons::*;
use serde_json::{Value, json};
const OLD: &[u8] = include_bytes!("fixtures/school-day-062.json");

#[test]
fn migration_preserves_every_field_and_identity_after_reopen() {
    let (data, old) = SchoolData::decode(OLD).unwrap();
    assert!(old);
    let encode = serde_json::to_vec(&data).unwrap();
    let (again, legacy) = SchoolData::decode(&encode).unwrap();
    assert!(!legacy);
    assert_eq!(serde_json::to_vec(&again).unwrap(), encode);
    let mut modern = serde_json::to_value(&data).unwrap();
    assert_eq!(
        modern.as_object_mut().unwrap().remove("format_version"),
        Some(json!(1))
    );
    assert!(
        modern["lessons"][0]
            .as_object_mut()
            .unwrap()
            .remove("homework_id")
            .unwrap()
            .is_string()
    );
    for key in ["lessons", "pending_homework"] {
        assert!(
            modern[key][0]
                .as_object_mut()
                .unwrap()
                .remove("id")
                .unwrap()
                .is_string()
        );
    }
    assert_eq!(modern, serde_json::from_slice::<Value>(OLD).unwrap());
}

#[test]
fn rejects_duplicate_missing_ids_and_future_versions_without_mutating_source() {
    let (data, _) = SchoolData::decode(OLD).unwrap();
    let clean = serde_json::to_value(&data).unwrap();
    for case in 0..5 {
        let mut bad = clean.clone();
        match case {
            0 => bad["format_version"] = json!(2),
            1 => bad["pending_homework"][0]["id"] = bad["lessons"][0]["id"].clone(),
            2 => {
                bad["lessons"][0].as_object_mut().unwrap().remove("id");
            }
            3 => bad["days"]["2026-11-18"]["short_minutes"] = json!(0),
            _ => bad["lessons"][0]["title"] = json!("\u{0}"),
        }
        assert!(SchoolData::decode(&serde_json::to_vec(&bad).unwrap()).is_err());
    }
}

#[test]
fn delete_edit_conflict_and_history_survive_failed_writes() {
    use std::rc::Rc;
    let (base, _) = SchoolData::decode(OLD).unwrap();
    let failed = Rc::new(std::cell::Cell::new(false));
    let flag = failed.clone();
    let local = Lessons::with_writer(base.clone(), move |_| {
        if flag.get() {
            Err("Диск заполнен".into())
        } else {
            Ok(())
        }
    });
    let other = Lessons::new(base.clone());
    let id = local.borrow()[0].id;
    local.set_homework_text(id, "Изменено", false).unwrap();
    other.delete_lesson(id).unwrap();
    assert_eq!(
        conflicts(&base, &local.data(), &other.data())
            .into_iter()
            .collect::<Vec<_>>(),
        [Key::Lesson(id)]
    );
    let task = local.homework()[0].id;
    local.set_homework_completed(task, true).unwrap();
    failed.set(true);
    assert!(local.travel(false).is_err());
    assert!(local.homework()[0].completed);
    failed.set(false);
    assert!(local.travel(false).unwrap());
    assert!(!local.homework()[0].completed);
    assert!(local.travel(false).unwrap());
    assert_eq!(local.borrow()[0].homework, "Повторить § 2");
    assert!(local.travel(true).unwrap());
    assert_eq!(local.borrow()[0].homework, "Изменено");
    assert!(local.travel(true).unwrap());
    assert!(local.homework()[0].completed);
    assert!(!local.travel(true).unwrap());
}

#[test]
#[ignore = "Замер на синтетическом расписании в release"]
fn homework_time_lookup() {
    use std::{hint::black_box, time::Instant};
    let start = chrono::NaiveDate::from_ymd_opt(2020, 1, 1).unwrap();
    let rows=(0..10000).map(|i|json!({"title":"Урок","date":start+chrono::Duration::days(i),"time":"08:00:00","end":"08:40:00","homework":"ДЗ"})).collect::<Vec<_>>();
    let (data, _) = SchoolData::decode(&serde_json::to_vec(&rows).unwrap()).unwrap();
    let items = Lessons::new(data);
    let tasks = items.homework();
    let lessons = items.borrow();
    let old = Instant::now();
    for h in &tasks {
        if let HomeworkId::Linked(id) = h.id {
            black_box(lessons.iter().find(|l| l.id == id).unwrap().time);
        }
    }
    let before = old.elapsed();
    let new = Instant::now();
    for h in &tasks {
        black_box(h.lesson_time.unwrap());
    }
    eprintln!(
        "10000 связанных заданий, поиск времени: {:?} -> {:?}",
        before,
        new.elapsed()
    );
}

#[test]
fn attaching_an_independent_task_preserves_its_serialized_id_through_delete_and_restore() {
    let items = Lessons::practice(|| {});
    let HomeworkId::Pending(task) = items
        .save_pending(None, "История", None, "Тема", None)
        .unwrap()
    else {
        panic!()
    };
    let day = chrono::Local::now().date_naive().succ_opt().unwrap();
    items
        .store_lesson(
            Lesson::new(
                "История",
                day,
                "09:00:00".parse().unwrap(),
                "09:40:00".parse().unwrap(),
                "1",
            )
            .unwrap(),
            None,
        )
        .unwrap();
    let lesson = items.borrow()[0].id;
    assert_eq!(items.borrow()[0].homework_id, Some(task));
    let removed = items.delete_homework(HomeworkId::Linked(lesson)).unwrap();
    items.restore(&removed).unwrap();
    assert_eq!(items.borrow()[0].homework_id, Some(task));
    let (data, _) = SchoolData::decode(&serde_json::to_vec(&*items.data()).unwrap()).unwrap();
    assert_eq!(Lessons::new(data).borrow()[0].homework_id, Some(task));
}

#[test]
fn portable_bells_are_validated_and_undoable() {
    use school_day_core::bells::BellSchedule;
    let items = Lessons::new(SchoolData::default());
    let bells = BellSchedule {
        lesson_minutes: 45,
        ..Default::default()
    };
    items.set_bells(bells.clone()).unwrap();
    let (copy, _) = SchoolData::decode(&serde_json::to_vec(&*items.data()).unwrap()).unwrap();
    assert_eq!(copy.bells, Some(bells));
    assert!(items.travel(false).unwrap());
    assert!(items.data().bells.is_none());
    let mut bad = serde_json::to_value(copy).unwrap();
    bad["bells"]["lesson_minutes"] = json!(0);
    assert!(SchoolData::decode(&serde_json::to_vec(&bad).unwrap()).is_err());
}

#[test]
fn clearing_completed_homework_is_atomic_and_undoable() {
    let fixture = json!({
        "lessons": [
            {"title":"Алгебра","date":"2026-11-18","time":"08:00:00","end":"08:40:00","room":"12","note":"Принести тетрадь","homework":"Упр. 1","homework_completed":true,"options":{"work":"test"}},
            {"title":"История","date":"2026-11-18","time":"09:00:00","end":"09:40:00","homework":"§ 7"}
        ],
        "pending_homework": [
            {"title":"Музыка","body":"Ноты","completed":true},
            {"title":"Физика","body":"Параграф","completed":false}
        ]
    });
    let (data, _) = SchoolData::decode(&serde_json::to_vec(&fixture).unwrap()).unwrap();
    let original = serde_json::to_value(&data).unwrap();
    let failed = std::rc::Rc::new(std::cell::Cell::new(true));
    let fail = failed.clone();
    let saves = std::rc::Rc::new(std::cell::Cell::new(0));
    let writes = saves.clone();
    let items = Lessons::with_writer(data, move |_| {
        if fail.get() {
            return Err("Нет места для записи".into());
        }
        writes.set(writes.get() + 1);
        Ok(())
    });
    assert_eq!(items.completed_homework_count(), 2);
    assert!(items.clear_completed_homework(1).is_err());
    assert!(items.clear_completed_homework(2).is_err());
    assert_eq!(items.revision(), 0);
    assert_eq!(serde_json::to_value(&*items.data()).unwrap(), original);
    assert!(!items.travel(false).unwrap());

    failed.set(false);
    items.clear_completed_homework(2).unwrap();
    assert_eq!(saves.get(), 1);
    assert_eq!(items.completed_homework_count(), 0);
    assert_eq!(items.homework().len(), 2);
    assert_eq!(items.borrow().len(), 2);
    let mut expected = original["lessons"][0].clone();
    expected.as_object_mut().unwrap().remove("homework_id");
    expected["homework"] = json!("");
    expected["homework_completed"] = json!(false);
    assert_eq!(serde_json::to_value(&items.borrow()[0]).unwrap(), expected);
    assert_eq!(
        serde_json::to_value(&items.borrow()[1]).unwrap(),
        original["lessons"][1]
    );
    items.clear_completed_homework(0).unwrap();
    assert_eq!(saves.get(), 1);

    assert!(items.travel(false).unwrap());
    assert_eq!(serde_json::to_value(&*items.data()).unwrap(), original);
    assert!(items.travel(true).unwrap());
    assert_eq!(items.homework().len(), 2);
}
