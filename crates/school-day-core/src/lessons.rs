use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use std::cell::{Cell, Ref, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use crate::forms::{HOMEWORK_LIMIT, SUBJECT_LIMIT, length_error};

mod history;
pub mod sync;
pub use history::{Changes, Key, conflicts};

pub type RecordId = uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HomeworkId {
    Linked(RecordId),
    Pending(RecordId),
}

pub struct Homework {
    pub lesson_time: Option<NaiveTime>,
    pub id: HomeworkId,
    pub title: String,
    pub date: Option<NaiveDate>,
    pub body: String,
    pub completed: bool,
}

#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
struct PendingHomework {
    #[serde(default)]
    id: RecordId,
    title: String,
    date: Option<NaiveDate>,
    body: String,
    #[serde(default)]
    completed: bool,
}

#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SchoolData {
    #[serde(default)]
    format_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bells: Option<crate::bells::BellSchedule>,
    lessons: Vec<Lesson>,
    pending_homework: Vec<PendingHomework>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    days: BTreeMap<NaiveDate, DayOptions>,
}

impl Default for SchoolData {
    fn default() -> Self {
        Self {
            format_version: FORMAT_VERSION,
            bells: None,
            lessons: Vec::new(),
            pending_homework: Vec::new(),
            days: BTreeMap::new(),
        }
    }
}

pub const MAX_SHORT_MINUTES: u16 = 1439;
pub const NOTE_LIMIT: usize = 240;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DayOptions {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub remote: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short_minutes: Option<u16>,
}

impl DayOptions {
    fn validate(self) -> Result<(), String> {
        if self
            .short_minutes
            .is_some_and(|minutes| !(1..=MAX_SHORT_MINUTES).contains(&minutes))
        {
            return Err("Укажите длительность урока от 1 до 1439 минут".into());
        }
        Ok(())
    }

