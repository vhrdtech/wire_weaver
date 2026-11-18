//! Captures (SRV-64, `ww_hub_api::Captures`): the hub records a device's link while a capture of it exists, through
//! the [CaptureHost] its host gave it ([crate::HubConfig::captures]). The hub keeps who asked for which capture, taps
//! the device, checks `save` paths and serves watches; the host (eventfull) keeps the records.
//!
//! Trust: every connection is the hub's own OS user (the socket lets no one else in), so a capture, a watch or a
//! saved file gives a client nothing it couldn't do itself. `save` still writes a file at a path another process
//! chose: [SavePath] refuses anything but a new file in an existing directory, so nothing is overwritten, and the
//! file is the user's alone. What each kind of caller may do is one allow-list, [allowed]: a relaying hub
//! (federation, SRV-33, has no trust model yet) may only start and stop. `discard` and `set_ram_max` need a part in
//! the capture. Every list and string crossing the API is bounded by a constant of `ww_hub_api`.

use std::collections::BTreeMap;
use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use wire_weaver::shrink_wrap::MaxLen;
use wire_weaver_client::tap::TapSource;
use ww_hub_api::{
    CaptureEntryOwned, CaptureErrorOwned, CaptureId, CaptureOptions, CapturePageOwned,
    CaptureStats, MAX_CAPTURE_CLIENTS, MAX_CAPTURE_PAGE_BYTES, MAX_CAPTURES, MAX_RELAY_CAPTURES,
    Pressure, RAM_MAX_DEFAULT, RamMax, RecordBatchOwned, SaveInfoOwned,
};
use ww_tap::ClientMap;

use crate::claims::Owner;

/// Records devices' links for the hub: one [HostCapture] per capture. Given by the program that hosts the hub
/// (`ww`, the Python wheel: eventfull's), in [crate::HubConfig::captures]; a hub without one records nothing.
pub trait CaptureHost: Send + Sync {
    /// A new capture of the device `key`, empty until the hub sets its [HostCapture::source]'s taps on the device's
    /// links. `options.ram_max` is set.
    fn start(&self, key: &str, options: &CaptureOptions) -> Result<Arc<dyn HostCapture>, String>;
}

impl std::fmt::Debug for dyn CaptureHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CaptureHost")
    }
}

/// One capture of a [CaptureHost]. Every call but [HostCapture::save] returns at once: the taps and
/// [HostCapture::client_map] are called on the device's event loop, which must never wait for the capture.
pub trait HostCapture: Send + Sync {
    /// Makes the tap of each of the device's links; set on the device's event loop while the capture exists.
    fn source(&self) -> TapSource;

    /// What the capture records changed: another client started it, or `set_ram_max`. `links` stays as started.
    fn set_options(&self, options: &CaptureOptions);

    /// The hub forwarded client `client`'s request `client_seq` to the device as `device_seq`, as it went out
    /// (`ww_tap::Link::client_map`).
    fn client_map(&self, map: ClientMap);

    /// Keep what is in RAM now, then everything after, in a project file with one run at `to`, created with
    /// [SavePath::create]. May take a while: called from a blocking thread.
    fn save(&self, to: &SavePath) -> Result<Saved, String>;

    /// A cursor over the capture's records from now on, read as `pressure` says. It never makes the device wait:
    /// a cursor behind the newest records skips (`RealTime`) or reads them later from RAM (`Everything`).
    fn watch(&self, pressure: Pressure) -> Result<Box<dyn HostWatch>, String>;

    fn stats(&self) -> CaptureStats;

    /// A saved capture ends: its taps are taken off the device's links, its file is closed as written, its watches
    /// end. Called once, only for a saved capture; nothing else is called after.
    fn end(&self);

    /// [HostCapture::end], and what is in RAM goes. The hub calls it, never [HostCapture::end], for a capture that
    /// ends unsaved (its last client stopped it or left): nothing could read those records any more.
    fn discard(&self);
}

/// A watch's cursor, see [HostCapture::watch].
pub trait HostWatch: Send {
    /// The next records, encoded at most `max_bytes` long as a `RecordBatch` (`ww_hub_api::MAX_BATCH_ENTRIES`
    /// entries at most): waits until there are some. `None` once the capture ended.
    fn next(
        &mut self,
        max_bytes: usize,
    ) -> Pin<Box<dyn Future<Output = Option<RecordBatchOwned>> + Send + '_>>;
}

/// What [HostCapture::save] wrote when it answered.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Saved {
    /// Records written: what was in RAM.
    pub records: u64,
    /// Records dropped unsaved at the RAM limit before the save: the file lacks them at its start.
    pub dropped: u64,
}

/// What the program hosting the hub gives it: `ww_hub::main_if_hub_with`, `ww hub serve`, `wire_weaver_py`.
#[derive(Copy, Clone, Debug, Default)]
pub struct HubHost {
    /// Makes the hub's [CaptureHost], `None`: the hub records nothing (test hosts, examples).
    pub captures: Option<fn() -> Arc<dyn CaptureHost>>,
}

