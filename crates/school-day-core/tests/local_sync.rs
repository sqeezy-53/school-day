use school_day_core::lessons::sync::{Checkpoint, Error, MergePlan, Prepared, Side, Snapshot};
use school_day_core::lessons::{Key, RecordId, SchoolData};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::PathBuf};

fn id(n: u128) -> RecordId {
    RecordId::from_u128(n)
}

fn lesson(n: u128, hour: u32) -> Value {
    json!({
        "id": id(n), "title": "Алгебра", "date": "2031-09-01",
        "time": format!("{hour:02}:00:00"), "end": format!("{hour:02}:40:00"),
    })
}

fn task(n: u128) -> Value {
    json!({"id": id(n), "title": "Алгебра", "body": "Упражнение 1"})
}

fn read(value: Value) -> SchoolData {
    SchoolData::decode(&serde_json::to_vec(&value).unwrap())
        .unwrap()
        .0
}

fn document(lessons: Vec<Value>, tasks: Vec<Value>) -> SchoolData {
    read(json!({"format_version": 1, "lessons": lessons, "pending_homework": tasks}))
}

fn snapshot(n: u128, checkpoint: Option<RecordId>, document: &SchoolData) -> Snapshot {
    Snapshot::new(id(n), checkpoint, document).unwrap()
}

fn review(local: &Snapshot, remote: &Snapshot, base: Option<&Checkpoint>) -> Checkpoint {
    MergePlan::compare(base, local, remote)
        .unwrap()
        .resolve(&BTreeMap::new())
        .unwrap()
        .checkpoint()
        .clone()
}

#[test]
fn first_exchange_merges_distinct_ids_and_resolves_equal_ids_only_once() {
    let left = document(vec![lesson(10, 8), lesson(20, 9)], vec![]);
    let right = document(vec![lesson(20, 9), lesson(30, 10)], vec![task(40)]);
    let local = snapshot(1, None, &left);
    let remote = snapshot(2, None, &right);
    let plan = MergePlan::compare(None, &local, &remote).unwrap();
    assert_eq!(plan.review().matching, 1);
    assert_eq!(plan.review().incoming.added, 2);
    assert_eq!(plan.review().outgoing.added, 1);
    assert_eq!(plan.review().conflicts, 0);
    let prepared = plan.resolve(&BTreeMap::new()).unwrap();
    let a = prepared.schedule_for(&local, &left).unwrap().unwrap();
    let b = prepared.schedule_for(&remote, &right).unwrap().unwrap();
    assert!(a == b);
    assert_eq!(a.counts(), (3, 1, 0));

    let mut changed = lesson(20, 9);
    changed["room"] = json!("42");
    let remote_document = document(vec![changed], vec![]);
    let remote = snapshot(2, None, &remote_document);
    let plan = MergePlan::compare(None, &local, &remote).unwrap();
    let key = Key::Lesson(id(20));
    assert_eq!(plan.conflicts().collect::<Vec<_>>(), vec![&key]);
    assert_eq!(
        plan.conflict_version(&key, Side::Incoming)
            .unwrap()
            .unwrap()["room"],
        "42"
    );
    assert!(matches!(
        plan.resolve(&BTreeMap::new()),
        Err(Error::MissingDecisions)
    ));
}

struct TestFolder(PathBuf);

impl TestFolder {
    fn new() -> Self {
        let folder = std::env::temp_dir().join(format!("school-day-sync-{}", RecordId::new_v4()));
        fs::create_dir(&folder).unwrap();
        Self(folder)
    }
}

impl Drop for TestFolder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct TestDevice {
    device: u128,
    directory: PathBuf,
}

impl TestDevice {
    fn new(root: &TestFolder, device: u128, initial: &SchoolData, checkpoint: &Checkpoint) -> Self {
        let directory = root.0.join(device.to_string());
        fs::create_dir(&directory).unwrap();
        let device = Self { device, directory };
        device.save(initial, checkpoint, false).unwrap();
        device
    }

