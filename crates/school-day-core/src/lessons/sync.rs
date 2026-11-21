//! Compare two schedules against an agreed checkpoint. The caller must authenticate both
//! peers and durably agree on that checkpoint. Device IDs and checkpoint IDs are not credentials.

use super::{
    DayOptions, FORMAT_VERSION, Key, Lesson, MAX_BYTES, PendingHomework, RecordId, SchoolData,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

const PROTOCOL: u32 = 1;
const MAX_PACKET: usize = MAX_BYTES + 512;
const MAX_CHECKPOINT: usize = 2 * MAX_BYTES;

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    InvalidPacket,
    UnsupportedProtocol,
    UnsupportedDataFormat,
    WrongDevices,
    DifferentCheckpoint,
    InvalidSchedule(String),
    MissingDecisions,
    InvalidDecisions,
    ScheduleChanged,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidPacket => "Неполный или повреждённый пакет синхронизации",
            Self::UnsupportedProtocol => "Несовместимая версия протокола синхронизации",
            Self::UnsupportedDataFormat => "Несовместимый формат расписания",
            Self::WrongDevices => "Этот обмен предназначен для другой пары устройств",
            Self::DifferentCheckpoint => "Устройства опираются на разные подтверждённые версии",
            Self::InvalidSchedule(error) => {
                return write!(f, "Не удалось объединить расписание: {error}");
            }
            Self::MissingDecisions => "Выберите версию каждой конфликтующей записи",
            Self::InvalidDecisions => "Выбор относится к другой версии списка конфликтов",
            Self::ScheduleChanged => "Расписание изменилось. Сравните версии заново",
        })
    }
}

impl std::error::Error for Error {}

#[derive(Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    protocol: u32,
    device: RecordId,
    checkpoint: Option<RecordId>,
    document: SchoolData,
}

impl Snapshot {
    /// Project only syncable data. Local bell settings remain in the device's document.
    pub fn new(
        device: RecordId,
        checkpoint: Option<RecordId>,
        schedule: &SchoolData,
    ) -> Result<Self, Error> {
        let mut document = schedule.clone();
        document.bells = None;
        Self {
            protocol: PROTOCOL,
            device,
            checkpoint,
            document,
        }
        .validate()
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() > MAX_PACKET {
            return Err(Error::InvalidPacket);
        }
        serde_json::from_slice::<Self>(bytes)
            .map_err(|_| Error::InvalidPacket)?
            .validate()
    }

    pub fn encode(&self) -> Result<Vec<u8>, Error> {
        encode(self, MAX_PACKET)
    }

    pub(crate) fn device(&self) -> RecordId {
        self.device
    }

    pub fn checkpoint(&self) -> Option<RecordId> {
        self.checkpoint
    }