/// A path [Captures::save](ww_hub_api::Captures::save) may write: absolute, without `.` or `..`, naming no existing
/// file, in an existing directory. Only the hub makes one.
#[derive(Clone, Debug)]
pub struct SavePath {
    path: PathBuf,
    /// The file [SavePath::create] made: removed again if the save fails
    created: Arc<OnceLock<FileId>>,
}

impl PartialEq for SavePath {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
    }
}

impl Eq for SavePath {}

/// Which file a path named when it was created: a file put there since is not it.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct FileId {
    #[cfg(unix)]
    dev: u64,
    #[cfg(unix)]
    ino: u64,
}

impl FileId {
    fn of(meta: &std::fs::Metadata) -> FileId {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            FileId {
                dev: meta.dev(),
                ino: meta.ino(),
            }
        }
        #[cfg(not(unix))]
        {
            let _ = meta;
            FileId {}
        }
    }
}

impl SavePath {
    /// Refuses what a client could not mean or what would overwrite something: the hub writes as its own user at a
    /// path another process chose. A relative path would land in the hub's working directory, not the client's.
    /// Reads the file system: called on a blocking thread.
    pub(crate) fn check(path: &str) -> Result<SavePath, Box<CaptureErrorOwned>> {
        let bad = |why: &str| Box::new(CaptureErrorOwned::BadPath { why: why.into() });
        if path.contains('\0') {
            return Err(bad("a NUL in it"));
        }
        let p = Path::new(path);
        if !p.is_absolute() {
            return Err(bad(
                "not absolute: the hub's working directory isn't the client's",
            ));
        }
        // on the string: `Path::components` drops a `.` inside; an odd name refused so is harmless
        if path.split(['/', '\\']).any(|seg| seg == "." || seg == "..") {
            return Err(bad("`.` or `..` in it"));
        }
        if p.file_name().is_none()
            || path.ends_with(std::path::MAIN_SEPARATOR)
            || path.ends_with('/')
        {
            return Err(bad("names no file"));
        }
        let dir = p.parent().ok_or_else(|| bad("no directory"))?;
        match std::fs::metadata(dir) {
            Ok(m) if m.is_dir() => {}
            Ok(_) => return Err(bad("its directory isn't one")),
            Err(_) => return Err(bad("no such directory")),
        }
        // the file itself is never followed: a symlink there is a file that exists
        if std::fs::symlink_metadata(p).is_ok() {
            return Err(CaptureErrorOwned::Exists.into());
        }
        Ok(SavePath {
            path: p.to_path_buf(),
            created: Default::default(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Creates the file, readable and writable only by the hub's user (a capture can hold secrets), refusing one
    /// that exists by now (`O_EXCL`, and `O_NOFOLLOW`: a symlink put there since the check isn't followed).
    pub fn create(&self) -> io::Result<std::fs::File> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        }
        let file = options.open(&self.path)?;
        let _ = self.created.set(FileId::of(&file.metadata()?));
        Ok(file)
    }

    /// The save failed: the file [SavePath::create] made goes, so that a retry at the same path can create it.
    /// A file another process put there since is left alone.
    fn remove_created(&self) {
        if let Some(created) = self.created.get()
            && std::fs::symlink_metadata(&self.path).is_ok_and(|m| FileId::of(&m) == *created)
        {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// A lock that survives a thread that panicked while holding it: the registry's state is changed in steps that
/// each leave it whole, and the host is never called under it.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What a client may do with captures: every method of `Captures`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Act {
    Start,
    Stop,
    Save,
    Discard,
    SetRamMax,
    List,
    Watch,
}

/// Who asks, as far as captures go.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Caller {
    /// A program of the hub's own OS user, on this PC
    Local,
    /// A relaying hub (`ClientMeta` kind `Hub`, the federation path, SRV-33): untrusted until federation has a trust
    /// model. The kind is the client's own word, so it only ever takes rights away.
    Relay,
}

/// The one allow-list of captures: every pair of method and caller is named, so a method or caller added later is
/// refused until it is decided here. A relaying hub may start and stop (within [MAX_RELAY_CAPTURES]), nothing that
/// writes files, reads traffic or touches captures of others.
pub(crate) fn allowed(act: Act, caller: Caller) -> Result<(), Box<CaptureErrorOwned>> {
    use Act::*;
    let ok = match (caller, act) {
        (Caller::Local, Start | Stop | Save | Discard | SetRamMax | List | Watch) => true,
        (Caller::Relay, Start | Stop) => true,
        (Caller::Relay, Save | Discard | SetRamMax | List | Watch) => false,
    };
    match ok {
        true => Ok(()),
        false => Err(CaptureErrorOwned::NotAllowed {
            why: format!("a relaying hub may only start and stop captures, not {act:?}"),
        }
        .into()),
    }
}

/// The hub's captures: who asked for which, and the host's object of each. The host is never called with
/// [Registry::state] locked: a slow or panicking host doesn't hold up or poison the registry.
pub(crate) struct Registry {
    pub(crate) host: Arc<dyn CaptureHost>,
    state: Mutex<State>,
    /// Held across a new capture's [CaptureHost::start], so that two clients starting the same device get one
    starting: Mutex<()>,
    /// Saves running on blocking threads: the hub waits for them before it exits
    saves: tokio::sync::watch::Sender<usize>,
}

#[derive(Default)]
struct State {
    next_id: u32,
    by_id: BTreeMap<u32, Entry>,
}

struct Entry {
    key: String,
    /// What the capture records for its clients together
    options: CaptureOptions,
    /// Client instances that started it ([MAX_CAPTURE_CLIENTS] at most): the connection that asked (for
    /// `captures()`) and what it asked for
    asks: Vec<Ask>,
    cap: Arc<dyn HostCapture>,
    saved_to: Option<String>,
    /// Open watches, over every connection
    watches: Arc<AtomicU32>,
}

impl Entry {
    fn has_part(&self, owner: &Owner) -> bool {
        self.asks.iter().any(|a| a.owner == *owner)
    }
}

struct Ask {
    owner: Owner,
    connection: u32,
    options: CaptureOptions,
}

/// What [Registry::start] did.
pub(crate) enum Started {
    /// A capture of the device runs already: this client asked too
    Joined(CaptureId),
    /// A new capture: the device is to be tapped
    New(CaptureId, Arc<dyn HostCapture>),
}

/// A capture taken out of the registry: [crate::Hub::capture_ended] ends it in the host and untaps its device.
pub(crate) struct Ended {
    pub(crate) key: String,
    pub(crate) cap: Arc<dyn HostCapture>,
    /// What it had in RAM goes: discarded, or ended unsaved
    discard: bool,
}

/// A host call decided with the registry locked, made after it is let go.
#[must_use]
struct SetOptions(Option<(Arc<dyn HostCapture>, CaptureOptions)>);

impl SetOptions {
    fn call(self) {
        if let Some((cap, options)) = self.0 {
            cap.set_options(&options);
        }
    }
}

/// `ram_max` filled in.
fn with_ram(mut options: CaptureOptions) -> CaptureOptions {
    options.ram_max = Some(options.ram_max.unwrap_or(ram(RAM_MAX_DEFAULT)));
    options
}

fn ram(bytes: u64) -> RamMax {
    RamMax::try_new(bytes)
        .expect("RAM limits come from RamMax values or the default, within its range")
}

/// What clients asked for together: the union of what to record, the largest RAM limit, the first one's links.
fn merged(asks: &[Ask], links: ww_hub_api::LinkIds) -> CaptureOptions {
    let mut out = CaptureOptions {
        record: Default::default(),
        links,
        ram_max: None,
    };
    let mut ram_max = 0;
    for a in asks {
        out.record.frames |= a.options.record.frames;
        out.record.messages |= a.options.record.messages;
        ram_max = ram_max.max(a.options.ram_max.map_or(RAM_MAX_DEFAULT, |r| r.get()));
    }
    out.ram_max = Some(ram(ram_max.max(ww_hub_api::RAM_MAX_MIN)));
    out
}

/// The entry's options from its asks; the host is told if they changed.
fn remerge(e: &mut Entry) -> SetOptions {
    let options = merged(&e.asks, e.options.links);
    if options == e.options {
        return SetOptions(None);
    }
    e.options = options;
    SetOptions(Some((e.cap.clone(), options)))
}

/// Takes capture `id` out of the registry. An unsaved capture's records go with it: nothing would ever read them.
fn take(st: &mut State, id: u32, discard: bool) -> Option<Ended> {
    let e = st.by_id.remove(&id)?;
    Some(Ended {
        discard: discard || e.saved_to.is_none(),
        key: e.key,
        cap: e.cap,
    })
}

impl Registry {
    pub(crate) fn new(host: Arc<dyn CaptureHost>) -> Self {
        Registry {
            host,
            state: Mutex::new(State::default()),
            starting: Mutex::new(()),
            saves: tokio::sync::watch::Sender::new(0),
        }
    }

    /// `owner` (on `connection`, as `caller`) records `key`.
    pub(crate) fn start(
        &self,
        owner: Owner,
        caller: Caller,
        connection: u32,
        key: &str,
        options: CaptureOptions,
    ) -> Result<Started, Box<CaptureErrorOwned>> {
        let options = with_ram(options);
        let _starting = lock(&self.starting);
        let mut st = lock(&self.state);
        if caller == Caller::Relay {
            let held = st
                .by_id
                .values()
                .filter(|e| e.key != key && e.has_part(&owner))
                .count();
            if held >= MAX_RELAY_CAPTURES {
                return Err(CaptureErrorOwned::TooMany.into());
            }
        }
        if let Some((id, e)) = st.by_id.iter_mut().find(|(_, e)| e.key == key) {
            let id = CaptureId(*id);
            let full = e.asks.len() >= MAX_CAPTURE_CLIENTS;
            match e.asks.iter_mut().find(|a| a.owner == owner) {
                Some(a) => a.options = options,
                None if full => return Err(CaptureErrorOwned::TooMany.into()),
                None => e.asks.push(Ask {
                    owner,
                    connection,
                    options,
                }),
            }
            let set = remerge(e);
            drop(st);
            set.call();
            return Ok(Started::Joined(id));
        }
        if st.by_id.len() >= MAX_CAPTURES {
            return Err(CaptureErrorOwned::TooMany.into());
        }
        drop(st);
        let cap = self
            .host
            .start(key, &options)
            .map_err(|why| CaptureErrorOwned::Host { why })?;
        let mut st = lock(&self.state);
        st.next_id = st.next_id.wrapping_add(1).max(1);
        while st.by_id.contains_key(&st.next_id) {
            st.next_id = st.next_id.wrapping_add(1).max(1);
        }
        let id = st.next_id;
        st.by_id.insert(
            id,
            Entry {
                key: key.to_string(),
                options,
                asks: vec![Ask {
                    owner,
                    connection,
                    options,
                }],
                cap: cap.clone(),
                saved_to: None,
                watches: Arc::new(AtomicU32::new(0)),
            },
        );
        Ok(Started::New(CaptureId(id), cap))
    }

    /// The capture of the device `key`, if one runs: its taps go on the device's links.
    pub(crate) fn of_device(&self, key: &str) -> Option<Arc<dyn HostCapture>> {
        lock(&self.state)
            .by_id
            .values()
            .find(|e| e.key == key)
            .map(|e| e.cap.clone())
    }

    /// `owner` stops its part in capture `id`; the capture ends with the last one.
    pub(crate) fn stop(
        &self,
        owner: &Owner,
        id: CaptureId,
    ) -> Result<Option<Ended>, Box<CaptureErrorOwned>> {
        let mut st = lock(&self.state);
        let e = st
            .by_id
            .get_mut(&id.0)
            .filter(|e| e.has_part(owner))
            .ok_or(CaptureErrorOwned::UnknownCapture)?;
        e.asks.retain(|a| a.owner != *owner);
        if e.asks.is_empty() {
            return Ok(take(&mut st, id.0, false));
        }
        let set = remerge(e);
        drop(st);
        set.call();
        Ok(None)
    }

    /// The capture `id`, refused to a client instance with no part in it.
    fn part_of<'s>(
        st: &'s mut State,
        owner: &Owner,
        id: CaptureId,
    ) -> Result<&'s mut Entry, Box<CaptureErrorOwned>> {
        let e = st
            .by_id
            .get_mut(&id.0)
            .ok_or(CaptureErrorOwned::UnknownCapture)?;
        match e.has_part(owner) {
            true => Ok(e),
            false => Err(CaptureErrorOwned::NotAllowed {
                why: "no part in this capture: only a client that started or joined it may".into(),
            }
            .into()),
        }
    }

    /// Capture `id` ends for everyone, what is in RAM goes; `owner` must have a part in it.
    pub(crate) fn discard(
        &self,
        owner: &Owner,
        id: CaptureId,
    ) -> Result<Ended, Box<CaptureErrorOwned>> {
        let mut st = lock(&self.state);
        Self::part_of(&mut st, owner, id)?;
        take(&mut st, id.0, true).ok_or_else(|| CaptureErrorOwned::UnknownCapture.into())
    }

    /// A capture that never got going (its device couldn't be opened) goes again.
    pub(crate) fn abandon(&self, id: CaptureId) -> Option<Ended> {
        take(&mut lock(&self.state), id.0, true)
    }

    /// Every capture whose last client instance was `owner`, which left: they end.
    pub(crate) fn owner_left(&self, owner: &Owner) -> Vec<Ended> {
        let mut st = lock(&self.state);
        let (mut ending, mut sets) = (vec![], vec![]);
        for (id, e) in st.by_id.iter_mut() {
            if !e.has_part(owner) {
                continue;
            }
            e.asks.retain(|a| a.owner != *owner);
            match e.asks.is_empty() {
                true => ending.push(*id),
                false => sets.push(remerge(e)),
            }
        }
        let ended = ending
            .into_iter()
            .filter_map(|id| take(&mut st, id, false))
            .collect();
        drop(st);
        sets.into_iter().for_each(SetOptions::call);
        ended
    }

    /// Every capture: the hub exits.
    pub(crate) fn end_all(&self) -> Vec<Ended> {
        let mut st = lock(&self.state);
        let ids: Vec<u32> = st.by_id.keys().copied().collect();
        ids.into_iter()
            .filter_map(|id| take(&mut st, id, false))
            .collect()
    }

    /// `owner`, with a part in capture `id`, sets its RAM limit.
    pub(crate) fn set_ram_max(
        &self,
        owner: &Owner,
        id: CaptureId,
        ram_max: RamMax,
    ) -> Result<(), Box<CaptureErrorOwned>> {
        let mut st = lock(&self.state);
        let e = Self::part_of(&mut st, owner, id)?;
        for a in &mut e.asks {
            a.options.ram_max = Some(ram_max);
        }
        e.options.ram_max = Some(ram_max);
        let set = SetOptions(Some((e.cap.clone(), e.options)));
        drop(st);
        set.call();
        Ok(())
    }

    /// Counts a save until the guard goes: the hub waits for it before it exits.
    pub(crate) fn save_running(&self) -> SaveRunning {
        self.saves.send_modify(|n| *n += 1);
        SaveRunning(self.saves.clone())
    }

    /// Waits until no save runs, `wait` at most.
    pub(crate) async fn saves_done(&self, wait: std::time::Duration) -> bool {
        let mut rx = self.saves.subscribe();
        tokio::time::timeout(wait, rx.wait_for(|n| *n == 0))
            .await
            .is_ok()
    }

    /// `Captures::save` of capture `id` to `path`, on a blocking thread (the path's checks and the host's save read
    /// and write the file system, which may hang). A failed save removes the file it created and leaves the capture
    /// unsaved.
    pub(crate) fn save(
        &self,
        id: CaptureId,
        path: &str,
    ) -> Result<SaveInfoOwned, Box<CaptureErrorOwned>> {
        let path = SavePath::check(path)?;
        let cap = {
            let mut st = lock(&self.state);
            let e = st
                .by_id
                .get_mut(&id.0)
                .ok_or(CaptureErrorOwned::UnknownCapture)?;
            if let Some(saved) = &e.saved_to {
                return Err(CaptureErrorOwned::Saved {
                    path: saved.clone(),
                }
                .into());
            }
            e.saved_to = Some(path.path().display().to_string());
            e.cap.clone()
        };
        let saved = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| cap.save(&path)))
            .unwrap_or_else(|_| Err("the capture host panicked".into()));
        match saved {
            Ok(saved) => Ok(SaveInfoOwned {
                path: path.path().display().to_string(),
                records: ww_si::Fixed::new(saved.records),
                dropped: ww_si::Fixed::new(saved.dropped),
            }),
            Err(why) => {
                path.remove_created();
                if let Some(e) = lock(&self.state).by_id.get_mut(&id.0) {
                    e.saved_to = None;
                }
                Err(CaptureErrorOwned::Host { why }.into())
            }
        }
    }

    /// A watch of capture `id`, counted until the guard goes.
    pub(crate) fn watch(
        &self,
        id: CaptureId,
        pressure: Pressure,
    ) -> Result<(Box<dyn HostWatch>, WatchCount), Box<CaptureErrorOwned>> {
        let (cap, count) = {
            let st = lock(&self.state);
            let e = st
                .by_id
                .get(&id.0)
                .ok_or(CaptureErrorOwned::UnknownCapture)?;
            (e.cap.clone(), e.watches.clone())
        };
        let watch = cap
            .watch(pressure)
            .map_err(|why| CaptureErrorOwned::Host { why })?;
        count.fetch_add(1, Ordering::Relaxed);
        Ok((watch, WatchCount(count)))
    }

    /// The captures after `after`, as many as fit [MAX_CAPTURE_PAGE_BYTES], for `Captures::captures`.
    pub(crate) fn page(&self, after: Option<CaptureId>) -> CapturePageOwned {
        let from = after.map_or(0, |a| a.0.saturating_add(1));
        let listed: Vec<_> = lock(&self.state)
            .by_id
            .range(from..)
            .map(|(id, e)| {
                let clients = e.asks.iter().map(|a| a.connection).collect();
                let entry = CaptureEntryOwned {
                    id: CaptureId(*id),
                    key: e.key.clone(),
                    options: e.options,
                    saved_to: e.saved_to.clone(),
                    clients: MaxLen::new_unchecked(clients),
                    watches: e.watches.load(Ordering::Relaxed),
                    stats: Default::default(),
                };
                (entry, e.cap.clone())
            })
            .collect();
        let (mut entries, mut more, mut bytes) = (vec![], false, 0);
        for (mut entry, cap) in listed {
            entry.stats = cap.stats();
            let len = entry_len(&entry);
            if bytes + len > PAGE_ENTRIES_BYTES {
                more = true;
                break;
            }
            bytes += len;
            entries.push(entry);
        }
        CapturePageOwned {
            // at most MAX_CAPTURES: listed from the registry, which holds no more
            entries: MaxLen::new_unchecked(entries),
            more,
        }
    }
}

