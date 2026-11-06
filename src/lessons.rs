use chrono::{NaiveDate, NaiveTime};
use std::cell::{Ref, RefCell};
use std::rc::Rc;

#[derive(Clone)]
pub struct Lessons(Rc<LessonList>);

struct LessonList {
    items: RefCell<Vec<Lesson>>,
    practice_changed: Option<Box<dyn Fn()>>,
}

impl Lessons {
    pub fn new(items: Vec<Lesson>) -> Self {
        Self(Rc::new(LessonList {
            items: RefCell::new(items),
            practice_changed: None,
        }))
    }

    pub fn practice(on_change: impl Fn() + 'static) -> Self {
        Self(Rc::new(LessonList {
            items: RefCell::new(Vec::new()),
            practice_changed: Some(Box::new(on_change)),
        }))
    }

    pub fn borrow(&self) -> Ref<'_, Vec<Lesson>> {
        self.0.items.borrow()
    }

    pub fn is_practice(&self) -> bool {
        self.0.practice_changed.is_some()
    }

    fn replace(&self, updated: Vec<Lesson>) -> Result<(), String> {
        if !self.is_practice() {
            crate::storage::save_json("lessons.json", &updated)?;
        }
        *self.0.items.borrow_mut() = updated;
        if let Some(on_change) = &self.0.practice_changed {
            on_change();
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Lesson {
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

fn check_slot(items: &[Lesson], lesson: &Lesson, index: Option<usize>) -> Result<(), String> {
    for (other_index, other) in items.iter().enumerate() {
        if Some(other_index) == index || other.date != lesson.date {
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

pub fn load_lessons() -> Result<Vec<Lesson>, String> {
    Ok(
        crate::storage::load_json("lessons.json", |items: &mut Vec<Lesson>| {
            items.sort_by_key(|lesson| (lesson.date, lesson.time));
            validate_sorted_lessons(items)
        })?
        .unwrap_or_default(),
    )
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

impl Lessons {
    pub fn store_lesson(&self, lesson: Lesson, index: Option<usize>) -> Result<(), String> {
        validate_lesson(&lesson)?;
        check_slot(&self.borrow(), &lesson, index)?;
        let mut updated = self.borrow().clone();
        match index {
            Some(index) => {
                let previous = updated
                    .get_mut(index)
                    .ok_or("Урок уже удалён. Откройте форму заново")?;
                *previous = lesson;
            }
            None => updated.push(lesson),
        }
        updated.sort_by_key(|lesson| (lesson.date, lesson.time));
        self.replace(updated)
    }

    pub fn delete_lesson(&self, index: usize) -> Result<(), String> {
        if index >= self.borrow().len() {
            return Err("Урок уже удалён".to_string());
        }
        let mut updated = self.borrow().clone();
        updated.remove(index);
        self.replace(updated)
    }

    pub fn set_homework_completed(&self, index: usize, completed: bool) -> Result<(), String> {
        let mut updated = self.borrow().clone();

        let lesson = updated.get_mut(index).ok_or("Урок не найден")?;

        if lesson.homework.trim().is_empty() {
            return Err("У этого урока нет домашнего задания".to_string());
        }

        lesson.homework_completed = completed;

        self.replace(updated)
    }

    pub fn set_homework_text(&self, index: usize, value: &str) -> Result<(), String> {
        let value = value.trim();

        if value.is_empty() {
            return Err("Напишите домашнее задание".to_string());
        }

        let mut updated = self.borrow().clone();
        let lesson = updated.get_mut(index).ok_or("Урок не найден")?;

        if lesson.homework != value {
            lesson.homework_completed = false;
        }

        lesson.homework = value.to_string();
        validate_lesson(lesson)?;

        self.replace(updated)
    }

    pub fn delete_homework(&self, index: usize) -> Result<(), String> {
        let mut updated = self.borrow().clone();
        let lesson = updated.get_mut(index).ok_or("Урок не найден")?;
        lesson.homework.clear();
        lesson.homework_completed = false;
        self.replace(updated)
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
}
