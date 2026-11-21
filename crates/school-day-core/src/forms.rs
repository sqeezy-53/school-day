pub const SUBJECT_LIMIT: usize = 64;
pub const ROOM_LIMIT: usize = 12;
pub const CLASS_LIMIT: usize = 12;
pub const HOMEWORK_LIMIT: usize = 2000;

pub fn length_error(value: &str, previous: &str, limit: usize) -> Option<String> {
    if value == previous {
        return None;
    }
    let count = value.chars().count();
    (count > limit).then(|| format!("Максимум {limit} символов. Сейчас {count}."))
}
