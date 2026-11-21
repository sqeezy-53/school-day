use super::*;
use std::collections::{BTreeSet, VecDeque};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub enum Key {
    Bells,
    Lesson(RecordId),
    Task(RecordId),
    Day(NaiveDate),
}

enum Prior {
    Bells(Option<crate::bells::BellSchedule>),
    Lesson(RecordId, Option<Lesson>),
    Task(RecordId, Option<(usize, PendingHomework)>),
    Day(NaiveDate, Option<DayOptions>),
}

pub struct Changes(Vec<Prior>);

impl Changes {
    pub fn between(old: &SchoolData, new: &SchoolData) -> Self {
        let mut changes = Vec::new();
        if old.bells != new.bells {
            changes.push(Prior::Bells(old.bells.clone()));
        }
        let mut lessons: HashMap<_, _> = old.lessons.iter().map(|l| (l.id, l)).collect();
        for item in &new.lessons {
            if let Some(prior) = lessons.remove(&item.id) {
                if prior != item {
                    changes.push(Prior::Lesson(item.id, Some(prior.clone())));
                }
            } else {
                changes.push(Prior::Lesson(item.id, None));
            }
        }
        changes.extend(
            lessons
                .into_iter()
                .map(|(id, l)| Prior::Lesson(id, Some(l.clone()))),
        );
        let mut tasks: HashMap<_, _> = old
            .pending_homework
            .iter()
            .enumerate()
            .map(|(i, h)| (h.id, (i, h)))
            .collect();
        for item in &new.pending_homework {
            match tasks.remove(&item.id) {
                Some((pos, prior)) if prior != item => {
                    changes.push(Prior::Task(item.id, Some((pos, prior.clone()))))
                }
                None => changes.push(Prior::Task(item.id, None)),
                _ => {}
            }
        }
        changes.extend(
            tasks
                .into_iter()
                .map(|(id, (pos, h))| Prior::Task(id, Some((pos, h.clone())))),
        );
        for day in old
            .days
            .keys()
            .chain(new.days.keys().filter(|d| !old.days.contains_key(*d)))
        {
            if old.days.get(day) != new.days.get(day) {
                changes.push(Prior::Day(*day, old.days.get(day).copied()));
            }
        }
        Self(changes)
    }

    /// Includes deletions relative to a common export. Comparing two branches
    /// against that same export also detects delete-versus-edit conflicts.
    pub fn keys(&self) -> BTreeSet<Key> {
        self.0
            .iter()
            .map(|v| match v {
                Prior::Bells(_) => Key::Bells,
                Prior::Lesson(id, _) => Key::Lesson(*id),
                Prior::Task(id, _) => Key::Task(*id),
                Prior::Day(day, _) => Key::Day(*day),
            })
            .collect()
    }

    fn apply(&self, data: &mut SchoolData) {
        let mut tasks = vec![];
        for change in &self.0 {
            match change {
                Prior::Bells(bells) => data.bells.clone_from(bells),
                Prior::Lesson(id, prior) => {
                    data.lessons.retain(|l| l.id != *id);
                    if let Some(l) = prior {
                        data.lessons.push(l.clone());
                    }
                }
                Prior::Task(id, prior) => {
                    data.pending_homework.retain(|h| h.id != *id);
                    if let Some(h) = prior {
                        tasks.push(h);
                    }
                }
                Prior::Day(date, prior) => match prior {
                    Some(opts) => {
                        data.days.insert(*date, *opts);
                    }
                    None => {
                        data.days.remove(date);
                    }
                },
            }
        }
        tasks.sort_by_key(|(pos, _)| *pos);
        for (pos, task) in tasks {
            data.pending_homework
                .insert((*pos).min(data.pending_homework.len()), task.clone());
        }
        data.lessons.sort_by_key(|l| (l.date, l.time));
    }
}

#[derive(Default)]
pub(super) struct History {
    undo: VecDeque<Changes>,
    redo: VecDeque<Changes>,
}
impl Lessons {
    pub fn travel(&self, redo: bool) -> Result<bool, String> {
        let mut history = self.0.history.borrow_mut();
        let source = if redo {
            &mut history.redo
        } else {
            &mut history.undo
        };
        let Some(change) = source.back() else {
            return Ok(false);
        };
        let mut restored = self.data().clone();
        change.apply(&mut restored);
        let reverse = Changes::between(&self.data(), &restored);
        let changed_bells = self.data().bells != restored.bells;
        self.commit(restored)?;
        source.pop_back();
        if redo {
            &mut history.undo
        } else {
            &mut history.redo
        }
        .push_back(reverse);
        drop(history);
        if changed_bells {
            for callback in self.0.imported.borrow().iter() {
                callback();
            }
        }
        Ok(true)
    }

    pub(super) fn remember(&self, change: Changes) {
        let mut history = self.0.history.borrow_mut();
        history.redo.clear();
        if history.undo.len() == 32 {
            history.undo.pop_front();
        }
        history.undo.push_back(change);
    }

    /// Refresh the UI after a journaled external commit has already replaced the file.
    pub fn adopt_durable_schedule(&self, data: SchoolData) -> bool {
        if *self.data() == data {
            return false;
        }
        *self.0.data.borrow_mut() = data;
        self.0.revision.set(self.revision().wrapping_add(1));
        self.reset_history_after_import();
        true
    }

    pub fn import(&self, mut data: SchoolData) -> Result<(), String> {
        if data.bells.is_none() {
            data.bells.clone_from(&self.data().bells);
        }
        self.commit(data)?;
        self.reset_history_after_import();
        Ok(())
    }

    fn reset_history_after_import(&self) {
        self.0.generation.set(self.generation().wrapping_add(1));
        *self.0.history.borrow_mut() = History::default();
        for callback in self.0.imported.borrow().iter() {
            callback();
        }
    }
}

/// With a retained common export, absent records represent observed deletions.
/// Diverging edits require a choice even when one branch deleted the record.
pub fn conflicts(base: &SchoolData, local: &SchoolData, incoming: &SchoolData) -> BTreeSet<Key> {
    let left = Changes::between(base, local).keys();
    let right = Changes::between(base, incoming).keys();
    let different = Changes::between(local, incoming).keys();
    left.intersection(&right)
        .filter(|key| different.contains(key))
        .cloned()
        .collect()
}
