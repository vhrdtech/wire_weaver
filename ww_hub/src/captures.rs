//! Captures (SRV-64, `ww_hub_api::Captures`): the hub records a device's link while a capture of it exists, through
//! the [CaptureHost] its host gave it ([crate::HubConfig::captures]). The hub keeps who asked for which capture, taps
//! the device, checks `save` paths and serves watches; the host (eventfull) keeps the records.
//!
//! Trust: every connection is the hub's own OS user (the socket lets no one else in), so a capture, a watch or a
//! saved file gives a client nothing it couldn't do itself. `save` still writes a file at a path another process
//! chose: [SavePath] refuses anything but a new file in an existing directory, so nothing is overwritten. A relaying
//! hub (federation, SRV-33, has no trust model yet) may not save or watch. Every list and string crossing the API is
//! bounded by a constant of `ww_hub_api`.

use std::collections::BTreeMap;
use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use wire_weaver_client::tap::TapSource;
use ww_hub_api::{
    CaptureEntryOwned, CaptureErrorOwned, CaptureId, CaptureOptions, CaptureStats, Pressure,
    RAM_MAX_DEFAULT, RamMax, RecordBatchOwned,
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

    /// The capture ends: its taps are taken off the device's links, a file it is saved to is closed as written,
    /// its watches end. Called once; nothing else is called after.
    fn end(&self);

    /// [HostCapture::end], and what is in RAM goes.
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavePath(PathBuf);

impl SavePath {
    /// Refuses what a client could not mean or what would overwrite something: the hub writes as its own user at a
    /// path another process chose. A relative path would land in the hub's working directory, not the client's.
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
        Ok(SavePath(p.to_path_buf()))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// Creates the file, refusing one that exists by now (`O_EXCL`: a symlink put there since the check isn't
    /// followed either).
    pub fn create(&self) -> io::Result<std::fs::File> {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&self.0)
    }
}

/// The hub's captures: who asked for which, and the host's object of each.
pub(crate) struct Registry {
    pub(crate) host: Arc<dyn CaptureHost>,
    state: Mutex<State>,
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
    /// Client instances that started it: the connection that asked (for `captures()`) and what it asked for
    asks: Vec<Ask>,
    cap: Arc<dyn HostCapture>,
    saved_to: Option<String>,
    /// Open watches, over every connection
    watches: Arc<AtomicU32>,
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

/// A capture that ended: its device is untapped.
pub(crate) struct Ended {
    pub(crate) key: String,
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

impl Registry {
    pub(crate) fn new(host: Arc<dyn CaptureHost>) -> Self {
        Registry {
            host,
            state: Mutex::new(State::default()),
        }
    }

    /// `owner` (on `connection`) records `key`.
    pub(crate) fn start(
        &self,
        owner: Owner,
        connection: u32,
        key: &str,
        options: CaptureOptions,
    ) -> Result<Started, Box<CaptureErrorOwned>> {
        let options = with_ram(options);
        let mut st = self.state.lock().unwrap();
        if let Some((id, e)) = st.by_id.iter_mut().find(|(_, e)| e.key == key) {
            let id = CaptureId(*id);
            match e.asks.iter_mut().find(|a| a.owner == owner) {
                Some(a) => a.options = options,
                None => e.asks.push(Ask {
                    owner,
                    connection,
                    options,
                }),
            }
            let options = merged(&e.asks, e.options.links);
            if options != e.options {
                e.options = options;
                e.cap.set_options(&options);
            }
            return Ok(Started::Joined(id));
        }
        if st.by_id.len() >= ww_hub_api::MAX_CAPTURES {
            return Err(CaptureErrorOwned::TooMany.into());
        }
        let cap = self
            .host
            .start(key, &options)
            .map_err(|why| CaptureErrorOwned::Host { why })?;
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
        let st = self.state.lock().unwrap();
        st.by_id
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
        let mut st = self.state.lock().unwrap();
        let e = st
            .by_id
            .get_mut(&id.0)
            .ok_or(CaptureErrorOwned::UnknownCapture)?;
        let before = e.asks.len();
        e.asks.retain(|a| a.owner != *owner);
        if e.asks.len() == before {
            return Err(CaptureErrorOwned::UnknownCapture.into());
        }
        if e.asks.is_empty() {
            return Ok(Some(end(&mut st, id.0, false)));
        }
        let options = merged(&e.asks, e.options.links);
        if options != e.options {
            e.options = options;
            e.cap.set_options(&options);
        }
        Ok(None)
    }

    /// Capture `id` ends for everyone, what is in RAM goes.
    pub(crate) fn discard(&self, id: CaptureId) -> Result<Ended, Box<CaptureErrorOwned>> {
        let mut st = self.state.lock().unwrap();
        if !st.by_id.contains_key(&id.0) {
            return Err(CaptureErrorOwned::UnknownCapture.into());
        }
        Ok(end(&mut st, id.0, true))
    }

    /// Every capture whose last client instance was `owner`, which left: they end.
    pub(crate) fn owner_left(&self, owner: &Owner) -> Vec<Ended> {
        let mut st = self.state.lock().unwrap();
        let mut ending = vec![];
        for (id, e) in st.by_id.iter_mut() {
            let before = e.asks.len();
            e.asks.retain(|a| a.owner != *owner);
            if e.asks.is_empty() {
                ending.push(*id);
            } else if e.asks.len() != before {
                let options = merged(&e.asks, e.options.links);
                if options != e.options {
                    e.options = options;
                    e.cap.set_options(&options);
                }
            }
        }
        ending
            .into_iter()
            .map(|id| end(&mut st, id, false))
            .collect()
    }

