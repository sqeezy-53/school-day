use super::{Cancel, Error, Identity, Invitation, Pairing, Peer, Result, pair_client};
use crate::lessons::sync::Checkpoint;
use crate::lessons::{MAX_BYTES, RecordId, SchoolData};
use ring::digest::{SHA256, digest};
use rustls::pki_types::PrivatePkcs8KeyDer;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const IDENTITY: &str = "lan-identity.json";
const PEER: &str = "lan-peer.json";
const STATE: &str = "lan-state.json";
const HOLD: &str = "lan-write-hold";
const SCHEDULE: &str = "lessons.json";
const META_LIMIT: usize = 4096;
const STATE_LIMIT: usize = 2 * MAX_BYTES + META_LIMIT;
static WRITER: Mutex<()> = Mutex::new(());

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct IdentityFile {
    id: RecordId,
    pkcs8: Vec<u8>,
}

pub struct Profile {
    root: PathBuf,
    pub(crate) identity: Identity,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Pending {
    pub id: RecordId,
    pub commit: bool,
    pub applied: bool,
    candidate: Checkpoint,
    before: [u8; 32],
    after: [u8; 32],
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct State {
    peer: RecordId,
    pub checkpoint: Option<Checkpoint>,
    pub pending: Option<Pending>,
    pub receipt: Option<RecordId>,
    pub last_sync: Option<i64>,
}

impl Profile {
    pub fn open(dir: &Path) -> Result<Self> {
        let _lock = writer()?;
        let file = read(dir, IDENTITY, META_LIMIT)?
            .map(|bytes| decode::<IdentityFile>(&bytes))
            .transpose()?;
        let identity = match file {
            Some(file) => Identity::load(file.id, PrivatePkcs8KeyDer::from(file.pkcs8))?,
            None => {
                let (identity, key) = Identity::generate()?;
                store(
                    dir,
                    IDENTITY,
                    &IdentityFile {
                        id: identity.id(),
                        pkcs8: key.secret_pkcs8_der().to_vec(),
                    },
                    META_LIMIT,
                )?;
                identity
            }
        };
        Ok(Self {
            root: dir.to_owned(),
            identity,
        })
    }

    pub(crate) fn session(&self) -> Result<File> {
        fs::create_dir_all(&self.root)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.custom_flags(0x0020_0000);
        }
        let file = options.open(self.root.join("lan-session.lock"))?;
        if !file.metadata()?.is_file() {
            return Err(Error::InvalidMessage);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if file.metadata()?.file_attributes() & 0x400 != 0 {
                return Err(Error::InvalidMessage);
            }
        }
        file.try_lock().map_err(|_| Error::ExchangePending)?;
        Ok(file)
    }

    pub fn peer(&self) -> Result<Option<Peer>> {
        let value = read(&self.root, PEER, META_LIMIT)?
            .map(|bytes| decode::<Peer>(&bytes))
            .transpose()?;
        if let Some(peer) = &value {
            peer.validate(&self.identity)?;
        }
        Ok(value)
    }

    pub fn invite(&self, address: std::net::SocketAddrV4) -> Result<Pairing> {
        if self.peer()?.is_some() {
            return Err(Error::AlreadyPaired);
        }
        Pairing::open(&self.identity, address)
    }

    pub fn accept_pair(
        &self,
        offer: Pairing,
        cancel: Cancel,
        confirm: impl FnOnce(&str) -> bool,
    ) -> Result<()> {
        let _lease = self.session()?;
        if self.peer()?.is_some() {
            return Err(Error::AlreadyPaired);
        }
        offer.accept(&self.identity, cancel, confirm, |peer| self.trust(peer))
    }

    pub fn connect_pair(
        &self,
        offer: Invitation,
        cancel: Cancel,
        confirm: impl FnOnce(&str) -> bool,
    ) -> Result<()> {
        let _lease = self.session()?;
        if self.peer()?.is_some() {
            return Err(Error::AlreadyPaired);
        }
        pair_client(&self.identity, offer, cancel, confirm, |peer| {
            self.trust(peer)
        })
    }

    fn trust(&self, peer: &Peer) -> Result<()> {
        let _lock = writer()?;
        peer.validate(&self.identity)?;
        if self.peer()?.is_some() {
            return Err(Error::AlreadyPaired);
        }
        // A prior revocation may have removed trust just before deleting its receipt.
        remove(&self.root, STATE)?;
        remove(&self.root, HOLD)?;
        store(&self.root, PEER, peer, META_LIMIT)
    }

    pub fn revoke(&self) -> Result<()> {
        let _lease = self.session()?;
        let _lock = writer()?;
        if let Some(peer) = self.peer()? {
            let mut state = self.state(&peer)?;
            self.apply_decision(&mut state)?;
            remove(&self.root, PEER)?;
            remove(&self.root, STATE)?;
            remove(&self.root, HOLD)?;
        }
        Ok(())
    }

    pub fn change_address(&self, ip: std::net::Ipv4Addr) -> Result<()> {
        let _lease = self.session()?;
        let _lock = writer()?;
        let mut peer = self.peer()?.ok_or(Error::UntrustedDevice)?;
        peer.set_address(ip)?;
        // The address is a route to the existing peer, never a change to its key.
        store(&self.root, PEER, &peer, META_LIMIT)
    }

    pub fn last_sync(&self) -> Result<Option<i64>> {
        self.peer()?
            .as_ref()
            .map(|peer| self.state(peer).map(|state| state.last_sync))
            .transpose()
            .map(Option::flatten)
    }

    pub fn needs_sync(&self) -> Result<bool> {
        let Some(peer) = self.peer()? else {
            return Ok(false);
        };
        let state = self.state(&peer)?;
        if state.pending.is_some() {
            return Ok(true);
        }
        let Some(checkpoint) = state.checkpoint else {
            return Ok(true);
        };
        Ok(!checkpoint.matches_schedule(&self.read_schedule()?))
    }

    pub(crate) fn state(&self, peer: &Peer) -> Result<State> {
        peer.validate(&self.identity)?;
        let Some(bytes) = read(&self.root, STATE, STATE_LIMIT)? else {
            return Ok(State {
                peer: peer.id(),
                checkpoint: None,
                pending: None,
                receipt: None,
                last_sync: None,
            });
        };
        let state = decode::<State>(&bytes)?;
        if state.peer != peer.id() {
            return Err(Error::UntrustedDevice);
        }
        let mut devices = [self.identity.id(), peer.id()];
        devices.sort_unstable();
        if let Some(cp) = &state.checkpoint {
            validate_checkpoint(cp, devices)?;
        }
        if let Some(p) = &state.pending {
            validate_checkpoint(&p.candidate, devices)?;
            if p.id.is_nil()
                || (p.applied
                    && (!p.commit
                        || state.receipt != Some(p.id)
                        || state.checkpoint.as_ref().map(Checkpoint::id) != Some(p.candidate.id())))
            {
                return Err(Error::InvalidMessage);
            }
        }
        Ok(state)
    }

    pub(crate) fn read_schedule(&self) -> Result<SchoolData> {
        read_schedule(&self.root)
    }

    pub(crate) fn stage(
        &self,
        state: &mut State,
        id: RecordId,
        candidate: Checkpoint,
        expected: &SchoolData,
    ) -> Result<()> {
        let _lock = writer()?;
        if state.pending.is_some() {
            return Err(Error::ExchangePending);
        }
        let current = read_schedule(&self.root)?;
        if &current != expected {
            return Err(Error::ScheduleChanged);
        }
        let before = serialized(&current)?;
        let next = serialized(&candidate.schedule(&current))?;
        let path = self.root.join("backups");
        if state.checkpoint.is_none() || before != next {
            write(&path, &format!("before-sync-{id}.json"), &before, MAX_BYTES)?;
        }
        write(&self.root, HOLD, b"", 0)?;
        state.pending = Some(Pending {
            id,
            commit: false,
            applied: false,
            candidate,
            before: hash(&before),
            after: hash(&next),
        });
        self.save(state)
    }

    pub(crate) fn commit(&self, state: &mut State, id: RecordId) -> Result<()> {
        let _lock = writer()?;
        if state.pending.is_none() && state.receipt == Some(id) {
            return Ok(());
        }
        let pending = state
            .pending
            .as_mut()
            .filter(|pending| pending.id == id)
            .ok_or(Error::InvalidMessage)?;
        if !pending.commit {
            pending.commit = true;
            self.save(state)?;
        }
        self.apply_decision(state)
    }

    fn apply_decision(&self, state: &mut State) -> Result<()> {
        let Some(pending) = state
            .pending
            .as_mut()
            .filter(|pending| pending.commit && !pending.applied)
        else {
            return Ok(());
        };
        let current = read_schedule(&self.root)?;
        let current_hash = hash(&serialized(&current)?);
        if current_hash != pending.after {
            if current_hash != pending.before {
                return Err(Error::ScheduleChanged);
            }
            let next = serialized(&pending.candidate.schedule(&current))?;
            if hash(&next) != pending.after {
                return Err(Error::InvalidMessage);
            }
            write(&self.root, SCHEDULE, &next, MAX_BYTES)?;
        }
        state.checkpoint = Some(pending.candidate.clone());
        state.receipt = Some(pending.id);
        pending.applied = true;
        self.save(state)?;
        remove(&self.root, HOLD)
    }

    pub(crate) fn abort(&self, state: &mut State, id: RecordId) -> Result<()> {
        let _lock = writer()?;
        if state
            .pending
            .as_ref()
            .is_some_and(|p| p.id != id || p.commit)
            || state.receipt == Some(id)
        {
            return Err(Error::InvalidMessage);
        }
        state.pending = None;
        self.save(state)?;
        remove(&self.root, HOLD)
    }

    pub(crate) fn finish(&self, state: &mut State, id: RecordId) -> Result<()> {
        let _lock = writer()?;
        if state.receipt != Some(id)
            || state
                .pending
                .as_ref()
                .is_some_and(|p| p.id != id || !p.applied)
        {
            return Err(Error::InvalidMessage);
        }
        state.pending = None;
        state.last_sync = Some(chrono::Utc::now().timestamp());
        self.save(state)
    }

    fn save(&self, state: &State) -> Result<()> {
        store(&self.root, STATE, state, STATE_LIMIT)
    }
}

/// Replay a locally committed journal before loading the normal schedule.
/// An undecided journal remains intact until the authenticated coordinator reconnects.
pub fn recover(dir: &Path) -> Result<()> {
    if !dir.join(HOLD).try_exists()? {
        return Ok(());
    }
    if !dir.join(STATE).try_exists()? {
        let _lock = writer()?;
        return remove(dir, HOLD);
    }
    let profile = Profile::open(dir)?;
    let _lease = profile.session()?;
    let _lock = writer()?;
    let peer = profile.peer()?.ok_or(Error::UntrustedDevice)?;
    let mut state = profile.state(&peer)?;
    profile.apply_decision(&mut state)?;
    if state.pending.as_ref().is_none_or(|pending| pending.applied) {
        remove(dir, HOLD)?;
    }
    Ok(())
}

pub fn ensure_writable(dir: &Path) -> Result<()> {
    match fs::symlink_metadata(dir.join(HOLD)) {
        Ok(_) => Err(Error::ExchangePending),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

pub fn local_write<T>(
    dir: &Path,
    save: impl FnOnce() -> std::result::Result<T, String>,
) -> std::result::Result<T, String> {
    let _lock = writer().map_err(|error| error.to_string())?;
    ensure_writable(dir).map_err(|e| e.to_string())?;
    save()
}

fn writer() -> Result<std::sync::MutexGuard<'static, ()>> {
    WRITER.lock().map_err(|_| Error::InvalidMessage)
}

fn validate_checkpoint(cp: &Checkpoint, devices: [RecordId; 2]) -> Result<()> {
    if cp.devices() != devices {
        return Err(Error::UntrustedDevice);
    }
    Checkpoint::decode(&cp.encode()?)?;
    Ok(())
}

fn read_schedule(dir: &Path) -> Result<SchoolData> {
    let Some(bytes) = read(dir, SCHEDULE, MAX_BYTES)? else {
        return Ok(SchoolData::default());
    };
    let (schedule, legacy) = SchoolData::decode(&bytes).map_err(|_| Error::InvalidMessage)?;
    if legacy {
        return Err(Error::InvalidMessage);
    }
    Ok(schedule)
}

fn serialized(schedule: &SchoolData) -> Result<Vec<u8>> {
    let bytes = serde_json::to_vec(schedule).map_err(|_| Error::InvalidMessage)?;
    if bytes.len() > MAX_BYTES {
        return Err(Error::InvalidMessage);
    }
    Ok(bytes)
}

fn hash(bytes: &[u8]) -> [u8; 32] {
    let mut output = [0; 32];
    output.copy_from_slice(digest(&SHA256, bytes).as_ref());
    output
}

fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    serde_json::from_slice(bytes).map_err(|_| Error::InvalidMessage)
}

fn store(dir: &Path, name: &str, value: &impl Serialize, limit: usize) -> Result<()> {
    write(
        dir,
        name,
        &serde_json::to_vec(value).map_err(|_| Error::InvalidMessage)?,
        limit,
    )
}

fn read(dir: &Path, name: &str, limit: usize) -> Result<Option<Vec<u8>>> {
    let path = dir.join(name);
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000); // FILE_FLAG_OPEN_REPARSE_POINT
    }
    let file = match options.open(&path) {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    let meta = file.metadata()?;
    if !meta.is_file() || meta.len() > limit as u64 {
        return Err(Error::InvalidMessage);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return Err(Error::InvalidMessage);
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if name == IDENTITY && (meta.mode() & 0o077 != 0 || meta.nlink() != 1) {
            return Err(Error::UntrustedDevice);
        }
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(Error::InvalidMessage);
    }
    Ok(Some(bytes))
}

fn write(dir: &Path, name: &str, bytes: &[u8], limit: usize) -> Result<()> {
    if bytes.len() > limit {
        return Err(Error::InvalidMessage);
    }
    fs::create_dir_all(dir)?;
    let temporary = dir.join(format!(".lan-{}.tmp", RecordId::new_v4()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, dir.join(name))?;
        sync_dir(dir)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(Into::into)
}

fn remove(dir: &Path, name: &str) -> Result<()> {
    match fs::remove_file(dir.join(name)) {
        Ok(()) => Ok(sync_dir(dir)?),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn sync_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        File::open(dir)?.sync_all()?;
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
    }
    Ok(())
}