    pub fn end_time(self, lesson: &Lesson) -> NaiveTime {
        self.short_minutes.map_or(lesson.end, |minutes| {
            let (end, overflow) = lesson
                .time
                .overflowing_add_signed(chrono::Duration::minutes(minutes.into()));
            if overflow == 0 {
                lesson.end.min(end)
            } else {
                lesson.end
            }
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LessonWork {
    #[default]
    Regular,
    Test,
    Independent,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct LessonOptions {
    pub cancelled: bool,
    pub replacement: String,
    pub work: LessonWork,
}

impl LessonOptions {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }
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
        task: Option<RecordId>,
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

type Save = dyn Fn(&SchoolData) -> Result<(), String>;
type RestoreCallback = Box<dyn Fn(&Lessons, &Removed)>;

#[derive(Clone)]
pub struct Lessons(Rc<LessonList>);

struct LessonList {
    data: RefCell<SchoolData>,
    save: Box<Save>,
    history: RefCell<history::History>,
    revision: Cell<u64>,
    generation: Cell<u64>,
    practice_changed: Option<Box<dyn Fn()>>,
    restored: RefCell<Vec<RestoreCallback>>,
    imported: RefCell<Vec<Box<dyn Fn()>>>,
}

impl Lessons {
    pub fn new(data: SchoolData) -> Self {
        Self::with_writer(data, |_| Ok(()))
    }

    pub fn with_writer(
        data: SchoolData,
        save: impl Fn(&SchoolData) -> Result<(), String> + 'static,
    ) -> Self {
        Self(Rc::new(LessonList {
            data: RefCell::new(data),
            history: RefCell::default(),
            save: Box::new(save),
            revision: Cell::new(0),
            generation: Cell::new(0),
            practice_changed: None,
            restored: RefCell::new(Vec::new()),
            imported: RefCell::default(),
        }))
    }

    pub fn practice(on_change: impl Fn() + 'static) -> Self {
        Self(Rc::new(LessonList {
            data: RefCell::new(SchoolData::default()),
            history: RefCell::default(),
            save: Box::new(|_| Ok(())),
            revision: Cell::new(0),
            generation: Cell::new(0),
            practice_changed: Some(Box::new(on_change)),
            restored: RefCell::new(Vec::new()),
            imported: RefCell::default(),
        }))
    }

    pub fn borrow(&self) -> Ref<'_, Vec<Lesson>> {
        Ref::map(self.0.data.borrow(), |data| &data.lessons)
    }

    pub fn on_date(&self, date: NaiveDate) -> Ref<'_, [Lesson]> {
        Ref::map(self.0.data.borrow(), |data| {
            let lessons = &data.lessons;
            let first = lessons.partition_point(|lesson| lesson.date < date);
            let count = lessons[first..].partition_point(|lesson| lesson.date == date);
            &lessons[first..first + count]
        })
    }

    pub fn generation(&self) -> u64 {
        self.0.generation.get()
    }
    pub fn connect_imported(&self, action: impl Fn() + 'static) {
        self.0.imported.borrow_mut().push(Box::new(action));
    }

    pub fn set_bells(&self, bells: crate::bells::BellSchedule) -> Result<(), String> {
        bells.slots()?;
        if self.data().bells.as_ref() == Some(&bells) {
            return Ok(());
        }
        let mut next = self.data().clone();
        next.bells = Some(bells);
        self.replace(next)
    }

    pub fn revision(&self) -> u64 {
        self.0.revision.get()
    }

    pub fn day_options(&self, date: NaiveDate) -> DayOptions {
        self.0
            .data
            .borrow()
            .days
            .get(&date)
            .copied()
            .unwrap_or_default()
    }

    pub fn set_day_options(&self, date: NaiveDate, options: DayOptions) -> Result<(), String> {
        options.validate()?;
        if self.day_options(date) == options {
            return Ok(());
        }
        let mut data = self.0.data.borrow().clone();
        if options == DayOptions::default() {
            data.days.remove(&date);
        } else {
            data.days.insert(date, options);
        }
        self.replace(data)
    }

    pub fn is_practice(&self) -> bool {
        self.0.practice_changed.is_some()
    }

    pub fn data(&self) -> Ref<'_, SchoolData> {
        self.0.data.borrow()
    }

    fn replace(&self, updated: SchoolData) -> Result<(), String> {
        let change = Changes::between(&self.data(), &updated);
        self.commit(updated)?;
        self.remember(change);
        Ok(())
    }

    fn commit(&self, updated: SchoolData) -> Result<(), String> {
        (self.0.save)(&updated)?;
        *self.0.data.borrow_mut() = updated;
        self.0.revision.set(self.revision().wrapping_add(1));
        if let Some(on_change) = &self.0.practice_changed {
            on_change();
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Lesson {
    #[serde(default)]
    pub id: RecordId,
    pub title: String,
    pub date: NaiveDate,
    pub time: NaiveTime,
    pub end: NaiveTime,
    #[serde(default)]
    pub room: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    #[serde(default, skip_serializing_if = "LessonOptions::is_default")]
    pub options: LessonOptions,
    #[serde(default)]
    pub homework: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homework_id: Option<RecordId>,

    #[serde(default)]
    pub homework_completed: bool,
}

impl Lesson {
    pub fn label(&self) -> String {
        format!(
            "{} · {} в {}",
            self.title,
            self.date.format("%d.%m.%Y"),
            self.time.format("%H:%M")
        )
    }

    pub fn new(
        title: &str,
        date: NaiveDate,
        time: NaiveTime,
        end: NaiveTime,
        room: &str,
    ) -> Result<Self, String> {
        let title = title.trim();
        let room = room.trim();
        if title.is_empty() {
            return Err("Введите название предмета".to_string());
        }
        let lesson = Self {
            id: RecordId::nil(),
            title: title.to_string(),
            date,
            time,
            end,
            room: room.to_string(),
            note: String::new(),
            options: LessonOptions::default(),
            homework: String::new(),
            homework_id: None,
            homework_completed: false,
        };
        validate_lesson(&lesson)?;
        Ok(lesson)
    }
}

pub fn parse_lesson(
    title: &str,
    date: &str,
    time: &str,
    end: &str,
    room: &str,
) -> Result<Lesson, String> {
    let date = NaiveDate::parse_from_str(date, "%d.%m.%Y")
        .map_err(|_| "Выберите дату урока".to_string())?;
    let time = NaiveTime::parse_from_str(time, "%H:%M")
        .map_err(|_| "Проверьте время начала".to_string())?;
    let end = NaiveTime::parse_from_str(end, "%H:%M")
        .map_err(|_| "Проверьте время окончания".to_string())?;
    Lesson::new(title, date, time, end, room)
}

fn validate_lesson(lesson: &Lesson) -> Result<(), String> {
    if [
        &lesson.title,
        &lesson.room,
        &lesson.homework,
        &lesson.note,
        &lesson.options.replacement,
    ]
    .iter()
    .any(|text| text.contains('\0'))
    {
        return Err("Недопустимые символы в данных урока".to_string());
    }
    if lesson.title.trim().is_empty() {
        return Err("Укажите предмет".to_string());
    }
    if let Some(error) = length_error(&lesson.note, "", NOTE_LIMIT)
        .or_else(|| length_error(&lesson.options.replacement, "", SUBJECT_LIMIT))
    {
        return Err(error);
    }
    if lesson.end <= lesson.time {
        return Err("Время окончания должно быть позже начала".to_string());
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

pub const FORMAT_VERSION: u32 = 1;
pub const MAX_BYTES: usize = 32 * 1024 * 1024;

impl SchoolData {
    /// Accepts both 0.6.2 layouts. Only legacy documents may omit identifiers.
    pub fn decode(bytes: &[u8]) -> Result<(Self, bool), String> {
        if bytes.len() > MAX_BYTES {
            return Err("Размер файла превышает 32 МиБ".into());
        }
        let mut saved: SavedData = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        validate_saved(&mut saved)?;
        let mut data = match saved {
            SavedData::Legacy(lessons) => Self {
                lessons,
                format_version: 0,
                ..Default::default()
            },
            SavedData::Current(data) => data,
        };
        let legacy = data.format_version == 0;
        if !legacy && data.format_version != FORMAT_VERSION {
            return Err("Неизвестная версия формата расписания".into());
        }
        let mut seen = std::collections::HashSet::new();
        for id in data
            .lessons
            .iter_mut()
            .map(|l| &mut l.id)
            .chain(data.pending_homework.iter_mut().map(|h| &mut h.id))
        {
            if legacy && id.is_nil() {
                *id = RecordId::new_v4();
            }
            if id.is_nil() || !seen.insert(*id) {
                return Err("Пустой или повторяющийся идентификатор записи".into());
            }
        }
        for lesson in &mut data.lessons {
            if legacy && !lesson.homework.trim().is_empty() && lesson.homework_id.is_none() {
                lesson.homework_id = Some(RecordId::new_v4());
            }
            if lesson.homework.trim().is_empty() != lesson.homework_id.is_none() {
                return Err("Нарушена связь урока и задания".into());
            }
            if let Some(id) = lesson.homework_id
                && (id.is_nil() || !seen.insert(id))
            {
                return Err("Повторяющийся идентификатор задания".into());
            }
        }
        if let Some(bells) = &data.bells {
            bells.slots()?;
        }
        data.format_version = FORMAT_VERSION;
        Ok((data, legacy))
    }

    pub fn counts(&self) -> (usize, usize, usize) {
        (
            self.lessons.len(),
            self.pending_homework.len()
                + self
                    .lessons
                    .iter()
                    .filter(|l| !l.homework.is_empty())
                    .count(),
            self.days.len(),
        )
    }
}

fn validate_saved(saved: &mut SavedData) -> Result<(), String> {
    let lessons = match saved {
        SavedData::Legacy(lessons) => lessons,
        SavedData::Current(data) => {
            for options in data.days.values() {
                options.validate()?;
            }
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
        return Err("Укажите предмет и текст задания".into());
    }
    if title.contains('\0') || body.contains('\0') {
        return Err("Недопустимые символы в задании".into());
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
        lesson.homework_id = Some(task.id);
        lesson.homework = task.body;
        lesson.homework_completed = task.completed;
        Ok(HomeworkId::Linked(lesson_id))
    }

    fn auto_attach(
        &mut self,
        now: NaiveDateTime,
        only_task: Option<RecordId>,
    ) -> Vec<(RecordId, RecordId)> {
        let proposals: Vec<_> = self
            .pending_homework
            .iter()
            .filter_map(|task| {
                let matches = |lesson: &&Lesson| same_subject(&lesson.title, &task.title);
                let target = if let Some(date) = task.date {
                    let mut candidates = self
                        .lessons
                        .iter()
                        .filter(|lesson| lesson.date == date)
                        .filter(matches);
                    let lesson = candidates.next()?;
                    if candidates.next().is_some()
                        || lesson.date.and_time(lesson.time) <= now
                        || !lesson.homework.trim().is_empty()
                    {
                        return None;
                    }
                    lesson
                } else {
                    self.lessons
                        .iter()
                        .filter(|lesson| {
                            lesson.date.and_time(lesson.time) > now
                                && lesson.homework.trim().is_empty()
                        })
                        .filter(matches)
                        .min_by_key(|lesson| (lesson.date, lesson.time))?
                };
                Some((task.id, target.id))
            })
            .collect();
        let mut counts = HashMap::new();
        for &(_, lesson) in &proposals {
            *counts.entry(lesson).or_insert(0usize) += 1;
        }
        let mut attached = Vec::new();
        for &(task, lesson) in &proposals {
            if only_task.is_some_and(|wanted| task != wanted) {
                continue;
            }
            if counts.get(&lesson) != Some(&1) {
                continue;
            }
            if let Ok(index) = self.pending_index(task)
                && self.attach(index, lesson).is_ok()
            {
                attached.push((task, lesson));
            }
        }
        attached
    }
}

impl Lessons {
    pub fn store_lesson(
        &self,
        mut lesson: Lesson,
        id: Option<RecordId>,
    ) -> Result<Vec<RecordId>, String> {
        lesson.id = id.unwrap_or_else(RecordId::new_v4);
        validate_lesson(&lesson)?;
        let mut data = self.0.data.borrow().clone();
        check_slot(&data.lessons, &lesson)?;
        let previous = id.map(|id| data.lesson_mut(id)).transpose()?;
        for (new, old, limit) in [
            (
                &lesson.title,
                previous.as_ref().map(|l| &l.title),
                SUBJECT_LIMIT,
            ),
            (
                &lesson.room,
                previous.as_ref().map(|l| &l.room),
                crate::forms::ROOM_LIMIT,
            ),
            (
                &lesson.homework,
                previous.as_ref().map(|l| &l.homework),
                HOMEWORK_LIMIT,
            ),
        ] {
            if let Some(e) = length_error(new, old.map_or("", String::as_str), limit) {
                return Err(e);
            }
        }
        lesson.homework_id = (!lesson.homework.trim().is_empty()).then(|| {
            previous
                .and_then(|l| l.homework_id)
                .unwrap_or_else(RecordId::new_v4)
        });
        if let Some(id) = id {
            *data.lesson_mut(id)? = lesson;
        } else {
            data.lessons.push(lesson);
        }
        data.lessons
            .sort_by_key(|lesson| (lesson.date, lesson.time));
        let attached = data
            .auto_attach(chrono::Local::now().naive_local(), None)
            .into_iter()
            .map(|(_, lesson)| lesson)
            .collect();
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
                task,
                body,
                completed,
            } => {
                let lesson = data
                    .lesson_mut(*lesson)
                    .map_err(|_| "Урок удалён. Сначала верните урок")?;
                if !lesson.homework.trim().is_empty() {
                    return Err("У этого урока уже есть домашнее задание".into());
                }
                lesson.homework_id = *task;
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
        self.notify_restored(removed);
        Ok(())
    }

    pub fn notify_restored(&self, removed: &Removed) {
        for callback in self.0.restored.borrow().iter() {
            callback(self, removed);
        }
    }

    pub fn homework(&self) -> Vec<Homework> {
        let data = self.0.data.borrow();
        let mut tasks: Vec<_> = data
            .lessons
            .iter()
            .filter(|lesson| !lesson.homework.trim().is_empty())
            .map(|lesson| Homework {
                lesson_time: Some(lesson.time),
                id: HomeworkId::Linked(lesson.id),
                title: lesson.title.clone(),
                date: Some(lesson.date),
                body: lesson.homework.clone(),
                completed: lesson.homework_completed,
            })
            .chain(data.pending_homework.iter().map(|task| Homework {
                lesson_time: None,
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

    pub fn completed_homework_count(&self) -> usize {
        let data = self.data();
        data.lessons
            .iter()
            .filter(|lesson| lesson.homework_completed && !lesson.homework.trim().is_empty())
            .count()
            + data
                .pending_homework
                .iter()
                .filter(|task| task.completed)
                .count()
    }

    pub fn clear_completed_homework(&self, expected: usize) -> Result<(), String> {
        if expected != self.completed_homework_count() {
            return Err(
                "Количество выполненных заданий изменилось. Подтвердите очистку ещё раз.".into(),
            );
        }
        if expected == 0 {
            return Ok(());
        }
        let mut data = self.data().clone();
        for lesson in data
            .lessons
            .iter_mut()
            .filter(|lesson| lesson.homework_completed && !lesson.homework.trim().is_empty())
        {
            lesson.homework.clear();
            lesson.homework_id = None;
            lesson.homework_completed = false;
        }
        data.pending_homework.retain(|task| !task.completed);
        self.replace(data)
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
        lesson.homework_id.get_or_insert_with(RecordId::new_v4);
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
            id: id.unwrap_or_else(RecordId::new_v4),
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
            data.auto_attach(
                chrono::Local::now().naive_local(),
                Some(data.pending_homework[index].id),
            )
            .into_iter()
            .find(|(task, _)| pending == HomeworkId::Pending(*task))
            .map_or(pending, |(_, lesson)| HomeworkId::Linked(lesson))
        };
        self.replace(data)?;
        Ok(saved)
    }

    pub fn attachment_notice(&self, ids: &[RecordId]) -> String {
        let data = self.0.data.borrow();
        let labels: Vec<_> = ids
            .iter()
            .filter_map(|id| data.lessons.iter().find(|lesson| lesson.id == *id))
            .map(Lesson::label)
            .collect();
        format!("ДЗ прикреплено: {}", labels.join("; "))
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
                    task: lesson.homework_id.take(),
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
    fn typed_lesson_creation_preserves_values_and_validation() -> Result<(), String> {
        let date = NaiveDate::from_ymd_opt(2026, 11, 10).ok_or("Неверная дата теста")?;
        let start = NaiveTime::from_hms_opt(8, 0, 30).ok_or("Неверное время теста")?;
        let end = NaiveTime::from_hms_opt(8, 40, 30).ok_or("Неверное время теста")?;
        let lesson = Lesson::new("  Физика  ", date, start, end, "  23  ")?;
        assert_eq!(lesson.title, "Физика");
        assert_eq!(lesson.room, "23");
        assert!(
            Lesson::new("Физика", date, start, end, " ")?
                .room
                .is_empty()
        );
        assert_eq!((lesson.date, lesson.time, lesson.end), (date, start, end));
        assert!(lesson.id.is_nil());
        assert!(lesson.homework.is_empty());
        assert!(!lesson.homework_completed);
        for (title, room, finish, error) in [
            (" ", "23", end, "Введите название предмета"),
            ("Физика\0", "23", end, "Недопустимые символы в данных урока"),
            (
                "Физика",
                "23",
                start,
                "Время окончания должно быть позже начала",
            ),
        ] {
            assert_eq!(
                Lesson::new(title, date, start, finish, room).unwrap_err(),
                error
            );
        }
        Ok(())
    }

    #[test]
    fn date_options_and_lesson_details_round_trip_without_changing_originals() -> Result<(), String>
    {
        let legacy = r#"[
            {"title":"Математика","date":"2026-11-10","time":"08:00:00","end":"08:40:00","homework":"Задача","homework_completed":true},
            {"title":"Математика","date":"2026-11-17","time":"08:00:00","end":"08:40:00","room":"23"}
        ]"#;
        let mut data = serde_json::from_str::<SavedData>(legacy).unwrap();
        validate_saved(&mut data)?;
        let SavedData::Legacy(lessons) = data else {
            unreachable!()
        };
        let date = lessons[0].date;
        let other = lessons[1].date;
        let items = Lessons::practice(|| {});
        for lesson in lessons {
            items.store_lesson(lesson, None)?;
        }
        let mut lesson = items.on_date(date)[0].clone();
        assert!(lesson.room.is_empty());
        assert_eq!(lesson.options, LessonOptions::default());
        lesson.note = "Принести атлас".into();
        lesson.options = LessonOptions {
            cancelled: true,
            replacement: "Физика".into(),
            work: LessonWork::Independent,
        };
        items.store_lesson(lesson.clone(), Some(lesson.id))?;
        let options = DayOptions {
            remote: true,
            short_minutes: Some(30),
        };
        items.set_day_options(date, options)?;
        assert!(
            items
                .set_day_options(
                    other,
                    DayOptions {
                        remote: true,
                        short_minutes: Some(0)
                    }
                )
                .is_err()
        );

        let json = serde_json::to_string(&*items.0.data.borrow()).unwrap();
        let mut saved = serde_json::from_str::<SavedData>(&json).unwrap();
        validate_saved(&mut saved)?;
        let SavedData::Current(data) = saved else {
            unreachable!()
        };
        let persisted = Lessons::new(data);
        assert_eq!(persisted.day_options(date), options);
        assert_eq!(persisted.day_options(other), DayOptions::default());
        assert_eq!(
            options.end_time(&persisted.on_date(date)[0]),
            NaiveTime::from_hms_opt(8, 30, 0).unwrap()
        );
        assert_eq!(persisted.on_date(date)[0], lesson);
        assert_eq!(persisted.on_date(other)[0].room, "23");
        assert_eq!(
            persisted.on_date(other)[0].options,
            LessonOptions::default()
        );
        assert_eq!(persisted.homework()[0].body, "Задача");
        assert!(persisted.homework()[0].completed);

        items.set_day_options(date, DayOptions::default())?;
        assert!(items.0.data.borrow().days.is_empty());
        assert_eq!(items.day_options(date).end_time(&lesson), lesson.end);
        assert_eq!(
            DayOptions {
                short_minutes: Some(60),
                ..options
            }
            .end_time(&lesson),
            lesson.end
        );
        Ok(())
    }

    #[test]
    fn string_lesson_parsing_keeps_input_errors() {
        for (date, start, end, error) in [
            ("31.02.2026", "08:00", "08:40", "Выберите дату урока"),
            ("10.11.2026", "24:00", "08:40", "Проверьте время начала"),
            ("10.11.2026", "08:00", "24:00", "Проверьте время окончания"),
        ] {
            assert_eq!(
                parse_lesson("Физика", date, start, end, "23").unwrap_err(),
                error
            );
        }
    }

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
        let mut lesson =
            parse_lesson("Физика  и астрономия", "06.11.2026", "08:00", "08:40", "23").unwrap();
        lesson.date = chrono::Local::now().date_naive().succ_opt().unwrap();
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
        assert!(items.store_lesson(different_day, None).unwrap().is_empty());
        assert_eq!(items.homework()[0].id, task);
        assert!(!items.store_lesson(lesson, None).unwrap().is_empty());
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
    fn competing_tasks_are_not_assigned_by_insertion_order() {
        let items = Lessons::practice(|| {});
        let mut lesson = parse_lesson("Физика", "06.11.2026", "08:00", "08:40", "23").unwrap();
        lesson.date = chrono::Local::now().date_naive().succ_opt().unwrap();
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
        assert!(items.store_lesson(lesson, None).unwrap().is_empty());
        assert_eq!(items.homework().len(), 3);
        assert!(items.borrow()[0].homework.is_empty());
    }

    #[test]
    fn undated_homework_uses_the_nearest_free_future_lesson() {
        let now = NaiveDate::from_ymd_opt(2026, 11, 10)
            .unwrap()
            .and_hms_opt(9, 0, 0)
            .unwrap();
        let mut data = SchoolData::default();
        for (id, title, day, start, body) in [
            (1, "Алгебра", "09.11.2026", "10:00", ""),
            (2, "Алгебра", "10.11.2026", "08:00", ""),
            (3, "Алгебра", "10.11.2026", "09:00", ""),
            (4, "Геометрия", "10.11.2026", "10:00", ""),
            (5, "Алгебра", "10.11.2026", "11:00", "Занято"),
            (6, "Алгебра", "10.11.2026", "12:00", ""),
            (7, "Алгебра", "11.11.2026", "08:00", ""),
        ] {
            let time = NaiveTime::parse_from_str(start, "%H:%M").unwrap();
            let mut lesson = Lesson::new(
                title,
                NaiveDate::parse_from_str(day, "%d.%m.%Y").unwrap(),
                time,
                time + chrono::Duration::minutes(40),
                "12",
            )
            .unwrap();
            lesson.id = RecordId::from_u128(id);
            lesson.homework = body.into();
            data.lessons.push(lesson);
        }
        data.pending_homework.push(PendingHomework {
            id: RecordId::from_u128(8),
            title: " АЛГЕБРА ".into(),
            date: None,
            body: "Задача 4".into(),
            completed: true,
        });
        assert_eq!(
            data.auto_attach(now, None),
            vec![(RecordId::from_u128(8), RecordId::from_u128(6))]
        );
        assert!(data.pending_homework.is_empty());
        assert_eq!(data.lessons[4].homework, "Занято");
        assert_eq!(data.lessons[5].homework, "Задача 4");
        assert!(data.lessons[5].homework_completed);
        assert!(data.auto_attach(now, None).is_empty());
    }

    #[test]
    fn explicit_dates_and_ambiguity_are_respected() {
        let now = NaiveDate::from_ymd_opt(2026, 11, 10)
            .unwrap()
            .and_hms_opt(9, 0, 0)
            .unwrap();
        let mut data = SchoolData::default();
        for (id, day, start, end) in [
            (1, "09.11.2026", "08:00", "08:40"),
            (2, "11.11.2026", "08:00", "08:40"),
            (3, "11.11.2026", "09:00", "09:40"),
            (4, "12.11.2026", "08:00", "08:40"),
        ] {
            let mut lesson = parse_lesson("Физика", day, start, end, "23").unwrap();
            lesson.id = RecordId::from_u128(id);
            data.lessons.push(lesson);
        }
        for (id, day) in [(5, 9), (6, 11), (7, 13), (8, 12)] {
            data.pending_homework.push(PendingHomework {
                id: RecordId::from_u128(id),
                title: "Физика".into(),
                date: NaiveDate::from_ymd_opt(2026, 11, day),
                body: format!("Задание {id}"),
                completed: false,
            });
        }
        assert_eq!(
            data.auto_attach(now, None),
            vec![(RecordId::from_u128(8), RecordId::from_u128(4))]
        );
        assert_eq!(
            data.pending_homework
                .iter()
                .map(|task| task.id.as_u128())
                .collect::<Vec<_>>(),
            vec![5, 6, 7]
        );
        data.pending_homework[0].date = None;
        data.pending_homework[1].date = None;
        assert!(data.auto_attach(now, None).is_empty());
        assert!(data.lessons[1].homework.is_empty());
    }

    #[test]
    fn a_later_schedule_change_attaches_undated_homework_and_preserves_manual_targets() {
        let items = Lessons::practice(|| {});
        let today = chrono::Local::now().date_naive();
        let mut lesson = parse_lesson("Алгебра", "11.11.2026", "08:00", "08:40", "12").unwrap();
        lesson.date = today.succ_opt().unwrap();
        let task = items
            .save_pending(None, "АЛГЕБРА", None, "Задача 1", None)
            .unwrap();
        items.set_homework_completed(task, true).unwrap();
        let mut other = lesson.clone();
        other.title = "Геометрия".into();
        assert!(items.store_lesson(other, None).unwrap().is_empty());
        let id = items.borrow()[0].id;
        let attached = items.store_lesson(lesson.clone(), Some(id)).unwrap();
        assert_eq!(attached, vec![id]);
        assert_eq!(items.homework().len(), 1);
        assert_eq!(items.homework()[0].id, HomeworkId::Linked(id));
        assert!(items.homework()[0].completed);
        let mut later = lesson.clone();
        later.date = lesson.date.succ_opt().unwrap();
        items.store_lesson(later, None).unwrap();
        let later_id = items.borrow()[1].id;
        let HomeworkId::Pending(pending) = items
            .save_pending(None, "Алгебра", Some(today), "Вручную", None)
            .unwrap()
        else {
            panic!("Прошедшая дата не должна привязываться");
        };
        items
            .set_homework_completed(HomeworkId::Pending(pending), true)
            .unwrap();
        assert_eq!(
            items
                .save_pending(
                    Some(pending),
                    "Алгебра",
                    Some(today),
                    "Вручную",
                    Some(later_id)
                )
                .unwrap(),
            HomeworkId::Linked(later_id)
        );
        let mut earlier = lesson;
        earlier.time += chrono::Duration::hours(1);
        earlier.end += chrono::Duration::hours(1);
        items.store_lesson(earlier, None).unwrap();
        assert_eq!(items.homework().len(), 2);
        let manual = items
            .homework()
            .into_iter()
            .find(|task| task.id == HomeworkId::Linked(later_id))
            .unwrap();
        assert_eq!(manual.body, "Вручную");
        assert!(manual.completed);
    }

    #[test]
    fn saving_one_pending_task_does_not_silently_attach_another() -> Result<(), String> {
        let items = Lessons::practice(|| {});
        let date = chrono::Local::now().date_naive().succ_opt().unwrap();
        let first = items.save_pending(None, "Алгебра", None, "Первое", None)?;
        let HomeworkId::Pending(second) =
            items.save_pending(None, "Алгебра", None, "Второе", None)?
        else {
            panic!("Пока нет уроков")
        };
        let lesson = Lesson::new(
            "Алгебра",
            date,
            NaiveTime::from_hms_opt(8, 0, 0).unwrap(),
            NaiveTime::from_hms_opt(8, 40, 0).unwrap(),
            "12",
        )?;
        assert!(items.store_lesson(lesson, None)?.is_empty());
        items.save_pending(Some(second), "Геометрия", None, "Второе", None)?;
        assert!(items.borrow()[0].homework.is_empty());
        assert!(
            items
                .homework()
                .iter()
                .any(|task| task.id == first && task.body == "Первое")
        );
        Ok(())
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
                id: RecordId::nil(),
                title: "География".into(),
                date: None,
                body: "Новое ДЗ".into(),
                completed: true,
            }],
            ..SchoolData::default()
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
            homework_id: None,
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