/// What a page's entries may take of [MAX_CAPTURE_PAGE_BYTES]: the rest is the page's own fields.
const PAGE_ENTRIES_BYTES: usize = MAX_CAPTURE_PAGE_BYTES - 64;

/// The longest entry: its strings at their limits, every client, every number at its widest. A page always holds one.
const MAX_ENTRY_BYTES: usize =
    ww_hub_api::MAX_KEY_LEN + ww_hub_api::MAX_PATH_LEN + MAX_CAPTURE_CLIENTS * 5 + 256;
const _: () = assert!(MAX_ENTRY_BYTES <= PAGE_ENTRIES_BYTES);

/// An entry's length on the wire.
fn entry_len(entry: &CaptureEntryOwned) -> usize {
    use wire_weaver::shrink_wrap::SerializeShrinkWrapOwned;
    entry
        .to_ww_bytes_owned()
        .map_or(MAX_ENTRY_BYTES, |b| b.len())
}

/// Counts one open watch of a capture while it lives.
pub(crate) struct WatchCount(Arc<AtomicU32>);

impl Drop for WatchCount {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Counts one running save while it lives, see [Registry::save_running].
pub(crate) struct SaveRunning(tokio::sync::watch::Sender<usize>);

impl Drop for SaveRunning {
    fn drop(&mut self) {
        self.0.send_modify(|n| *n -= 1);
    }
}

impl crate::Hub {
    /// Who connection `connection` is for captures, and its client instance.
    pub(crate) fn caller(&self, connection: u32) -> (Owner, Caller) {
        let state = self.state.lock().unwrap();
        let owner = crate::owner_of(&state, connection);
        let relayed = state
            .clients
            .get(&connection)
            .and_then(|c| c.meta.as_ref())
            .is_some_and(|m| m.kind == Some(ww_device::ww_link::ClientKind::Hub));
        // every principal is named: one added later (SRV-33, SRV-37) has no rights until it is decided here
        let caller = match owner.principal {
            crate::claims::Principal::LocalUser => match relayed {
                true => Caller::Relay,
                false => Caller::Local,
            },
        };
        (owner, caller)
    }

