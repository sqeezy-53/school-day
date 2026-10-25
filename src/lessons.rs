use chrono::{NaiveDate, NaiveTime};

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Lesson {
    pub title: String,
    pub date: NaiveDate,
    pub time: NaiveTime,
    pub end: NaiveTime,
    pub room: String,
}

const FILE: &str = "data/lessons.json";

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
    };
    validate_lesson(&lesson)?;
    Ok(lesson)
}

fn validate_lesson(lesson: &Lesson) -> Result<(), String> {
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
            return Err(format!(
                "В это время уже есть {} ({}–{})",
                other.title,
                other.time.format("%H:%M"),
                other.end.format("%H:%M"),
            ));
        }
    }
    Ok(())
}

pub fn load_lessons() -> Result<Vec<Lesson>, String> {
    let json = match std::fs::read_to_string(FILE) {
        Ok(json) => json,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.to_string()),
    };
    let mut items: Vec<Lesson> = serde_json::from_str(&json).map_err(|error| error.to_string())?;
    for (index, lesson) in items.iter().enumerate() {
        validate_lesson(lesson)?;
        check_slot(&items, lesson, Some(index))?;
    }
    items.sort_by_key(|lesson| (lesson.date, lesson.time));
    Ok(items)
}

fn save_lessons(items: &[Lesson]) -> Result<(), String> {
    let json = serde_json::to_string_pretty(items).map_err(|error| error.to_string())?;
    std::fs::create_dir_all("data").map_err(|error| error.to_string())?;
    // Сначала пишем отдельный файл. Старый заменяем только после успешной записи.
    let temporary = "data/lessons.json.tmp";
    std::fs::write(temporary, json).map_err(|error| error.to_string())?;
    std::fs::rename(temporary, FILE).map_err(|error| error.to_string())
}

pub fn store_lesson(
    items: &mut Vec<Lesson>,
    lesson: Lesson,
    index: Option<usize>,
) -> Result<(), String> {
    validate_lesson(&lesson)?;
    check_slot(items, &lesson, index)?;
    let mut updated = items.clone();
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
    save_lessons(&updated)?;
    // В памяти и на экране данные меняются только после сохранения на диске.
    *items = updated;
    Ok(())
}

pub fn delete_lesson(items: &mut Vec<Lesson>, index: usize) -> Result<(), String> {
    if index >= items.len() {
        return Err("Урок уже удалён".to_string());
    }
    let mut updated = items.clone();
    updated.remove(index);
    save_lessons(&updated)?;
    *items = updated;
    Ok(())
}