    fn state(&self) -> (SchoolData, Checkpoint, Snapshot) {
        let value: Value = serde_json::from_slice(&self.saved()).unwrap();
        let checkpoint =
            Checkpoint::decode(&serde_json::to_vec(&value["checkpoint"]).unwrap()).unwrap();
        let data = read(value["schedule"].clone());
        let sync = snapshot(self.device, Some(checkpoint.id()), &data);
        (data, checkpoint, sync)
    }

    fn saved(&self) -> Vec<u8> {
        fs::read(self.directory.join("state.json")).unwrap()
    }

    fn save(
        &self,
        schedule: &SchoolData,
        checkpoint: &Checkpoint,
        fail: bool,
    ) -> std::io::Result<()> {
        let checkpoint: Value = serde_json::from_slice(&checkpoint.encode().unwrap()).unwrap();
        let bytes =
            serde_json::to_vec(&json!({"checkpoint": checkpoint, "schedule": schedule})).unwrap();
        let temp = self.directory.join("incoming.json");
        // The candidate is persisted under a temporary name. Simulate a write failure
        // before the durable state is replaced.
        fs::write(&temp, bytes)?;
        if fail {
            return Err(std::io::Error::other("simulated failure before rename"));
        }
        fs::OpenOptions::new().write(true).open(&temp)?.sync_all()?;
        fs::rename(temp, self.directory.join("state.json"))
    }

    fn apply(&self, prepared: &Prepared<'_>, fail: bool) -> std::io::Result<bool> {
        let (document, _, current) = self.state();
        let Some(next) = prepared.schedule_for(&current, &document).unwrap() else {
            return Ok(false);
        };
        self.save(&next, prepared.checkpoint(), fail)?;
        Ok(true)
    }
}

#[test]
fn isolated_profiles_retry_after_interruption_and_atomic_write_failure() {
    let folder = TestFolder::new();
    let mut linked = lesson(10, 8);
    linked["homework"] = json!("Упражнение 2");
    linked["homework_id"] = json!(id(11));
    let original = document(vec![linked.clone()], vec![task(40)]);
    let base = review(
        &snapshot(1, None, &original),
        &snapshot(2, None, &original),
        None,
    );
    let a = TestDevice::new(&folder, 1, &original, &base);
    let b = TestDevice::new(&folder, 2, &original, &base);

    linked["note"] = json!("Принести тетрадь");
    linked["options"] = json!({"cancelled": true, "replacement": "История", "work": "test"});
    let mut left =
        serde_json::to_value(document(vec![linked, lesson(20, 9)], vec![task(40)])).unwrap();
    left["days"] = json!({"2031-09-01":{"remote": true,"short_minutes":30}});
    left["bells"] = json!({"first_start":"08:15:00","lesson_minutes":40,"breaks":[10]});
    let mut pending = task(40);
    pending["completed"] = json!(true);
    let mut right = serde_json::to_value(document(
        vec![
            serde_json::to_value(&original).unwrap()["lessons"][0].clone(),
            lesson(30, 10),
        ],
        vec![pending],
    ))
    .unwrap();
    right["bells"] = json!({"first_start":"09:00:00","lesson_minutes":45,"breaks":[10]});
    a.save(&read(left), &base, false).unwrap();
    b.save(&read(right), &base, false).unwrap();
    let (a_document, base, local) = a.state();
    let (_, _, remote) = b.state();

    let packet = remote.encode().unwrap();
    assert!(matches!(
        Snapshot::decode(&packet[..packet.len() - 12]),
        Err(Error::InvalidPacket)
    ));
    let remote = Snapshot::decode(&packet).unwrap();
    let plan = MergePlan::compare(Some(&base), &local, &remote).unwrap();
    assert_eq!(plan.review().conflicts, 0);
    let prepared = plan.resolve(&BTreeMap::new()).unwrap();

    let before = b.saved();
    assert!(a.apply(&prepared, false).unwrap());
    assert!(b.apply(&prepared, true).is_err());
    assert_eq!(b.saved(), before);
    let (_, _, a_now) = a.state();
    assert!(matches!(
        MergePlan::compare(Some(prepared.checkpoint()), &a_now, &remote),
        Err(Error::DifferentCheckpoint)
    ));
    assert!(b.apply(&prepared, false).unwrap());
    assert!(!a.apply(&prepared, false).unwrap());
    assert!(!b.apply(&prepared, false).unwrap());

    let (a_result, agreed, a_wire) = a.state();
    let (b_result, _, b_wire) = b.state();
    let repeated = MergePlan::compare(Some(&agreed), &a_wire, &b_wire).unwrap();
    assert_eq!(repeated.review().outgoing.added, 0);
    assert_eq!(
        repeated
            .resolve(&BTreeMap::new())
            .unwrap()
            .checkpoint()
            .id(),
        agreed.id()
    );
    assert_eq!(a_result.counts(), (3, 2, 1));
    assert_eq!(a_result.bells, a_document.bells);
    assert_ne!(a_result.bells, b_result.bells);
    let value = serde_json::to_value(a_result).unwrap();
    assert_eq!(value["lessons"][0]["homework_id"], json!(id(11)));
    assert_eq!(value["lessons"][0]["note"], "Принести тетрадь");
    assert_eq!(value["lessons"][0]["options"]["cancelled"], true);
    assert_eq!(value["pending_homework"][0]["completed"], true);
    assert_eq!(value["days"]["2031-09-01"]["remote"], true);
}