    pub(crate) fn set_ram_max(
        &self,
        id: CaptureId,
        ram_max: RamMax,
    ) -> Result<(), Box<CaptureErrorOwned>> {
        let mut st = self.state.lock().unwrap();
        let e = st
            .by_id
            .get_mut(&id.0)
            .ok_or(CaptureErrorOwned::UnknownCapture)?;
        for a in &mut e.asks {
            a.options.ram_max = Some(ram_max);
        }
        e.options.ram_max = Some(ram_max);
        e.cap.set_options(&e.options);
        Ok(())
    }

    /// The host's capture `id` to save, with the path recorded as its file; refused when it is saved already.
    pub(crate) fn saving(
        &self,
        id: CaptureId,
        path: &SavePath,
    ) -> Result<Arc<dyn HostCapture>, Box<CaptureErrorOwned>> {
        let mut st = self.state.lock().unwrap();
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
        Ok(e.cap.clone())
    }

    /// The save of capture `id` failed: it is unsaved again.
    pub(crate) fn save_failed(&self, id: CaptureId) {
        if let Some(e) = self.state.lock().unwrap().by_id.get_mut(&id.0) {
            e.saved_to = None;
        }
    }

    /// A watch of capture `id`, counted until the guard goes.
    pub(crate) fn watch(
        &self,
        id: CaptureId,
        pressure: Pressure,
    ) -> Result<(Box<dyn HostWatch>, WatchCount), Box<CaptureErrorOwned>> {
        let (cap, count) = {
            let st = self.state.lock().unwrap();
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

    /// Every capture, for `Captures::captures`, with each client as `entry` makes it.
    pub(crate) fn entries(
        &self,
        entry: impl Fn(u32) -> ww_hub_api::ClientEntryOwned,
    ) -> Vec<CaptureEntryOwned> {
        let st = self.state.lock().unwrap();
        st.by_id
            .iter()
            .map(|(id, e)| CaptureEntryOwned {
                id: CaptureId(*id),
                key: e.key.clone(),
                options: e.options,
                saved_to: e.saved_to.clone(),
                clients: wire_weaver::shrink_wrap::MaxLen::new(
                    e.asks.iter().map(|a| entry(a.connection)).collect(),
                )
                .expect("a capture has fewer clients than MAX_CAPTURES: one per client instance"),
                watches: e.watches.load(Ordering::Relaxed),
                stats: e.cap.stats(),
            })
            .collect()
    }
}

/// Removes capture `id` and ends it in the host.
fn end(st: &mut State, id: u32, discard: bool) -> Ended {
    let e = st
        .by_id
        .remove(&id)
        .expect("ended captures are in the registry");
    match discard {
        true => e.cap.discard(),
        false => e.cap.end(),
    }
    Ended { key: e.key }
}

/// Counts one open watch of a capture while it lives.
pub(crate) struct WatchCount(Arc<AtomicU32>);

impl Drop for WatchCount {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

impl crate::Hub {
    /// `Captures::start` of client `connection`.
    pub(crate) async fn capture_start(
        self: &Arc<Self>,
        connection: u32,
        key: &str,
        options: CaptureOptions,
    ) -> Result<CaptureId, Box<CaptureErrorOwned>> {
        let registry = self.registry()?;
        if !crate::device::has_transport(key) || self.lent_to(key).is_some() {
            return Err(CaptureErrorOwned::UnknownDevice.into());
        }
        let owner = crate::owner_of(&self.state.lock().unwrap(), connection);
        match registry.start(owner, connection, key, options)? {
            Started::Joined(id) => Ok(id),
            // opened with the tap if the hub doesn't hold it yet, else tapped now; kept open while recorded
            Started::New(id, cap) => {
                match self
                    .send_to_device(key, crate::device::ToDevice::Capture(Some(cap)))
                    .await
                {
                    Ok(()) => Ok(id),
                    Err(status) => {
                        tracing::debug!("ww hub: capture of {key} can't open it: {status}");
                        let _ = registry.discard(id);
                        Err(CaptureErrorOwned::UnknownDevice.into())
                    }
                }
            }
        }
    }

    /// A capture ended: its device, if open, is untapped and lingers as usual.
    pub(crate) fn capture_ended(self: &Arc<Self>, ended: Ended) {
        let hub = self.clone();
        tokio::spawn(async move {
            let slot = hub.device_slot(&ended.key);
            if let Some(handle) = slot.lock().await.as_ref() {
                let _ = handle.tx.send(crate::device::ToDevice::Capture(None));
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

    /// Whether client `connection` may save captures and watch them: they write files and read traffic on this PC.
    /// Deny by default: every principal is named here, and a relaying hub (`ClientMeta` kind `Hub`, the federation
    /// path, SRV-33) is refused until federation has a trust model. The kind is the client's own word, so it only
    /// ever takes rights away.
    pub(crate) fn may_save_or_watch(&self, connection: u32) -> Result<(), Box<CaptureErrorOwned>> {
        let state = self.state.lock().unwrap();
        match crate::owner_of(&state, connection).principal {
            crate::claims::Principal::LocalUser => {}
        }
        let relayed = state
            .clients
            .get(&connection)
            .and_then(|c| c.meta.as_ref())
            .is_some_and(|m| m.kind == Some(ww_device::ww_link::ClientKind::Hub));
        match relayed {
            true => Err(CaptureErrorOwned::NotAllowed {
                why: "a relaying hub may not save or watch captures".into(),
            }
            .into()),
            false => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // created only once
        let p = SavePath::check(ok.to_str().unwrap()).unwrap();
        p.create().unwrap();
        assert_eq!(p.create().unwrap_err().kind(), io::ErrorKind::AlreadyExists);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