    fn validate(mut self) -> Result<Self, Error> {
        if self.protocol != PROTOCOL {
            return Err(Error::UnsupportedProtocol);
        }
        if self.device.is_nil() || self.checkpoint.is_some_and(|id| id.is_nil()) {
            return Err(Error::InvalidPacket);
        }
        self.document = validate_document(self.document)?;
        Ok(self)
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Checkpoint {
    protocol: u32,
    id: RecordId,
    devices: [RecordId; 2],
    document: SchoolData,
    deleted: BTreeSet<Key>,
}

impl Checkpoint {
    pub fn id(&self) -> RecordId {
        self.id
    }

    pub(crate) fn devices(&self) -> [RecordId; 2] {
        self.devices
    }

    pub(crate) fn matches_schedule(&self, schedule: &SchoolData) -> bool {
        self.document.lessons == schedule.lessons
            && self.document.pending_homework == schedule.pending_homework
            && self.document.days == schedule.days
    }

    pub(crate) fn schedule(&self, local: &SchoolData) -> SchoolData {
        let mut document = self.document.clone();
        document.bells.clone_from(&local.bells);
        document
    }

    pub fn encode(&self) -> Result<Vec<u8>, Error> {
        encode(self, MAX_CHECKPOINT)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() > MAX_CHECKPOINT {
            return Err(Error::InvalidPacket);
        }
        let mut state: Self = serde_json::from_slice(bytes).map_err(|_| Error::InvalidPacket)?;
        if state.protocol != PROTOCOL {
            return Err(Error::UnsupportedProtocol);
        }
        if state.id.is_nil() || state.devices[0].is_nil() || state.devices[0] >= state.devices[1] {
            return Err(Error::InvalidPacket);
        }
        state.document = validate_document(state.document)?;
        let live = persistent_ids(&state.document);
        if state.deleted.iter().any(|key| {
            let id = match key {
                Key::Lesson(id) | Key::Task(id) => id,
                _ => return true,
            };
            id.is_nil() || live.contains(key)
        }) {
            return Err(Error::InvalidPacket);
        }
        Ok(state)
    }
}

fn encode(value: &impl Serialize, limit: usize) -> Result<Vec<u8>, Error> {
    let bytes = serde_json::to_vec(value).map_err(|_| Error::InvalidPacket)?;
    if bytes.len() > limit {
        return Err(Error::InvalidPacket);
    }
    Ok(bytes)
}

fn validate_document(document: SchoolData) -> Result<SchoolData, Error> {
    if document.format_version != FORMAT_VERSION || document.bells.is_some() {
        return Err(Error::UnsupportedDataFormat);
    }
    let bytes = encode(&document, MAX_BYTES)?;
    let (mut document, _) = SchoolData::decode(&bytes).map_err(Error::InvalidSchedule)?;
    document.pending_homework.sort_by_key(|task| task.id);
    Ok(document)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side {
    Local,
    Incoming,
}

#[derive(Clone, Copy, PartialEq)]
enum Record<'a> {
    Lesson(&'a Lesson),
    Task(&'a PendingHomework),
    Day(&'a DayOptions),
}

fn records(data: &SchoolData) -> BTreeMap<Key, Record<'_>> {
    data.lessons
        .iter()
        .map(|lesson| (Key::Lesson(lesson.id), Record::Lesson(lesson)))
        .chain(
            data.pending_homework
                .iter()
                .map(|task| (Key::Task(task.id), Record::Task(task))),
        )
        .chain(
            data.days
                .iter()
                .map(|(date, options)| (Key::Day(*date), Record::Day(options))),
        )
        .collect()
}

fn persistent_ids(data: &SchoolData) -> BTreeSet<Key> {
    data.lessons
        .iter()
        .flat_map(|lesson| {
            [
                Some(Key::Lesson(lesson.id)),
                lesson.homework_id.map(Key::Task),
            ]
            .into_iter()
            .flatten()
        })
        .chain(data.pending_homework.iter().map(|task| Key::Task(task.id)))
        .collect()
}

#[derive(Clone, Copy, Default, Serialize)]
pub struct Delta {
    pub added: usize,
    pub changed: usize,
    pub deleted: usize,
}

impl Delta {
    fn compare(&mut self, from: Option<Record<'_>>, to: Option<Record<'_>>) {
        match (from, to) {
            (None, Some(_)) => self.added += 1,
            (Some(_), None) => self.deleted += 1,
            (Some(from), Some(to)) if from != to => self.changed += 1,
            _ => {}
        }
    }
}

pub struct Review {
    pub incoming: Delta,
    pub outgoing: Delta,
    pub matching: usize,
    pub conflicts: usize,
}

struct Entry<'a> {
    local: Option<Record<'a>>,
    incoming: Option<Record<'a>>,
    proposed: Option<Side>,
}

impl<'a> Entry<'a> {
    fn for_side(&self, side: Side) -> Option<Record<'a>> {
        match side {
            Side::Local => self.local,
            Side::Incoming => self.incoming,
        }
    }
}

pub struct MergePlan<'a> {
    devices: [RecordId; 2],
    base: Option<&'a Checkpoint>,
    local: &'a Snapshot,
    incoming: &'a Snapshot,
    entries: BTreeMap<Key, Entry<'a>>,
    review: Review,
}

impl<'a> MergePlan<'a> {
    pub fn compare(
        base: Option<&'a Checkpoint>,
        local: &'a Snapshot,
        incoming: &'a Snapshot,
    ) -> Result<Self, Error> {
        let mut devices = [local.device, incoming.device];
        devices.sort_unstable();
        if devices[0] == devices[1] || base.is_some_and(|state| state.devices != devices) {
            return Err(Error::WrongDevices);
        }
        let expected = base.map(Checkpoint::id);
        if local.checkpoint != expected || incoming.checkpoint != expected {
            return Err(Error::DifferentCheckpoint);
        }
        let left = records(&local.document);
        let right = records(&incoming.document);
        let previous = base
            .map(|state| records(&state.document))
            .unwrap_or_default();
        let keys: BTreeSet<_> = left
            .keys()
            .chain(right.keys())
            .chain(previous.keys())
            .cloned()
            .collect();
        let mut review = Review {
            incoming: Delta::default(),
            outgoing: Delta::default(),
            matching: 0,
            conflicts: 0,
        };
        let mut entries = BTreeMap::new();
        for key in keys {
            let local = left.get(&key).copied();
            let incoming = right.get(&key).copied();
            let previous = previous.get(&key).copied();
            let restores_deleted_record = base.is_some_and(|state| {
                state.deleted.contains(&key) ||
                    [local, incoming].into_iter().any(|record| {
                        matches!(record, Some(Record::Lesson(lesson))
                            if lesson.homework_id.is_some_and(|id| state.deleted.contains(&Key::Task(id))))
                    })
            });
            let proposed = if restores_deleted_record {
                None
            } else if local == incoming {
                review.matching += usize::from(local.is_some());
                Some(Side::Local)
            } else if local == previous {
                review.incoming.compare(local, incoming);
                Some(Side::Incoming)
            } else if incoming == previous {
                review.outgoing.compare(incoming, local);
                Some(Side::Local)
            } else {
                None
            };
            review.conflicts += usize::from(proposed.is_none());
            entries.insert(
                key,
                Entry {
                    local,
                    incoming,
                    proposed,
                },
            );
        }
        Ok(Self {
            devices,
            base,
            local,
            incoming,
            entries,
            review,
        })
    }

