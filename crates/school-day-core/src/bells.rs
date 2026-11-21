use chrono::{Duration, NaiveTime};
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct BellSchedule {
    pub lesson_minutes: u32,
    pub first_start: NaiveTime,
    pub breaks: Vec<u32>,
}

impl Default for BellSchedule {
    fn default() -> Self {
        Self {
            lesson_minutes: 40,
            first_start: NaiveTime::from_hms_opt(8, 0, 0).unwrap(),
            breaks: vec![10, 10, 15, 15, 10, 10, 10, 10],
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Bell {
    pub start: NaiveTime,
    pub end: NaiveTime,
}

impl BellSchedule {
    pub fn slots(&self) -> Result<Vec<Bell>, String> {
        if ![40, 45].contains(&self.lesson_minutes) {
            return Err("Длительность урока: 40 или 45 минут".to_string());
        }
        if self.breaks.len() > 11 {
            return Err("Можно задать от 1 до 12 уроков".to_string());
        }
        if self.breaks.iter().any(|minutes| *minutes > 120) {
            return Err("Перемена должна быть от 0 до 120 минут".to_string());
        }

        let mut slots = Vec::with_capacity(self.breaks.len() + 1);
        let mut start = self.first_start;
        for index in 0..=self.breaks.len() {
            let end = add_minutes(start, self.lesson_minutes)?;
            slots.push(Bell { start, end });
            if let Some(minutes) = self.breaks.get(index) {
                start = add_minutes(end, *minutes)?;
            }
        }
        Ok(slots)
    }
}

fn add_minutes(time: NaiveTime, minutes: u32) -> Result<NaiveTime, String> {
    let (result, overflow) = time.overflowing_add_signed(Duration::minutes(minutes as i64));
    if overflow != 0 {
        return Err("Звонки должны закончиться до полуночи".to_string());
    }
    Ok(result)
}
