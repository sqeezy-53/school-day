use chrono::{NaiveDate, NaiveTime};
use std::cell::{Cell, Ref, RefCell};
use std::rc::Rc;

use crate::forms::{HOMEWORK_LIMIT, SUBJECT_LIMIT, length_error};

pub type RecordId = u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HomeworkId {
    Linked(RecordId),
    Pending(RecordId),
}

pub struct Homework {
    pub id: HomeworkId,
    pub title: String,
    pub date: Option<NaiveDate>,
    pub body: String,
    pub completed: bool,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct PendingHomework {
    #[serde(skip)]
    id: RecordId,
    title: String,
    date: Option<NaiveDate>,
    body: String,
    #[serde(default)]
    completed: bool,
}

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct SchoolData {
    lessons: Vec<Lesson>,
    pending_homework: Vec<PendingHomework>,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
enum SavedData {
    Legacy(Vec<Lesson>),
    Current(SchoolData),
}

pub struct Removed(RemovedItem);

enum RemovedItem {
    Lesson(Lesson),
    LinkedHomework {
        lesson: RecordId,
        body: String,
        completed: bool,
    },
    PendingHomework {
        index: usize,
        task: PendingHomework,
    },
}

impl Removed {
    pub fn lesson_slot(&self) -> Option<(NaiveDate, NaiveTime)> {
        match &self.0 {
            RemovedItem::Lesson(lesson) => Some((lesson.date, lesson.time)),
            _ => None,
        }
    }

    pub fn homework_id(&self) -> Option<HomeworkId> {
        match &self.0 {
            RemovedItem::Lesson(lesson) if !lesson.homework.trim().is_empty() => {
                Some(HomeworkId::Linked(lesson.id))
            }
            RemovedItem::LinkedHomework { lesson, .. } => Some(HomeworkId::Linked(*lesson)),
            RemovedItem::PendingHomework { task, .. } => Some(HomeworkId::Pending(task.id)),
            _ => None,
        }
    }
}

type RestoreCallback = Box<dyn Fn(&Lessons, &Removed)>;

#[derive(Clone)]
pub struct Lessons(Rc<LessonList>);

struct LessonList {
    data: RefCell<SchoolData>,
    next_id: Cell<RecordId>,
    practice_changed: Option<Box<dyn Fn()>>,
    restored: RefCell<Vec<RestoreCallback>>,
}

impl Lessons {
    pub fn new(mut data: SchoolData) -> Self {
        let mut next_id = 1..;
        for lesson in &mut data.lessons {
            lesson.id = next_id.next().unwrap();
        }
        for task in &mut data.pending_homework {
            task.id = next_id.next().unwrap();
        }
        Self(Rc::new(LessonList {
            data: RefCell::new(data),
            next_id: Cell::new(next_id.next().unwrap()),
            practice_changed: None,
            restored: RefCell::new(Vec::new()),
        }))
    }

    pub fn practice(on_change: impl Fn() + 'static) -> Self {
        Self(Rc::new(LessonList {
            data: RefCell::new(SchoolData::default()),
            next_id: Cell::new(1),
            practice_changed: Some(Box::new(on_change)),
            restored: RefCell::new(Vec::new()),
        }))
    }

    pub fn borrow(&self) -> Ref<'_, Vec<Lesson>> {
        Ref::map(self.0.data.borrow(), |data| &data.lessons)
    }

    pub fn is_practice(&self) -> bool {
        self.0.practice_changed.is_some()
    }

    fn next_id(&self) -> RecordId {
        let id = self.0.next_id.get();
        self.0.next_id.set(id + 1);
        id
    }

    fn replace(&self, updated: SchoolData) -> Result<(), String> {
        if !self.is_practice() {
            if updated.pending_homework.is_empty() {
                crate::storage::save_json("lessons.json", &updated.lessons)?;
            } else {
                crate::storage::save_json("lessons.json", &updated)?;
            }
        }
        *self.0.data.borrow_mut() = updated;
        if let Some(on_change) = &self.0.practice_changed {
            on_change();
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Lesson {
    #[serde(skip)]
    pub id: RecordId,
    pub title: String,
    pub date: NaiveDate,
    pub time: NaiveTime,
    pub end: NaiveTime,
    pub room: String,
    #[serde(default)]
    pub homework: String,

    #[serde(default)]
    pub homework_completed: bool,
}

pub fn parse_lesson(
    title: &str,
    date: &str,
    time: &str,
    end: &str,
    room: &str,
) -> Result<Lesson, String> {
    let title = title.trim();
    let room = room.trim();
    if title.is_empty() {
        return Err("Введите название предмета".to_string());
    }
    if room.is_empty() {
        return Err("Введите кабинет".to_string());
    }
    let date = NaiveDate::parse_from_str(date, "%d.%m.%Y")
        .map_err(|_| "Выберите дату урока".to_string())?;
    let time = NaiveTime::parse_from_str(time, "%H:%M")
        .map_err(|_| "Проверьте время начала".to_string())?;
    let end = NaiveTime::parse_from_str(end, "%H:%M")
        .map_err(|_| "Проверьте время окончания".to_string())?;
    let lesson = Lesson {
        id: 0,
        title: title.to_string(),
        date,
        time,
        end,
        room: room.to_string(),
        homework: String::new(),
        homework_completed: false,
    };

    validate_lesson(&lesson)?;
    Ok(lesson)
}

fn validate_lesson(lesson: &Lesson) -> Result<(), String> {
    if [&lesson.title, &lesson.room, &lesson.homework]
        .iter()
        .any(|text| text.contains('\0'))
    {
        return Err("В данных урока есть недопустимый нулевой символ".to_string());
    }
    if lesson.title.trim().is_empty() || lesson.room.trim().is_empty() {
        return Err("У урока должны быть предмет и кабинет".to_string());
    }
    if lesson.end <= lesson.time {
        return Err("Конец урока должен быть позже начала в тот же день".to_string());
    }
    Ok(())
}

fn check_slot(items: &[Lesson], lesson: &Lesson) -> Result<(), String> {
    for other in items {
        if other.id == lesson.id || other.date != lesson.date {
            continue;
        }
        if other.time < lesson.end && lesson.time < other.end {
            return Err(slot_conflict(other));
        }
    }
    Ok(())
}

fn slot_conflict(lesson: &Lesson) -> String {
    format!(
        "В это время уже есть {} ({}–{})",
        lesson.title,
        lesson.time.format("%H:%M"),
        lesson.end.format("%H:%M"),
    )
}

pub fn load_lessons() -> Result<SchoolData, String> {
    let saved = crate::storage::load_json("lessons.json", validate_saved)?;
    Ok(match saved {
        Some(SavedData::Legacy(lessons)) => SchoolData {
            lessons,
            ..SchoolData::default()
        },
        Some(SavedData::Current(data)) => data,
        None => SchoolData::default(),
    })
}

fn validate_saved(saved: &mut SavedData) -> Result<(), String> {
    let lessons = match saved {
        SavedData::Legacy(lessons) => lessons,
        SavedData::Current(data) => {
            for task in &data.pending_homework {
                validate_pending(&task.title, &task.body)?;
            }
            &mut data.lessons
        }
    };
    lessons.sort_by_key(|lesson| (lesson.date, lesson.time));
    validate_sorted_lessons(lessons)
}

fn validate_pending(title: &str, body: &str) -> Result<(), String> {
    if title.trim().is_empty() || body.trim().is_empty() {
        return Err("У задания должны быть предмет и текст".into());
    }
    if title.contains('\0') || body.contains('\0') {
        return Err("В данных задания есть недопустимый нулевой символ".into());
    }
    Ok(())
}

pub fn same_subject(left: &str, right: &str) -> bool {
    left.split_whitespace()
        .map(str::to_lowercase)
        .eq(right.split_whitespace().map(str::to_lowercase))
}

fn validate_sorted_lessons(items: &[Lesson]) -> Result<(), String> {
    for lesson in items {
        validate_lesson(lesson)?;
    }
    for pair in items.windows(2) {
        if pair[0].date == pair[1].date && pair[0].end > pair[1].time {
            return Err(slot_conflict(&pair[0]));
        }
    }
    Ok(())
}

impl SchoolData {
    fn lesson_mut(&mut self, id: RecordId) -> Result<&mut Lesson, String> {
        self.lessons
            .iter_mut()
            .find(|lesson| lesson.id == id)
            .ok_or_else(|| "Урок не найден".into())
    }

    fn pending_index(&self, id: RecordId) -> Result<usize, String> {
        self.pending_homework
            .iter()
            .position(|task| task.id == id)
            .ok_or_else(|| "Задание не найдено".into())
    }

    fn attach(&mut self, index: usize, lesson_id: RecordId) -> Result<HomeworkId, String> {
        let lesson = self
            .lessons
            .iter_mut()
            .find(|lesson| lesson.id == lesson_id)
            .ok_or("Урок не найден")?;
        if !lesson.homework.trim().is_empty() {
            return Err("У этого урока уже есть домашнее задание".into());
        }
        let task = self.pending_homework.remove(index);
        lesson.homework = task.body;
        lesson.homework_completed = task.completed;
        Ok(HomeworkId::Linked(lesson_id))
    }

    fn auto_attach(&mut self, title: &str, date: NaiveDate) -> Option<HomeworkId> {
        let mut lessons = self
            .lessons
            .iter()
            .filter(|lesson| lesson.date == date && same_subject(&lesson.title, title));
        let lesson = lessons.next()?;
        if lessons.next().is_some() || !lesson.homework.trim().is_empty() {
            return None;
        }
        let id = lesson.id;
        let mut tasks = self
            .pending_homework
            .iter()
            .enumerate()
            .filter(|(_, task)| task.date == Some(date) && same_subject(&task.title, title));
        let (index, _) = tasks.next()?;
        if tasks.next().is_some() {
            return None;
        }
        self.attach(index, id).ok()
    }
}

impl Lessons {
    pub fn store_lesson(&self, mut lesson: Lesson, id: Option<RecordId>) -> Result<bool, String> {
        lesson.id = id.unwrap_or_else(|| self.next_id());
        validate_lesson(&lesson)?;
        let mut data = self.0.data.borrow().clone();
        check_slot(&data.lessons, &lesson)?;
        let (title, date) = (lesson.title.clone(), lesson.date);
        if let Some(id) = id {
            *data.lesson_mut(id)? = lesson;
        } else {
            data.lessons.push(lesson);
        }
        data.lessons
            .sort_by_key(|lesson| (lesson.date, lesson.time));
        let attached = data.auto_attach(&title, date).is_some();
        self.replace(data)?;
        Ok(attached)
    }

    pub fn delete_lesson(&self, id: RecordId) -> Result<Removed, String> {
        let mut data = self.0.data.borrow().clone();
        let index = data
            .lessons
            .iter()
            .position(|lesson| lesson.id == id)
            .ok_or("Урок уже удалён")?;
        let removed = RemovedItem::Lesson(data.lessons.remove(index));
        self.replace(data)?;
        Ok(Removed(removed))
    }

    pub fn connect_restored(&self, callback: impl Fn(&Self, &Removed) + 'static) {
        self.0.restored.borrow_mut().push(Box::new(callback));
    }

    pub fn restore(&self, removed: &Removed) -> Result<(), String> {
        let mut data = self.0.data.borrow().clone();
        match &removed.0 {
            RemovedItem::Lesson(lesson) => {
                if data.lessons.iter().any(|item| item.id == lesson.id) {
                    return Err("Урок уже восстановлен".into());
                }
                check_slot(&data.lessons, lesson)?;
                data.lessons.push(lesson.clone());
                data.lessons
                    .sort_by_key(|lesson| (lesson.date, lesson.time));
            }
            RemovedItem::LinkedHomework {
                lesson,
                body,
                completed,
            } => {
                let lesson = data
                    .lesson_mut(*lesson)
                    .map_err(|_| "Урок удалён. Сначала верните урок")?;
                if !lesson.homework.trim().is_empty() {
                    return Err("У этого урока уже есть домашнее задание".into());
                }
                lesson.homework.clone_from(body);
                lesson.homework_completed = *completed;
            }
            RemovedItem::PendingHomework { index, task } => {
                if data.pending_homework.iter().any(|item| item.id == task.id) {
                    return Err("Задание уже восстановлено".into());
                }
                data.pending_homework
                    .insert((*index).min(data.pending_homework.len()), task.clone());
            }
        }
        self.replace(data)?;
        for callback in self.0.restored.borrow().iter() {
            callback(self, removed);
        }
        Ok(())
    }

    pub fn homework(&self) -> Vec<Homework> {
        let data = self.0.data.borrow();
        let mut tasks: Vec<_> = data
            .lessons
            .iter()
            .filter(|lesson| !lesson.homework.trim().is_empty())
            .map(|lesson| Homework {
                id: HomeworkId::Linked(lesson.id),
                title: lesson.title.clone(),
                date: Some(lesson.date),
                body: lesson.homework.clone(),
                completed: lesson.homework_completed,
            })
            .chain(data.pending_homework.iter().map(|task| Homework {
                id: HomeworkId::Pending(task.id),
                title: task.title.clone(),
                date: task.date,
                body: task.body.clone(),
                completed: task.completed,
            }))
            .collect();
        tasks.sort_by_key(|task| (task.date.is_none(), task.date));
        tasks
    }

    pub fn set_homework_completed(&self, id: HomeworkId, completed: bool) -> Result<(), String> {
        let mut data = self.0.data.borrow().clone();
        match id {
            HomeworkId::Linked(id) => {
                let lesson = data.lesson_mut(id)?;
                if lesson.homework.trim().is_empty() {
                    return Err("У этого урока нет домашнего задания".into());
                }
                lesson.homework_completed = completed;
            }
            HomeworkId::Pending(id) => {
                let index = data.pending_index(id)?;
                data.pending_homework[index].completed = completed;
            }
        }
        self.replace(data)
    }

    pub fn set_homework_text(&self, id: RecordId, value: &str, adding: bool) -> Result<(), String> {
        if value.trim().is_empty() {
            return Err("Напишите домашнее задание".into());
        }
        let mut data = self.0.data.borrow().clone();
        let lesson = data.lesson_mut(id)?;
        if adding && !lesson.homework.trim().is_empty() {
            return Err("У этого урока уже есть домашнее задание".into());
        }
        if lesson.homework == value {
            return Ok(());
        }
        let value = value.trim();
        if let Some(error) = length_error(value, lesson.homework.trim(), HOMEWORK_LIMIT) {
            return Err(error);
        }
        if lesson.homework != value {
            lesson.homework_completed = false;
        }
        lesson.homework = value.to_string();
        validate_lesson(lesson)?;
        self.replace(data)
    }

    pub fn save_pending(
        &self,
        id: Option<RecordId>,
        title: &str,
        date: Option<NaiveDate>,
        body: &str,
        target: Option<RecordId>,
    ) -> Result<HomeworkId, String> {
        validate_pending(title, body)?;
        let mut data = self.0.data.borrow().clone();
        let index = match id {
            Some(id) => data.pending_index(id)?,
            None => data.pending_homework.len(),
        };
        let previous = data.pending_homework.get(index);
        let title = if previous.is_some_and(|task| task.title == title) {
            title
        } else {
            title.trim()
        };
        let body = if previous.is_some_and(|task| task.body == body) {
            body
        } else {
            body.trim()
        };
        for (value, previous, limit) in [
            (
                title.trim(),
                previous.map_or("", |task| task.title.trim()),
                SUBJECT_LIMIT,
            ),
            (
                body.trim(),
                previous.map_or("", |task| task.body.trim()),
                HOMEWORK_LIMIT,
            ),
        ] {
            if let Some(error) = length_error(value, previous, limit) {
                return Err(error);
            }
        }
        let task = PendingHomework {
            id: id.unwrap_or_else(|| self.next_id()),
            title: title.to_string(),
            date,
            body: body.to_string(),
            completed: previous.is_some_and(|task| task.completed && task.body == body),
        };
        let pending = HomeworkId::Pending(task.id);
        if previous.is_some() {
            data.pending_homework[index] = task;
        } else {
            data.pending_homework.push(task);
        }
        let saved = if let Some(target) = target {
            if !same_subject(&data.lesson_mut(target)?.title, title) {
                return Err("Выберите урок по этому предмету".into());
            }
            data.attach(index, target)?
        } else {
            date.and_then(|date| data.auto_attach(title, date))
                .unwrap_or(pending)
        };
        self.replace(data)?;
        Ok(saved)
    }

    pub fn delete_homework(&self, id: HomeworkId) -> Result<Removed, String> {
        let mut data = self.0.data.borrow().clone();
        let removed = match id {
            HomeworkId::Linked(id) => {
                let lesson = data.lesson_mut(id)?;
                if lesson.homework.trim().is_empty() {
                    return Err("Задание уже удалено".into());
                }
                RemovedItem::LinkedHomework {
                    lesson: id,
                    body: std::mem::take(&mut lesson.homework),
                    completed: std::mem::take(&mut lesson.homework_completed),
                }
            }
            HomeworkId::Pending(id) => {
                let index = data.pending_index(id)?;
                RemovedItem::PendingHomework {
                    index,
                    task: data.pending_homework.remove(index),
                }
            }
        };
        self.replace(data)?;
        Ok(Removed(removed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorted_validation_allows_adjacent_lessons_and_rejects_overlap() {
        let first = parse_lesson("Математика", "06.11.2026", "08:00", "08:40", "23").unwrap();
        let adjacent = parse_lesson("Физика", "06.11.2026", "08:40", "09:20", "29").unwrap();
        let next_day = parse_lesson("Химия", "07.11.2026", "08:10", "08:50", "30").unwrap();
        assert!(validate_sorted_lessons(&[first.clone(), adjacent.clone(), next_day]).is_ok());
        let overlapping = Lesson {
            time: NaiveTime::from_hms_opt(8, 39, 0).unwrap(),
            ..adjacent
        };
        assert!(validate_sorted_lessons(&[first, overlapping]).is_err());
    }

    #[test]
    fn dated_homework_attaches_when_the_lesson_appears_and_keeps_completion() {
        let items = Lessons::practice(|| {});
        let lesson =
            parse_lesson("Физика  и астрономия", "06.11.2026", "08:00", "08:40", "23").unwrap();
        let task = items
            .save_pending(
                None,
                "ФИЗИКА и астрономия",
                Some(lesson.date),
                "Задача 4",
                None,
            )
            .unwrap();
        items.set_homework_completed(task, true).unwrap();
        let different_day = Lesson {
            date: lesson.date.succ_opt().unwrap(),
            ..lesson.clone()
        };
        assert!(!items.store_lesson(different_day, None).unwrap());
        assert_eq!(items.homework()[0].id, task);
        assert!(items.store_lesson(lesson, None).unwrap());
        let tasks = items.homework();
        assert_eq!(tasks.len(), 1);
        assert!(matches!(tasks[0].id, HomeworkId::Linked(_)));
        assert!(tasks[0].completed);
        assert_eq!(tasks[0].body, "Задача 4");
    }

    #[test]
    fn ambiguous_or_occupied_lessons_leave_homework_pending_until_manual_selection() {
        let items = Lessons::practice(|| {});
        let first = parse_lesson("Физика", "06.11.2026", "08:00", "08:40", "23").unwrap();
        let second = parse_lesson("Физика", "06.11.2026", "08:40", "09:20", "23").unwrap();
        items.store_lesson(first.clone(), None).unwrap();
        items.store_lesson(second, None).unwrap();
        let (first_id, second_id) = {
            let lessons = items.borrow();
            (lessons[0].id, lessons[1].id)
        };
        items
            .set_homework_text(first_id, "Старое ДЗ", true)
            .unwrap();
        let HomeworkId::Pending(id) = items
            .save_pending(None, "Физика", Some(first.date), "Новое ДЗ", None)
            .unwrap()
        else {
            panic!("ДЗ должно остаться без урока")
        };
        items
            .set_homework_completed(HomeworkId::Pending(id), true)
            .unwrap();
        assert!(
            items
                .save_pending(
                    Some(id),
                    "Физика",
                    Some(first.date),
                    "Новое ДЗ",
                    Some(first_id)
                )
                .is_err()
        );
        assert_eq!(items.homework().len(), 2);
        assert_eq!(
            items
                .save_pending(
                    Some(id),
                    "Физика",
                    Some(first.date),
                    "Новое ДЗ",
                    Some(second_id)
                )
                .unwrap(),
            HomeworkId::Linked(second_id)
        );
        let lessons = items.borrow();
        assert_eq!(lessons[0].homework, "Старое ДЗ");
        assert_eq!(lessons[1].homework, "Новое ДЗ");
        assert!(lessons[1].homework_completed);
    }

    #[test]
    fn undated_and_duplicate_homework_are_never_attached_automatically() {
        let items = Lessons::practice(|| {});
        let lesson = parse_lesson("Физика", "06.11.2026", "08:00", "08:40", "23").unwrap();
        for (date, body) in [
            (None, "Без даты"),
            (Some(lesson.date), "Первое"),
            (Some(lesson.date), "Второе"),
        ] {
            assert!(matches!(
                items
                    .save_pending(None, "Физика", date, body, None)
                    .unwrap(),
                HomeworkId::Pending(_)
            ));
        }
        assert!(!items.store_lesson(lesson, None).unwrap());
        assert_eq!(items.homework().len(), 3);
        assert!(items.borrow()[0].homework.is_empty());
    }

    #[test]
    fn both_saved_formats_keep_linked_homework_and_completion() {
        let legacy = r#"[{"title":"Физика","date":"2026-11-06","time":"08:00:00","end":"08:40:00","room":"23","homework":"Старое ДЗ","homework_completed":true}]"#;
        let mut saved = serde_json::from_str::<SavedData>(legacy).unwrap();
        validate_saved(&mut saved).unwrap();
        let SavedData::Legacy(lessons) = saved else {
            panic!("Прочитан другой формат")
        };
        let data = SchoolData {
            lessons,
            pending_homework: vec![PendingHomework {
                id: 0,
                title: "География".into(),
                date: None,
                body: "Новое ДЗ".into(),
                completed: true,
            }],
        };
        let json = serde_json::to_string(&data).unwrap();
        let mut saved = serde_json::from_str::<SavedData>(&json).unwrap();
        validate_saved(&mut saved).unwrap();
        let SavedData::Current(data) = saved else {
            panic!("Прочитан другой формат")
        };
        let items = Lessons::new(data);
        let tasks = items.homework();
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].body, "Старое ДЗ");
        assert!(tasks[0].completed);
        assert_eq!(tasks[1].body, "Новое ДЗ");
        assert!(tasks[1].completed);
    }

    #[test]
    fn undo_lesson_keeps_completion_and_rejects_an_occupied_slot() {
        let items = Lessons::practice(|| {});
        let mut lesson = parse_lesson("Физика", "06.11.2026", "08:00", "08:40", "23").unwrap();
        lesson.homework = "Выполнено".into();
        lesson.homework_completed = true;
        items.store_lesson(lesson.clone(), None).unwrap();
        let id = items.borrow()[0].id;
        let removed = items.delete_lesson(id).unwrap();
        assert!(items.homework().is_empty());

        let replacement = Lesson {
            title: "География".into(),
            homework: String::new(),
            homework_completed: false,
            ..lesson.clone()
        };
        items.store_lesson(replacement, None).unwrap();
        assert!(items.restore(&removed).is_err());
        assert_eq!(items.borrow()[0].title, "География");
        let replacement_id = items.borrow()[0].id;
        items.delete_lesson(replacement_id).unwrap();

        items.restore(&removed).unwrap();
        assert_eq!(items.borrow()[0].id, id);
        assert_eq!(items.homework()[0].body, "Выполнено");
        assert!(items.homework()[0].completed);
        assert!(items.restore(&removed).is_err());
    }

    #[test]
    fn undo_homework_uses_lesson_identity_and_keeps_standalone_state() {
        let items = Lessons::practice(|| {});
        let mut lesson = parse_lesson("Физика", "06.11.2026", "08:00", "08:40", "23").unwrap();
        items.store_lesson(lesson.clone(), None).unwrap();
        let id = items.borrow()[0].id;
        items.set_homework_text(id, "Старое", true).unwrap();
        items
            .set_homework_completed(HomeworkId::Linked(id), true)
            .unwrap();
        let removed = items.delete_homework(HomeworkId::Linked(id)).unwrap();
        items.set_homework_text(id, "Новое", true).unwrap();
        assert!(items.restore(&removed).is_err());
        assert_eq!(items.homework()[0].body, "Новое");

        items.delete_homework(HomeworkId::Linked(id)).unwrap();
        lesson.date = lesson.date.succ_opt().unwrap();
        items.store_lesson(lesson, Some(id)).unwrap();
        items.restore(&removed).unwrap();
        assert_eq!(items.homework()[0].body, "Старое");
        assert!(items.homework()[0].completed);

        let id = items
            .save_pending(None, "Музыка", None, "Послушать", None)
            .unwrap();
        items.set_homework_completed(id, true).unwrap();
        let removed = items.delete_homework(id).unwrap();
        items.restore(&removed).unwrap();
        let pending = items
            .homework()
            .into_iter()
            .find(|task| task.id == id)
            .unwrap();
        assert_eq!(pending.body, "Послушать");
        assert_eq!(pending.date, None);
        assert!(pending.completed);
    }
}