    /// The captures and connection `connection`'s client instance, if it may `act`.
    pub(crate) fn captures_for(
        &self,
        connection: u32,
        act: Act,
    ) -> Result<(&Registry, Owner, Caller), Box<CaptureErrorOwned>> {
        let registry = self.registry()?;
        let (owner, caller) = self.caller(connection);
        allowed(act, caller)?;
        Ok((registry, owner, caller))
    }

    /// `Captures::start` of client `connection`.
    pub(crate) async fn capture_start(
        self: &Arc<Self>,
        connection: u32,
        key: &str,
        options: CaptureOptions,
    ) -> Result<CaptureId, Box<CaptureErrorOwned>> {
        let (registry, owner, caller) = self.captures_for(connection, Act::Start)?;
        if !crate::device::has_transport(key) || self.lent_to(key).is_some() {
            return Err(CaptureErrorOwned::UnknownDevice.into());
        }
        match registry.start(owner, caller, connection, key, options)? {
            Started::Joined(id) => Ok(id),
            // opened with the tap if the hub doesn't hold it yet, else tapped now; kept open while recorded
            Started::New(id, cap) => {
                match self
                    .send_to_device(key, crate::device::ToDevice::Capture(cap))
                    .await
                {
                    Ok(()) => Ok(id),
                    Err(status) => {
                        tracing::debug!("ww hub: capture of {key} can't open it: {status}");
                        if let Some(ended) = registry.abandon(id) {
                            self.capture_ended(ended);
                        }
                        Err(CaptureErrorOwned::UnknownDevice.into())
                    }
                }
            }
        }
    }

