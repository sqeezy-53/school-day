use super::profile::{Pending, State};
use super::transport::Channel;
use super::{Cancel, Error, Profile, Result};
use crate::lessons::sync::{Delta, MergePlan, Side, Snapshot};
use crate::lessons::{Key, MAX_BYTES, RecordId, SchoolData};
use ring::digest::{SHA256, digest};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, net::TcpListener};

const SNAPSHOT_LIMIT: usize = MAX_BYTES + 512;
const CHOICES_LIMIT: usize = 64 * 1024;
const MAX_CONFLICTS: usize = 512;
const MASS_DELETION: usize = 10;

const STATUS: u8 = 10;
const SNAPSHOT: u8 = 11;
const PROPOSE: u8 = 12;
const READY: u8 = 13;
const DECISION: u8 = 14;
const APPLIED: u8 = 15;
const DONE: u8 = 16;
const PREFERRED: u8 = 17;
const REVIEWED: u8 = 18;

#[derive(Clone, Serialize)]
pub struct Conflict {
    pub desktop: Option<serde_json::Value>,
    pub android: Option<serde_json::Value>,
}

#[derive(Clone, Serialize)]
pub struct Review {
    pub incoming: Delta,
    pub outgoing: Delta,
    pub conflicts: Vec<Conflict>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Synchronized,
    Resumed,
    Cancelled,
    Unchanged,
    ReviewRequired,
    AwaitingReceipt,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SessionMode {
    Interactive,
    Automatic,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Status {
    device: RecordId,
    checkpoint: Option<RecordId>,
    pending: Option<PendingStatus>,
    receipt: Option<RecordId>,
    clean: bool,
    interactive: bool,
}

impl Status {
    fn new(device: RecordId, state: &State, data: &SchoolData, mode: SessionMode) -> Self {
        Self {
            device,
            checkpoint: state.checkpoint.as_ref().map(|cp| cp.id()),
            pending: state.pending.as_ref().map(PendingStatus::from),
            receipt: state.receipt,
            clean: state
                .checkpoint
                .as_ref()
                .is_some_and(|cp| cp.matches_schedule(data)),
            interactive: mode == SessionMode::Interactive,
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PendingStatus {
    id: RecordId,
    commit: bool,
}
impl From<&Pending> for PendingStatus {
    fn from(pending: &Pending) -> Self {
        Self {
            id: pending.id,
            commit: pending.commit,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum Action {
    Start,
    Abort(RecordId),
    Commit(RecordId),
    Cancel,
    Unchanged,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Proposal {
    id: RecordId,
    choices: Vec<Side>,
    digest: [u8; 32],
}

fn view(plan: &MergePlan<'_>) -> Result<Review> {
    if plan.review().conflicts > MAX_CONFLICTS {
        return Err(Error::TooManyConflicts);
    }
    let conflicts = plan
        .conflicts()
        .map(|key| {
            Ok(Conflict {
                desktop: plan.conflict_version(key, Side::Local)?,
                android: plan.conflict_version(key, Side::Incoming)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Review {
        incoming: plan.review().incoming,
        outgoing: plan.review().outgoing,
        conflicts,
    })
}

fn requires_review(plan: &MergePlan<'_>, initial: bool) -> bool {
    let review = plan.review();
    initial
        || review.conflicts != 0
        || review.incoming.deleted.max(review.outgoing.deleted) >= MASS_DELETION
}

fn decisions(plan: &MergePlan<'_>, choices: &[Side]) -> Result<BTreeMap<Key, Side>> {
    if choices.len() != plan.review().conflicts {
        return Err(Error::InvalidMessage);
    }
    Ok(plan
        .conflicts()
        .cloned()
        .zip(choices.iter().copied())
        .collect())
}

fn fingerprint(data: &[u8]) -> [u8; 32] {
    let mut value = [0; 32];
    value.copy_from_slice(digest(&SHA256, data).as_ref());
    value
}

fn send_choices(link: &mut Channel, kind: u8, choices: Option<&[Side]>) -> Result<()> {
    let bytes = serde_json::to_vec(&choices).map_err(|_| Error::InvalidMessage)?;
    link.send(kind, &bytes, CHOICES_LIMIT)
}

fn receive_choices(link: &mut Channel, kind: u8) -> Result<Option<Vec<Side>>> {
    serde_json::from_slice(&link.receive(kind, CHOICES_LIMIT)?).map_err(|_| Error::InvalidMessage)
}

fn verify_status(status: &Status, expected: RecordId) -> Result<()> {
    if status.device != expected
        || status.pending.as_ref().is_some_and(|p| p.id.is_nil())
        || status.checkpoint.is_some_and(|v| v.is_nil())
        || status.receipt.is_some_and(|v| v.is_nil())
    {
        return Err(Error::UntrustedDevice);
    }
    Ok(())
}

fn exact_ack(link: &mut Channel, kind: u8, id: RecordId) -> Result<()> {
    let ack: RecordId = link.receive_control(kind)?;
    if ack != id {
        return Err(Error::InvalidMessage);
    }
    Ok(())
}

impl Profile {
    /// Desktop coordinates a single authenticated exchange. The application holds an
    /// exclusive local sync session while the review is open.
    pub fn serve(
        &self,
        listener: TcpListener,
        cancel: Cancel,
        review: impl FnMut(&Review, Option<&[Side]>) -> Option<Vec<Side>>,
    ) -> Result<Outcome> {
        self.serve_in(listener, cancel, SessionMode::Interactive, || true, review)
    }

    /// The gate runs after peer authentication, before a recovery or exchange can
    /// change the local schedule. The caller can reserve its in-memory model here.
    pub fn serve_in(
        &self,
        listener: TcpListener,
        cancel: Cancel,
        mode: SessionMode,
        begin_write: impl FnOnce() -> bool,
        mut review: impl FnMut(&Review, Option<&[Side]>) -> Option<Vec<Side>>,
    ) -> Result<Outcome> {
        let _lease = self.session()?;
        let peer = self.peer()?.ok_or(Error::UntrustedDevice)?;
        if listener.local_addr()? != peer.endpoint().into() {
            return Err(Error::InvalidAddress);
        }
        let poll = if mode == SessionMode::Automatic {
            150
        } else {
            35
        };
        let mut link = Channel::accept(
            listener,
            &self.identity,
            &peer,
            cancel.clone(),
            std::time::Duration::from_millis(poll),
        )?;
        let mut state = self.state(&peer)?;
        let current = self.read_schedule()?;
        let local_status = Status::new(self.identity.id(), &state, &current, mode);
        link.send_control(STATUS, &local_status)?;
        let remote: Status = link.receive_control(STATUS)?;
        verify_status(&remote, peer.id())?;

        let recovery = recovery(&local_status, &remote)?;
        if recovery.is_none() && remote.checkpoint != local_status.checkpoint {
            return Err(Error::InvalidMessage);
        }
        if recovery.is_none() && local_status.clean && remote.clean {
            link.send_control(DECISION, &Action::Unchanged)?;
            return Ok(Outcome::Unchanged);
        }
        if !begin_write() {
            link.send_control(DECISION, &Action::Cancel)?;
            return Ok(Outcome::Cancelled);
        }
        if let Some(action) = recovery {
            cancel.check()?;
            return self.resume_host(&mut link, &mut state, action);
        }
        link.send_control(DECISION, &Action::Start)?;
        let local = Snapshot::new(self.identity.id(), local_status.checkpoint, &current)?;
        link.send(SNAPSHOT, &local.encode()?, SNAPSHOT_LIMIT)?;
        let incoming = Snapshot::decode(&link.receive(SNAPSHOT, SNAPSHOT_LIMIT)?)?;
        if incoming.device() != peer.id() {
            return Err(Error::UntrustedDevice);
        }
        let plan = MergePlan::compare(state.checkpoint.as_ref(), &local, &incoming)?;
        let review_required = requires_review(&plan, local_status.checkpoint.is_none());
        if review_required && !(local_status.interactive && remote.interactive) {
            return Ok(Outcome::ReviewRequired);
        }
        let summary = review_required.then(|| view(&plan)).transpose()?;
        let preferred = match &summary {
            Some(summary) => review(summary, None),
            None => Some(Vec::new()),
        };
        if let Some(preferred) = &preferred {
            decisions(&plan, preferred)?;
        }
        cancel.check()?;
        send_choices(&mut link, PREFERRED, preferred.as_deref())?;
        let Some(preferred) = preferred else {
            return Ok(Outcome::Cancelled);
        };
        let Some(choices) = receive_choices(&mut link, REVIEWED)? else {
            return Ok(Outcome::Cancelled);
        };
        let decisions = decisions(&plan, &choices)?;
        if choices != preferred
            && summary
                .as_ref()
                .and_then(|summary| review(summary, Some(&choices)))
                .as_deref()
                != Some(&choices)
        {
            link.send_control(PROPOSE, &Option::<Proposal>::None)?;
            return Ok(Outcome::Cancelled);
        }
        let id = RecordId::new_v4();
        let agreed = plan.resolve_proposal(&decisions, id)?.checkpoint().clone();
        let proposal = Proposal {
            id,
            choices,
            digest: fingerprint(&agreed.encode()?),
        };
        cancel.check()?;
        link.send(
            PROPOSE,
            &serde_json::to_vec(&Some(proposal)).map_err(|_| Error::InvalidMessage)?,
            CHOICES_LIMIT,
        )?;
        exact_ack(&mut link, READY, id)?;
        self.stage(&mut state, id, agreed, &current)?;
        cancel.check()?;
        // Durable decision and local write precede sending COMMIT. Recovery can
        // retransmit the same ID even if the socket closes immediately afterwards.
        self.commit(&mut state, id)?;
        self.resume_host(&mut link, &mut state, Action::Commit(id))?;
        Ok(Outcome::Synchronized)
    }

    pub fn join(
        &self,
        cancel: Cancel,
        review: impl FnOnce(&Review, &[Side]) -> Option<Vec<Side>>,
    ) -> Result<Outcome> {
        self.join_in(cancel, SessionMode::Interactive, review)
    }

    pub fn join_in(
        &self,
        cancel: Cancel,
        mode: SessionMode,
        review: impl FnOnce(&Review, &[Side]) -> Option<Vec<Side>>,
    ) -> Result<Outcome> {
        let _lease = self.session()?;
        let peer = self.peer()?.ok_or(Error::UntrustedDevice)?;
        let mut state = self.state(&peer)?;
        let mut link = Channel::connect(&self.identity, &peer, cancel.clone())?;
        let remote: Status = link.receive_control(STATUS)?;
        verify_status(&remote, peer.id())?;
        let current = self.read_schedule()?;
        let local = Status::new(self.identity.id(), &state, &current, mode);
        link.send_control(STATUS, &local)?;
        let action: Action = link.receive_control(DECISION)?;
        match action {
            Action::Start
                if remote.checkpoint == local.checkpoint
                    && remote.pending.is_none()
                    && local.pending.is_none() => {}
            Action::Start => return Err(Error::InvalidMessage),
            Action::Unchanged
                if local.clean
                    && remote.clean
                    && local.checkpoint == remote.checkpoint
                    && local.pending.is_none()
                    && remote.pending.is_none() =>
            {
                return Ok(Outcome::Unchanged);
            }
            action => return self.resume_client(&mut link, &mut state, action),
        }

        let desktop = Snapshot::decode(&link.receive(SNAPSHOT, SNAPSHOT_LIMIT)?)?;
        if desktop.device() != peer.id() {
            return Err(Error::UntrustedDevice);
        }
        let android = Snapshot::new(self.identity.id(), local.checkpoint, &current)?;
        link.send(SNAPSHOT, &android.encode()?, SNAPSHOT_LIMIT)?;
        let plan = MergePlan::compare(state.checkpoint.as_ref(), &desktop, &android)?;
        let review_required = requires_review(&plan, local.checkpoint.is_none());
        if review_required && !(local.interactive && remote.interactive) {
            return Ok(Outcome::ReviewRequired);
        }
        let Some(preferred) = receive_choices(&mut link, PREFERRED)? else {
            return Ok(Outcome::Cancelled);
        };
        decisions(&plan, &preferred)?;
        let choices = if review_required {
            review(&view(&plan)?, &preferred)
        } else {
            Some(Vec::new())
        };
        if let Some(choices) = &choices {
            decisions(&plan, choices)?;
        }
        cancel.check()?;
        send_choices(&mut link, REVIEWED, choices.as_deref())?;
        let Some(choices) = choices else {
            return Ok(Outcome::Cancelled);
        };
        let Some(proposal) =
            serde_json::from_slice::<Option<Proposal>>(&link.receive(PROPOSE, CHOICES_LIMIT)?)
                .map_err(|_| Error::InvalidMessage)?
        else {
            return Ok(Outcome::Cancelled);
        };
        if proposal.choices != choices {
            return Err(Error::InvalidMessage);
        }
        let agreed = plan
            .resolve_proposal(&decisions(&plan, &proposal.choices)?, proposal.id)?
            .checkpoint()
            .clone();
        if fingerprint(&agreed.encode()?) != proposal.digest {
            return Err(Error::InvalidMessage);
        }
        cancel.check()?;
        let id = proposal.id;
        self.stage(&mut state, id, agreed, &current)?;
        link.send_control(READY, &id)?;
        match link.receive_control::<Action>(DECISION)? {
            Action::Commit(received) if received == id => self
                .resume_client(&mut link, &mut state, Action::Commit(id))
                .map(|outcome| {
                    if outcome == Outcome::Resumed {
                        Outcome::Synchronized
                    } else {
                        outcome
                    }
                }),
            Action::Abort(received) if received == id => {
                self.resume_client(&mut link, &mut state, Action::Abort(id))
            }
            _ => Err(Error::InvalidMessage),
        }
    }

    fn resume_host(
        &self,
        link: &mut Channel,
        state: &mut State,
        action: Action,
    ) -> Result<Outcome> {
        match action {
            Action::Commit(id) => {
                self.commit(state, id)?;
                link.send_control(DECISION, &Action::Commit(id))?;
                exact_ack(link, APPLIED, id)?;
                self.finish(state, id)?;
                // Both writes are durable at this point. A lost final reply is
                // recoverable using the persisted receipt.
                let _ = link.send_control(DONE, &id);
                Ok(Outcome::Resumed)
            }
            Action::Abort(id) => {
                self.abort(state, id)?;
                link.send_control(DECISION, &Action::Abort(id))?;
                exact_ack(link, APPLIED, id)?;
                Ok(Outcome::Cancelled)
            }
            _ => Err(Error::InvalidMessage),
        }
    }

    fn resume_client(
        &self,
        link: &mut Channel,
        state: &mut State,
        action: Action,
    ) -> Result<Outcome> {
        match action {
            Action::Commit(id) => {
                self.commit(state, id)?;
                if let Err(reason) = link
                    .send_control(APPLIED, &id)
                    .and_then(|()| exact_ack(link, DONE, id))
                {
                    return match reason {
                        Error::Io(_) | Error::Deadline | Error::Cancelled => {
                            Ok(Outcome::AwaitingReceipt)
                        }
                        other => Err(other),
                    };
                }
                self.finish(state, id)?;
                Ok(Outcome::Resumed)
            }
            Action::Abort(id) => {
                self.abort(state, id)?;
                link.send_control(APPLIED, &id)?;
                Ok(Outcome::Cancelled)
            }
            Action::Cancel => Ok(Outcome::Cancelled),
            Action::Start | Action::Unchanged => Err(Error::InvalidMessage),
        }
    }
}

/// A participant in doubt obeys the same coordinator decision on reconnect.
/// A receipt is kept after clearing a completed journal to recover a lost final ACK.
fn recovery(host: &Status, client: &Status) -> Result<Option<Action>> {
    Ok(match (&host.pending, &client.pending) {
        (Some(h), Some(c)) if h.id == c.id && h.commit => Some(Action::Commit(h.id)),
        (Some(h), Some(c)) if h.id == c.id && !c.commit => Some(Action::Abort(h.id)),
        (Some(h), None) if h.commit && client.receipt == Some(h.id) => Some(Action::Commit(h.id)),
        (Some(h), None) if !h.commit => Some(Action::Abort(h.id)),
        (None, Some(c)) if host.receipt == Some(c.id) => Some(Action::Commit(c.id)),
        (None, Some(c)) if !c.commit => Some(Action::Abort(c.id)),
        (None, None) => None,
        _ => return Err(Error::InvalidMessage),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lan::{Invitation, ensure_writable};
    use crate::lessons::SchoolData;
    use serde_json::{Value, json};
    use std::fs;
    use std::path::{Path, PathBuf};

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("school-day-lan-{}", RecordId::new_v4()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn profile(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn paired(a: &Path, b: &Path) -> (Profile, Profile) {
        let host = Profile::open(a).unwrap();
        let client = Profile::open(b).unwrap();
        let offer = host.invite("127.0.0.1:0".parse().unwrap()).unwrap();
        let encoded = offer.invitation().encode();
        let invite = Invitation::decode(&encoded).unwrap();
        let (host_code, compare) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            let desktop = scope.spawn(|| {
                host.accept_pair(offer, Cancel::default(), |code| {
                    host_code.send(code.to_owned()).is_ok()
                })
            });
            client
                .connect_pair(invite, Cancel::default(), |code| {
                    compare
                        .recv_timeout(std::time::Duration::from_secs(4))
                        .is_ok_and(|host| host == code)
                })
                .unwrap();
            desktop.join().unwrap().unwrap();
        });
        assert!(host.peer().unwrap().is_some());
        assert!(client.peer().unwrap().is_some());
        (host, client)
    }

    fn save(path: &Path, lessons: Vec<Value>, tasks: Vec<Value>) {
        let document = json!({"format_version":1,"lessons":lessons,"pending_homework":tasks});
        fs::write(
            path.join("lessons.json"),
            serde_json::to_vec(&document).unwrap(),
        )
        .unwrap();
    }
    fn lesson(num: u128, hour: u8, note: &str) -> Value {
        json!({"id":RecordId::from_u128(num),"title": "Физика","date":"2031-10-12",
            "time":format!("{hour:02}:00:00"),"end":format!("{hour:02}:40:00"),"note":note})
    }

    fn exchange(
        host: &Profile,
        client: &Profile,
        decide: impl FnOnce(&Review) -> Option<Vec<Side>> + Send,
    ) -> (Outcome, Outcome) {
        let mut decide = Some(decide);
        exchange_with(
            host,
            client,
            move |view, updated| {
                assert!(updated.is_none());
                decide.take().unwrap()(view)
            },
            |_, choices| Some(choices.to_vec()),
        )
    }

    fn exchange_with(
        host: &Profile,
        client: &Profile,
        desktop: impl FnMut(&Review, Option<&[Side]>) -> Option<Vec<Side>> + Send,
        phone: impl FnOnce(&Review, &[Side]) -> Option<Vec<Side>>,
    ) -> (Outcome, Outcome) {
        let listener = TcpListener::bind(host.peer().unwrap().unwrap().endpoint()).unwrap();
        std::thread::scope(|scope| {
            let serve = scope.spawn(|| host.serve(listener, Cancel::default(), desktop));
            let join = client.join(Cancel::default(), phone).unwrap();
            (serve.join().unwrap().unwrap(), join)
        })
    }

    #[test]
    fn tls_pair_and_bidirectional_commit_survive_restart_and_conflict() {
        let f = Fixture::new();
        let a = f.profile("desktop");
        let b = f.profile("phone");
        let (host, client) = paired(&a, &b);
        let a_initial = lesson(10, 8, "ПК");
        let b_initial = lesson(20, 9, "Телефон");
        let task =
            json!({"id":RecordId::from_u128(30),"title":"История","body":"Тест","completed":false});
        save(&a, vec![a_initial.clone()], vec![task.clone()]);
        save(&b, vec![b_initial.clone()], vec![]);
        assert_eq!(
            exchange(&host, &client, |view| {
                assert!(view.conflicts.is_empty());
                assert_eq!(view.incoming.added, 1);
                assert_eq!(view.outgoing.added, 2);
                Some(vec![])
            }),
            (Outcome::Synchronized, Outcome::Synchronized)
        );
        assert_eq!(host.read_schedule().unwrap().counts(), (2, 1, 0));
        assert!(host.read_schedule().unwrap() == client.read_schedule().unwrap());

        drop((host, client));
        let host = Profile::open(&a).unwrap();
        let client = Profile::open(&b).unwrap();
        save(
            &a,
            vec![lesson(10, 8, "Версия ПК"), b_initial.clone()],
            vec![],
        );
        save(
            &b,
            vec![lesson(10, 8, "Версия телефона"), b_initial.clone()],
            vec![task],
        );
        assert_eq!(
            exchange_with(
                &host,
                &client,
                |view, updated| {
                    if let Some(choices) = updated {
                        assert_eq!(choices, &[Side::Incoming]);
                        return Some(choices.to_vec());
                    }
                    assert_eq!(view.conflicts.len(), 1);
                    assert_eq!(
                        view.conflicts[0].desktop.as_ref().unwrap()["note"],
                        "Версия ПК"
                    );
                    assert_eq!(
                        view.conflicts[0].android.as_ref().unwrap()["note"],
                        "Версия телефона"
                    );
                    Some(vec![Side::Local])
                },
                |_, preferred| {
                    assert_eq!(preferred, &[Side::Local]);
                    Some(vec![Side::Incoming])
                }
            ),
            (Outcome::Synchronized, Outcome::Synchronized)
        );
        let agreed = host.read_schedule().unwrap();
        assert!(agreed == client.read_schedule().unwrap());
        assert_eq!(agreed.counts(), (2, 0, 0));
        assert_eq!(
            serde_json::to_value(agreed).unwrap()["lessons"][0]["note"],
            "Версия телефона"
        );

        let first = fs::read(a.join("lessons.json")).unwrap();
        assert_eq!(
            exchange(&host, &client, |_| panic!(
                "An unchanged exchange opens no review"
            )),
            (Outcome::Unchanged, Outcome::Unchanged)
        );
        assert_eq!(fs::read(a.join("lessons.json")).unwrap(), first);
        assert!(host.last_sync().unwrap().is_some());
        assert!(client.last_sync().unwrap().is_some());
    }

    #[test]
    fn automatic_exchange_needs_consent_only_for_first_merge_conflicts_and_bulk_deletions() {
        let f = Fixture::new();
        let a = f.profile("desktop");
        let b = f.profile("phone");
        let (host, client) = paired(&a, &b);
        let phone = lesson(20, 9, "Телефон");
        let tasks: Vec<_> = (50..60)
            .map(|id| json!({"id":RecordId::from_u128(id), "title":"Химия", "body":"Повторить"}))
            .collect();
        save(&a, vec![lesson(10, 8, "ПК")], tasks.clone());
        save(&b, vec![phone.clone()], vec![]);
        let auto = || {
            let listener = TcpListener::bind(host.peer().unwrap().unwrap().endpoint()).unwrap();
            std::thread::scope(|scope| {
                let desktop = scope.spawn(|| {
                    host.serve_in(
                        listener,
                        Cancel::default(),
                        SessionMode::Automatic,
                        || true,
                        |_, _| panic!("Automatic mode cannot prompt"),
                    )
                });
                let phone = client
                    .join_in(Cancel::default(), SessionMode::Automatic, |_, _| {
                        panic!("Automatic mode cannot prompt")
                    })
                    .unwrap();
                (desktop.join().unwrap().unwrap(), phone)
            })
        };
        assert_eq!(auto(), (Outcome::ReviewRequired, Outcome::ReviewRequired));
        assert_eq!(host.last_sync().unwrap(), None);
        assert_eq!(
            exchange(&host, &client, |_| Some(vec![])).0,
            Outcome::Synchronized
        );
        let edited = lesson(10, 8, "Изменение ПК");
        save(&a, vec![edited.clone(), phone.clone()], tasks.clone());
        assert_eq!(auto(), (Outcome::Synchronized, Outcome::Synchronized));
        let checkpoint = host.state(&host.peer().unwrap().unwrap()).unwrap().receipt;
        assert!(matches!(
            client.change_address("192.0.2.1".parse().unwrap()),
            Err(Error::InvalidAddress)
        ));
        let new_address = "127.0.0.2".parse().unwrap();
        host.change_address(new_address).unwrap();
        client.change_address(new_address).unwrap();
        assert_eq!(auto(), (Outcome::Unchanged, Outcome::Unchanged));
        assert_eq!(
            host.state(&host.peer().unwrap().unwrap()).unwrap().receipt,
            checkpoint
        );
        assert!(host.read_schedule().unwrap() == client.read_schedule().unwrap());
        save(
            &a,
            vec![lesson(10, 8, "Изменение ПК 2"), phone.clone()],
            tasks.clone(),
        );
        save(
            &b,
            vec![lesson(10, 8, "Изменение телефона"), phone.clone()],
            tasks.clone(),
        );
        let retained_desktop = fs::read(a.join("lessons.json")).unwrap();
        let retained_phone = fs::read(b.join("lessons.json")).unwrap();
        assert_eq!(auto(), (Outcome::ReviewRequired, Outcome::ReviewRequired));
        assert_eq!(fs::read(a.join("lessons.json")).unwrap(), retained_desktop);
        assert_eq!(fs::read(b.join("lessons.json")).unwrap(), retained_phone);
        save(&b, vec![edited.clone(), phone.clone()], tasks.clone());
        assert_eq!(auto(), (Outcome::Synchronized, Outcome::Synchronized));
        assert_eq!(auto(), (Outcome::Unchanged, Outcome::Unchanged));
        save(&a, vec![edited.clone(), phone.clone()], vec![]);
        save(&b, vec![edited, phone], tasks);
        assert_eq!(auto(), (Outcome::ReviewRequired, Outcome::ReviewRequired));
        assert_eq!(client.read_schedule().unwrap().counts().1, 10);
    }

    #[test]
    fn commit_decision_replays_after_both_processes_restart() {
        let f = Fixture::new();
        let a = f.profile("desktop");
        let b = f.profile("phone");
        let (host, client) = paired(&a, &b);
        save(&a, vec![lesson(10, 8, "Первый")], vec![]);
        save(&b, vec![lesson(20, 9, "Второй")], vec![]);
        let old_host = host.read_schedule().unwrap();
        let old_client = client.read_schedule().unwrap();
        let left = Snapshot::new(host.identity.id(), None, &old_host).unwrap();
        let right = Snapshot::new(client.identity.id(), None, &old_client).unwrap();
        let id = RecordId::new_v4();
        let plan = MergePlan::compare(None, &left, &right).unwrap();
        let agreed = plan
            .resolve_proposal(&BTreeMap::new(), id)
            .unwrap()
            .checkpoint()
            .clone();
        let mut host_state = host.state(&host.peer().unwrap().unwrap()).unwrap();
        let mut client_state = client.state(&client.peer().unwrap().unwrap()).unwrap();
        host.stage(&mut host_state, id, agreed.clone(), &old_host)
            .unwrap();
        client
            .stage(&mut client_state, id, agreed, &old_client)
            .unwrap();
        // Force the participant's record of preparation to survive a disconnect.
        host.commit(&mut host_state, id).unwrap();
        assert!(client.read_schedule().unwrap() == old_client);
        assert!(matches!(ensure_writable(&b), Err(Error::ExchangePending)));
        assert!(ensure_writable(&a).is_ok());
        drop((host, client));
        let host = Profile::open(&a).unwrap();
        let client = Profile::open(&b).unwrap();
        assert_eq!(
            exchange(&host, &client, |_| panic!(
                "journal recovery needs no new approval"
            )),
            (Outcome::Resumed, Outcome::Resumed)
        );
        assert!(ensure_writable(&b).is_ok());
        assert!(host.read_schedule().unwrap() == client.read_schedule().unwrap());
        assert_eq!(client.read_schedule().unwrap().counts(), (2, 0, 0));
        assert!(fs::read_dir(b.join("backups")).unwrap().next().is_some());
    }

    #[test]
    fn interrupted_before_commit_discards_only_the_preparation() {
        let f = Fixture::new();
        let a = f.profile("desktop");
        let b = f.profile("phone");
        let (host, client) = paired(&a, &b);
        save(&a, vec![lesson(10, 8, "Первая")], vec![]);
        save(&b, vec![], vec![]);
        let before = fs::read(b.join("lessons.json")).unwrap();
        let left = Snapshot::new(host.identity.id(), None, &host.read_schedule().unwrap()).unwrap();
        let empty = SchoolData::default();
        let right = Snapshot::new(client.identity.id(), None, &empty).unwrap();
        let id = RecordId::new_v4();
        let plan = MergePlan::compare(None, &left, &right).unwrap();
        let prepared = plan.resolve_proposal(&BTreeMap::new(), id).unwrap();
        let mut state = client.state(&client.peer().unwrap().unwrap()).unwrap();
        client
            .stage(&mut state, id, prepared.checkpoint().clone(), &empty)
            .unwrap();
        assert_eq!(
            exchange(&host, &client, |_| panic!(
                "aborting an interrupted preparation"
            )),
            (Outcome::Cancelled, Outcome::Cancelled)
        );
        assert_eq!(fs::read(b.join("lessons.json")).unwrap(), before);
        assert!(ensure_writable(&b).is_ok());
    }

    #[test]
    fn missing_final_receipt_is_retried_without_rewriting_saved_data() {
        let f = Fixture::new();
        let a = f.profile("desktop");
        let b = f.profile("phone");
        let (host, client) = paired(&a, &b);
        save(&a, vec![lesson(10, 8, "Первый урок")], vec![]);
        save(&b, vec![], vec![]);
        let desktop = host.read_schedule().unwrap();
        let phone = client.read_schedule().unwrap();
        let left = Snapshot::new(host.identity.id(), None, &desktop).unwrap();
        let right = Snapshot::new(client.identity.id(), None, &phone).unwrap();
        let plan = MergePlan::compare(None, &left, &right).unwrap();
        let id = RecordId::new_v4();
        let prepared = plan.resolve_proposal(&BTreeMap::new(), id).unwrap();
        let peer = host.peer().unwrap().unwrap();
        let mut desktop_state = host.state(&peer).unwrap();
        let mut phone_state = client.state(&client.peer().unwrap().unwrap()).unwrap();
        host.stage(
            &mut desktop_state,
            id,
            prepared.checkpoint().clone(),
            &desktop,
        )
        .unwrap();
        client
            .stage(&mut phone_state, id, prepared.checkpoint().clone(), &phone)
            .unwrap();
        host.commit(&mut desktop_state, id).unwrap();

        let listener = TcpListener::bind(peer.endpoint()).unwrap();
        std::thread::scope(|scope| {
            scope.spawn(|| {
                let mut link = Channel::accept(
                    listener,
                    &host.identity,
                    &peer,
                    Cancel::default(),
                    std::time::Duration::from_millis(10),
                )
                .unwrap();
                link.send_control(
                    STATUS,
                    &Status::new(
                        host.identity.id(),
                        &desktop_state,
                        &desktop,
                        SessionMode::Interactive,
                    ),
                )
                .unwrap();
                let _: Status = link.receive_control(STATUS).unwrap();
                link.send_control(DECISION, &Action::Commit(id)).unwrap();
                exact_ack(&mut link, APPLIED, id).unwrap();
                host.finish(&mut desktop_state, id).unwrap();
                // Close before sending DONE, as if the network disappeared here.
            });
            assert_eq!(
                client
                    .join(Cancel::default(), |_, _| panic!("No new review"))
                    .unwrap(),
                Outcome::AwaitingReceipt
            );
        });
        let retained = fs::read(b.join("lessons.json")).unwrap();
        assert!(ensure_writable(&b).is_ok());
        assert_eq!(
            exchange(&host, &client, |_| panic!("Resume the old transaction")),
            (Outcome::Resumed, Outcome::Resumed)
        );
        assert_eq!(fs::read(b.join("lessons.json")).unwrap(), retained);
    }

    #[test]
    #[ignore = "measure encrypted round trips in release mode"]
    fn measure_local_wire() {
        for size in [12u128, 10_000] {
            let f = Fixture::new();
            let (a, b) = (f.profile("desktop"), f.profile("phone"));
            let (host, client) = paired(&a, &b);
            let tasks = (1..=size).map(|id| {
                json!({"id":RecordId::from_u128(id), "title":"Алгебра", "body":"Решить упражнения"})
            }).collect();
            save(&a, vec![], tasks);
            let start = std::time::Instant::now();
            assert_eq!(
                exchange(&host, &client, |_| Some(vec![])).0,
                Outcome::Synchronized
            );
            let first = start.elapsed();
            let start = std::time::Instant::now();
            assert_eq!(
                exchange(&host, &client, |_| Some(vec![])).0,
                Outcome::Unchanged
            );
            println!(
                "{size} records: first {:.1} ms, repeated {:.1} ms",
                first.as_secs_f64() * 1000.0,
                start.elapsed().as_secs_f64() * 1000.0
            );
        }
    }
}