#[test]
fn deletion_versus_edit_and_tombstones_require_explicit_choice() {
    let original = document(vec![lesson(10, 8)], vec![task(20)]);
    let base = review(
        &snapshot(1, None, &original),
        &snapshot(2, None, &original),
        None,
    );
    let removed = document(vec![lesson(10, 8)], vec![]);
    let mut changed = task(20);
    changed["body"] = json!("Другая версия");
    let modified = document(vec![lesson(10, 8)], vec![changed]);
    let local = snapshot(1, Some(base.id()), &removed);
    let remote = snapshot(2, Some(base.id()), &modified);
    let key = Key::Task(id(20));
    let plan = MergePlan::compare(Some(&base), &local, &remote).unwrap();
    assert_eq!(plan.conflicts().collect::<Vec<_>>(), vec![&key]);
    assert!(plan.conflict_version(&key, Side::Local).unwrap().is_none());
    assert_eq!(
        plan.conflict_version(&key, Side::Incoming)
            .unwrap()
            .unwrap()["body"],
        "Другая версия"
    );
    let prepared = plan
        .resolve(&BTreeMap::from([(key.clone(), Side::Local)]))
        .unwrap();
    let result = prepared.schedule_for(&remote, &modified).unwrap().unwrap();
    assert_eq!(result.counts(), (1, 0, 0));
    let stored = Checkpoint::decode(&prepared.checkpoint().encode().unwrap()).unwrap();
    let accepted = snapshot(1, Some(stored.id()), &result);
    assert!(matches!(
        MergePlan::compare(Some(&stored), &accepted, &remote),
        Err(Error::DifferentCheckpoint)
    ));
    let reintroduced = snapshot(2, Some(stored.id()), &original);
    let plan = MergePlan::compare(Some(&stored), &accepted, &reintroduced).unwrap();
    assert_eq!(plan.conflicts().collect::<Vec<_>>(), vec![&key]);
    assert!(matches!(
        plan.resolve(&BTreeMap::new()),
        Err(Error::MissingDecisions)
    ));
    assert!(matches!(
        prepared.schedule_for(&remote, &original),
        Err(Error::ScheduleChanged)
    ));

    let unchanged = snapshot(2, Some(base.id()), &original);
    let unilateral = MergePlan::compare(Some(&base), &local, &unchanged).unwrap();
    assert_eq!(unilateral.review().outgoing.deleted, 1);
    assert_eq!(unilateral.review().conflicts, 0);
    assert!(unilateral.resolve(&BTreeMap::new()).is_ok());
}