    /// A capture left the registry: it ends in the host (here, not under the registry's lock), and its device, if
    /// open, takes its taps off and lingers as usual. The device is told which capture ended, so that a late word
    /// never untaps a capture of the same device started since.
    pub(crate) fn capture_ended(self: &Arc<Self>, ended: Ended) {
        let Ended { key, cap, discard } = ended;
        let host = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match discard {
            true => cap.discard(),
            false => cap.end(),
        }));
        if host.is_err() {
            tracing::warn!("ww hub: the capture host panicked ending the capture of {key}");
        }
        let hub = self.clone();
        tokio::spawn(async move {
            let slot = hub.device_slot(&key);
            if let Some(handle) = slot.lock().await.as_ref() {
                let _ = handle.tx.send(crate::device::ToDevice::Untap(cap));
            }
        });
    }

    /// The captures, or why this hub has none.
    pub(crate) fn registry(&self) -> Result<&Registry, Box<CaptureErrorOwned>> {
        self.captures.as_ref().ok_or_else(|| {
            Box::new(CaptureErrorOwned::NotAvailable {
                hub: self.info(),
                started_by: self.started_by.clone(),
            })
        })
    }

    /// The hub exits: running saves finish (`wait` at most), then every capture ends, closing its file.
    pub(crate) async fn end_captures(self: &Arc<Self>, wait: std::time::Duration) {
        let Some(registry) = &self.captures else {
            return;
        };
        if !registry.saves_done(wait).await {
            tracing::warn!("ww hub: saves still running after {wait:?}, exiting anyway");
        }
        for ended in registry.end_all() {
            self.capture_ended(ended);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ww_hub_api::{LinkIds, RecordWhat};

    /// A host that keeps nothing: counts calls, panics in `set_options` when told to.
    #[derive(Default)]
    struct NullHost {
        panic_set_options: std::sync::atomic::AtomicBool,
        ends: AtomicU32,
        discards: AtomicU32,
    }

    struct NullCapture(Arc<NullHost>);

    impl CaptureHost for Arc<NullHost> {
        fn start(&self, _: &str, _: &CaptureOptions) -> Result<Arc<dyn HostCapture>, String> {
            Ok(Arc::new(NullCapture(self.clone())))
        }
    }

    impl HostCapture for NullCapture {
        fn source(&self) -> TapSource {
            TapSource::new(|| Box::new(ww_tap::NoTap))
        }
        fn set_options(&self, _: &CaptureOptions) {
            assert!(
                !self.0.panic_set_options.load(Ordering::Relaxed),
                "host bug"
            );
        }
        fn client_map(&self, _: ClientMap) {}
        fn save(&self, _: &SavePath) -> Result<Saved, String> {
            Ok(Saved::default())
        }
        fn watch(&self, _: Pressure) -> Result<Box<dyn HostWatch>, String> {
            Err("no watches".into())
        }
        fn stats(&self) -> CaptureStats {
            CaptureStats::default()
        }
        fn end(&self) {
            self.0.ends.fetch_add(1, Ordering::Relaxed);
        }
        fn discard(&self) {
            self.0.discards.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn registry() -> (Registry, Arc<NullHost>) {
        let host = Arc::new(NullHost::default());
        (Registry::new(Arc::new(host.clone())), host)
    }

    fn owner(instance: u64) -> Owner {
        Owner {
            principal: crate::claims::Principal::LocalUser,
            instance: crate::claims::Instance::Meta(instance),
        }
    }

    fn opts(frames: bool) -> CaptureOptions {
        CaptureOptions {
            record: RecordWhat {
                frames,
                messages: false,
            },
            links: LinkIds::ByPeer,
            ram_max: None,
        }
    }

    fn id_of(started: Started) -> CaptureId {
        match started {
            Started::Joined(id) | Started::New(id, _) => id,
        }
    }

    fn start(
        r: &Registry,
        who: u64,
        caller: Caller,
        key: &str,
    ) -> Result<CaptureId, Box<CaptureErrorOwned>> {
        r.start(
            owner(who),
            caller,
            who as u32,
            key,
            opts(who.is_multiple_of(2)),
        )
        .map(id_of)
    }

    /// Ends what the hub would end in the host, as `Hub::capture_ended` does.
    fn finish(ended: Ended) {
        match ended.discard {
            true => ended.cap.discard(),
            false => ended.cap.end(),
        }
    }

    #[test]
    fn a_capture_has_a_bounded_number_of_clients() {
        let (r, _) = registry();
        let id = start(&r, 1, Caller::Local, "dev").unwrap();
        for who in 2..=MAX_CAPTURE_CLIENTS as u64 {
            assert_eq!(start(&r, who, Caller::Local, "dev"), Ok(id));
        }
        assert_eq!(
            start(&r, 100, Caller::Local, "dev"),
            Err(CaptureErrorOwned::TooMany.into())
        );
        // one already in it asks again
        assert_eq!(start(&r, 1, Caller::Local, "dev"), Ok(id));
        let page = r.page(None);
        assert_eq!(page.entries[0].clients.len(), MAX_CAPTURE_CLIENTS);
    }

    #[test]
    fn a_relaying_hub_holds_few_captures() {
        let (r, _) = registry();
        for n in 0..MAX_RELAY_CAPTURES {
            start(&r, 7, Caller::Relay, &format!("dev{n}")).unwrap();
        }
        assert_eq!(
            start(&r, 7, Caller::Relay, "one more"),
            Err(CaptureErrorOwned::TooMany.into())
        );
        // joining again one it has is no new capture
        start(&r, 7, Caller::Relay, "dev0").unwrap();
        // a local program is not counted with it
        start(&r, 8, Caller::Local, "one more").unwrap();
    }

    #[test]
    fn the_allow_list_gives_a_relay_start_and_stop_only() {
        use Act::*;
        for act in [Start, Stop, Save, Discard, SetRamMax, List, Watch] {
            assert!(allowed(act, Caller::Local).is_ok());
            let relay = allowed(act, Caller::Relay);
            assert_eq!(relay.is_ok(), matches!(act, Start | Stop), "{act:?}");
        }
    }

    #[test]
    fn discard_and_ram_max_need_a_part() {
        let (r, host) = registry();
        let id = start(&r, 1, Caller::Local, "dev").unwrap();
        let ram = ram(1 << 20);
        let not_allowed =
            |e: Box<CaptureErrorOwned>| matches!(*e, CaptureErrorOwned::NotAllowed { .. });
        assert!(not_allowed(r.set_ram_max(&owner(2), id, ram).unwrap_err()));
        assert!(not_allowed(r.discard(&owner(2), id).err().unwrap()));
        assert_eq!(host.discards.load(Ordering::Relaxed), 0);
        r.set_ram_max(&owner(1), id, ram).unwrap();
        finish(r.discard(&owner(1), id).ok().unwrap());
        assert_eq!(host.discards.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn an_unsaved_capture_that_ends_is_discarded() {
        let (r, host) = registry();
        let id = start(&r, 1, Caller::Local, "unsaved").unwrap();
        finish(r.stop(&owner(1), id).unwrap().unwrap());
        start(&r, 2, Caller::Local, "left").unwrap();
        r.owner_left(&owner(2)).into_iter().for_each(finish);
        assert_eq!(host.discards.load(Ordering::Relaxed), 2);
        assert_eq!(host.ends.load(Ordering::Relaxed), 0);
        // a saved one is ended, its file kept
        let dir = dir().join("unsaved");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("saved.capture");
        let _ = std::fs::remove_file(&path);
        let id = start(&r, 3, Caller::Local, "saved").unwrap();
        r.save(id, path.to_str().unwrap()).unwrap();
        finish(r.stop(&owner(3), id).unwrap().unwrap());
        assert_eq!(host.ends.load(Ordering::Relaxed), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_panicking_host_leaves_the_registry_usable() {
        let (r, host) = registry();
        let id = start(&r, 1, Caller::Local, "dev").unwrap();
        host.panic_set_options.store(true, Ordering::Relaxed);
        // the second client changes what is recorded: the host's set_options panics
        let joined = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            start(&r, 2, Caller::Local, "dev")
        }));
        assert!(joined.is_err());
        host.panic_set_options.store(false, Ordering::Relaxed);
        assert_eq!(start(&r, 3, Caller::Local, "dev"), Ok(id));
        assert!(r.of_device("dev").is_some());
        assert_eq!(r.page(None).entries.len(), 1);
    }

    #[test]
    fn pages_hold_what_fits_one_message() {
        let (r, _) = registry();
        let long = "k".repeat(ww_hub_api::MAX_KEY_LEN - 4);
        for n in 0..MAX_CAPTURES {
            start(&r, n as u64, Caller::Local, &format!("{n:03}{long}")).unwrap();
        }
        let (mut seen, mut after, mut pages) = (vec![], None, 0);
        loop {
            let page = r.page(after);
            pages += 1;
            assert!(!page.entries.is_empty());
            let bytes: usize = page.entries.iter().map(entry_len).sum();
            assert!(bytes <= PAGE_ENTRIES_BYTES, "{bytes}");
            seen.extend(page.entries.iter().map(|e| e.id));
            after = seen.last().copied();
            if !page.more {
                break;
            }
        }
        assert!(pages > 1, "{pages}");
        assert_eq!(seen.len(), MAX_CAPTURES);
        assert!(seen.windows(2).all(|w| w[0] < w[1]));
    }

    /// A device is untapped only by the end of the capture it has: the late end of one before never takes a newer
    /// capture's taps off.
    #[test]
    fn a_late_untap_leaves_a_newer_capture() {
        let host = Arc::new(NullHost::default());
        let a: Arc<dyn HostCapture> = Arc::new(NullCapture(host.clone()));
        let b: Arc<dyn HostCapture> = Arc::new(NullCapture(host));
        let current = Some(b.clone());
        assert!(!crate::device::is_current(&current, &a));
        assert!(crate::device::is_current(&current, &b));
        assert!(!crate::device::is_current(&None, &b));
    }

    fn dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ww-hub-savepath-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn save_paths() {
        let dir = dir();
        let ok = dir.join("new.eventfull");
        let _ = std::fs::remove_file(&ok);
        assert_eq!(
            SavePath::check(ok.to_str().unwrap()).unwrap().path(),
            ok.as_path()
        );
        let refused = |p: &str| match SavePath::check(p).map_err(|e| *e) {
            Err(CaptureErrorOwned::BadPath { .. }) => {}
            other => panic!("{p}: {other:?}"),
        };
        refused("relative.eventfull");
        refused("");
        refused(&format!("{}/../x", dir.display()));
        refused(&format!("{}/./x", dir.display()));
        refused(&format!("{}/no_such_dir/x", dir.display()));
        refused(&format!("{}/", dir.display()));
        refused(&format!("{}/a\0b", dir.display()));
        // the directory names a file
        let file = dir.join("a_file");
        std::fs::write(&file, b"").unwrap();
        refused(&format!("{}/x", file.display()));
        // an existing file, and a symlink to one that doesn't exist yet, are never written
        assert_eq!(
            SavePath::check(file.to_str().unwrap()),
            Err(CaptureErrorOwned::Exists.into())
        );
        #[cfg(unix)]
        {
            let link = dir.join("link");
            let _ = std::fs::remove_file(&link);
            std::os::unix::fs::symlink(dir.join("target_not_there"), &link).unwrap();
            assert_eq!(
                SavePath::check(link.to_str().unwrap()),
                Err(CaptureErrorOwned::Exists.into())
            );
        }
        // created only once, readable by the hub's user alone
        let p = SavePath::check(ok.to_str().unwrap()).unwrap();
        let file = p.create().unwrap();
        assert_eq!(p.create().unwrap_err().kind(), io::ErrorKind::AlreadyExists);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(file.metadata().unwrap().permissions().mode() & 0o777, 0o600);
        }
        drop(file);
        // a failed save takes its file away, never one put there since
        p.remove_created();
        assert!(!ok.exists());
        let p = SavePath::check(ok.to_str().unwrap()).unwrap();
        p.create().unwrap();
        std::fs::remove_file(&ok).unwrap();
        std::fs::write(&ok, b"someone else's").unwrap();
        p.remove_created();
        assert_eq!(std::fs::read(&ok).unwrap(), b"someone else's");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