    pub fn review(&self) -> &Review {
        &self.review
    }

    pub fn conflicts(&self) -> impl Iterator<Item = &Key> {
        self.entries
            .iter()
            .filter_map(|(key, entry)| entry.proposed.is_none().then_some(key))
    }

    /// An absent value means that side removed the record.
    pub fn conflict_version(
        &self,
        key: &Key,
        side: Side,
    ) -> Result<Option<serde_json::Value>, Error> {
        let entry = self
            .entries
            .get(key)
            .filter(|entry| entry.proposed.is_none())
            .ok_or(Error::InvalidDecisions)?;
        match entry.for_side(side) {
            Some(Record::Lesson(record)) => serde_json::to_value(record),
            Some(Record::Task(record)) => serde_json::to_value(record),
            Some(Record::Day(record)) => serde_json::to_value(record),
            None => return Ok(None),
        }
        .map(Some)
        .map_err(|_| Error::InvalidPacket)
    }

    pub fn resolve(&self, choices: &BTreeMap<Key, Side>) -> Result<Prepared<'a>, Error> {
        self.resolve_proposal(choices, RecordId::new_v4())
    }

    pub(crate) fn resolve_proposal(
        &self,
        choices: &BTreeMap<Key, Side>,
        proposal: RecordId,
    ) -> Result<Prepared<'a>, Error> {
        if proposal.is_nil() {
            return Err(Error::InvalidDecisions);
        }
        if choices.keys().any(|key| {
            self.entries
                .get(key)
                .is_none_or(|entry| entry.proposed.is_some())
        }) {
            return Err(Error::InvalidDecisions);
        }
        let mut document = SchoolData::default();
        for (key, entry) in &self.entries {
            let choice = entry
                .proposed
                .or_else(|| choices.get(key).copied())
                .ok_or(Error::MissingDecisions)?;
            match entry.for_side(choice) {
                Some(Record::Lesson(lesson)) => document.lessons.push(lesson.clone()),
                Some(Record::Task(task)) => document.pending_homework.push(task.clone()),
                Some(Record::Day(options)) => {
                    if let Key::Day(day) = key {
                        document.days.insert(*day, *options);
                    }
                }
                None => {}
            }
        }
        // Independent edits can still overlap or assign the same homework ID
        // to two places. Reject the whole candidate through the existing format validator.
        let document = validate_document(document)?;
        let mut deleted = self
            .base
            .map(|state| state.deleted.clone())
            .unwrap_or_default();
        for branch in [self.local, self.incoming] {
            deleted.extend(persistent_ids(&branch.document));
        }
        if let Some(state) = self.base {
            deleted.extend(persistent_ids(&state.document));
        }
        let live = persistent_ids(&document);
        deleted.retain(|key| !live.contains(key));
        let id = match self.base {
            Some(state) if state.document == document && state.deleted == deleted => state.id,
            _ => proposal,
        };
        let checkpoint = Checkpoint {
            protocol: PROTOCOL,
            id,
            devices: self.devices,
            document,
            deleted,
        };
        checkpoint.encode()?;
        Ok(Prepared {
            checkpoint,
            local: self.local,
            incoming: self.incoming,
        })
    }
}

pub struct Prepared<'a> {
    checkpoint: Checkpoint,
    local: &'a Snapshot,
    incoming: &'a Snapshot,
}

impl Prepared<'_> {
    pub fn checkpoint(&self) -> &Checkpoint {
        &self.checkpoint
    }

    /// Call against a fresh snapshot before committing. The caller saves the returned
    /// schedule and the checkpoint together, or records an interrupted commit for recovery.
    /// Replaying an already installed exchange produces no write.
    pub fn schedule_for(
        &self,
        current: &Snapshot,
        current_schedule: &SchoolData,
    ) -> Result<Option<SchoolData>, Error> {
        let reviewed = if current.device == self.local.device {
            self.local
        } else if current.device == self.incoming.device {
            self.incoming
        } else {
            return Err(Error::WrongDevices);
        };
        let fresh = Snapshot::new(current.device, current.checkpoint, current_schedule)?;
        if current != &fresh {
            return Err(Error::ScheduleChanged);
        }
        if current.checkpoint == Some(self.checkpoint.id)
            && current.document == self.checkpoint.document
        {
            return Ok(None);
        }
        if current != reviewed {
            return Err(Error::ScheduleChanged);
        }
        Ok(Some(self.checkpoint.schedule(current_schedule)))
    }
}