#[test]
fn rejects_bad_protocol_format_ids_and_merges_that_would_break_invariants() {
    let left = document(vec![lesson(10, 8)], vec![]);
    let local = snapshot(1, None, &left);
    let packet = local.encode().unwrap();
    let mut value: Value = serde_json::from_slice(&packet).unwrap();
    value["protocol"] = json!(99);
    assert!(matches!(
        Snapshot::decode(&serde_json::to_vec(&value).unwrap()),
        Err(Error::UnsupportedProtocol)
    ));
    value["protocol"] = json!(1);
    value["document"]["format_version"] = json!(0);
    assert!(matches!(
        Snapshot::decode(&serde_json::to_vec(&value).unwrap()),
        Err(Error::UnsupportedDataFormat)
    ));
    value["document"]["format_version"] = json!(1);
    value["document"]["lessons"][0]["id"] = json!(id(0));
    assert!(matches!(
        Snapshot::decode(&serde_json::to_vec(&value).unwrap()),
        Err(Error::InvalidSchedule(_))
    ));
    assert!(matches!(
        MergePlan::compare(None, &local, &local),
        Err(Error::WrongDevices)
    ));

    let right = document(vec![lesson(20, 8)], vec![]);
    let remote = snapshot(2, None, &right);
    assert!(matches!(
        MergePlan::compare(None, &local, &remote)
            .unwrap()
            .resolve(&BTreeMap::new()),
        Err(Error::InvalidSchedule(_))
    ));

    let mut attached = lesson(10, 8);
    attached["homework_id"] = json!(id(40));
    attached["homework"] = json!("Упражнение 1");
    let local = snapshot(1, None, &document(vec![attached], vec![]));
    let remote = snapshot(2, None, &document(vec![], vec![task(40)]));
    assert!(matches!(
        MergePlan::compare(None, &local, &remote)
            .unwrap()
            .resolve(&BTreeMap::new()),
        Err(Error::InvalidSchedule(_))
    ));
}

#[test]
#[ignore = "run with --release --ignored --nocapture for local merge timing"]
fn measure_exchange_core() {
    for size in [40u128, 10_000] {
        let data = document(
            vec![lesson(10, 8)],
            (0..size).map(|i| task(i + 100)).collect(),
        );
        let empty = SchoolData::default();
        let initial_left = snapshot(1, None, &data);
        let initial_right = snapshot(2, None, &empty);
        let mut fresh = Vec::new();
        let mut repeat = Vec::new();
        for _ in 0..7 {
            let start = std::time::Instant::now();
            let encoded = initial_left.encode().unwrap();
            let from_wire = Snapshot::decode(&encoded).unwrap();
            let prepared = MergePlan::compare(None, &initial_right, &from_wire)
                .unwrap()
                .resolve(&BTreeMap::new())
                .unwrap();
            let result = prepared
                .schedule_for(&initial_right, &empty)
                .unwrap()
                .unwrap();
            fresh.push(start.elapsed().as_secs_f64() * 1_000.0);
            let base = prepared.checkpoint();
            let left = snapshot(1, Some(base.id()), &data);
            let right = snapshot(2, Some(base.id()), &result);
            let start = std::time::Instant::now();
            let again = MergePlan::compare(Some(base), &left, &right)
                .unwrap()
                .resolve(&BTreeMap::new())
                .unwrap();
            assert_eq!(again.checkpoint().id(), base.id());
            assert!(again.schedule_for(&right, &result).unwrap().is_none());
            repeat.push(start.elapsed().as_secs_f64() * 1_000.0);
        }
        fresh.sort_by(f64::total_cmp);
        repeat.sort_by(f64::total_cmp);
        println!(
            "{size} tasks: first prepare {:.2} ms, no-change {:.2} ms, median of 7",
            fresh[3], repeat[3]
        );
    }
}
