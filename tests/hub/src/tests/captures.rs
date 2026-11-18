//! Captures through the hub (SRV-64): a capture host in memory ([MemHost]) and the hub's `Captures` API over its real
//! socket.

use std::collections::VecDeque;
use std::future::Future;
use std::io::Write as _;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use wire_weaver::shrink_wrap::{MaxLen, SerializeShrinkWrap};
use wire_weaver_client::tap::TapSource;
use ww_hub::{CaptureHost, HostCapture, HostWatch, SavePath, Saved};
use ww_hub_api::{
    Behind, CaptureErrorOwned, CaptureId, CaptureOptions, CaptureStats, GapCause, LinkIds, LinkNo,
    MAX_BATCH_ENTRIES, Pressure, RamMax, RecordBatchOwned, RecordWhat, TapStream, WatchEntryOwned,
};
use ww_si::prefix::One;
use ww_si::{Bytes, Fixed, Nanos};
use ww_tap::{ClientMap, Dir, LinkEvent, Session, Tap};

use super::*;

/// Records in memory: what [MemCapture]s there were, for the tests to look at. Every change of a capture is told on
/// `changed`: the tests wait on it, never on the clock.
#[derive(Default)]
struct MemHost {
    captures: Mutex<Vec<Arc<MemCapture>>>,
    changed: Arc<tokio::sync::Notify>,
}

impl CaptureHost for MemHost {
    fn start(&self, key: &str, options: &CaptureOptions) -> Result<Arc<dyn HostCapture>, String> {
        let cap = Arc::new_cyclic(|me| MemCapture {
            me: me.clone(),
            key: key.to_string(),
            options: Mutex::new(*options),
            ring: Mutex::new(Ring::default()),
            notify: tokio::sync::Notify::new(),
            ended: AtomicBool::new(false),
            discarded: AtomicBool::new(false),
            maps: Mutex::new(vec![]),
            saved: Mutex::new(None),
            links: AtomicU64::new(0),
            taps_dropped: AtomicU64::new(0),
            fail_save: AtomicBool::new(false),
            panic_messages: AtomicBool::new(false),
            started: std::time::Instant::now(),
            changed: self.changed.clone(),
        });
        self.captures.lock().unwrap().push(cap.clone());
        self.changed.notify_waiters();
        Ok(cap)
    }
}

impl MemHost {
    fn only(&self) -> Arc<MemCapture> {
        let caps = self.captures.lock().unwrap();
        assert_eq!(caps.len(), 1, "one capture started");
        caps[0].clone()
    }

    fn nth(&self, n: usize) -> Arc<MemCapture> {
        self.captures.lock().unwrap()[n].clone()
    }

    /// Waits until `f` holds (`_what`, for the reader): checked at every change of a capture. A wait that never ends
    /// is nextest's to stop.
    async fn until(&self, _what: &str, f: impl Fn() -> bool) {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if f() {
                return;
            }
            changed.await;
        }
    }
}

/// One record of a [MemCapture].
#[derive(Clone)]
struct Rec {
    no: u64,
    link: LinkNo,
    stream: TapStream,
    tick: u64,
    item: Vec<u8>,
}

/// Records kept up to the capture's RAM limit, the oldest going first.
#[derive(Default)]
struct Ring {
    recs: VecDeque<Rec>,
    bytes: u64,
    next_no: u64,
    dropped_ram: u64,
    /// Records since the save, written to its file at the end
    after_save: Vec<Rec>,
}

struct MemCapture {
    me: std::sync::Weak<MemCapture>,
    key: String,
    options: Mutex<CaptureOptions>,
    ring: Mutex<Ring>,
    notify: tokio::sync::Notify,
    ended: AtomicBool,
    discarded: AtomicBool,
    maps: Mutex<Vec<ClientMap>>,
    /// The file it is saved to
    saved: Mutex<Option<std::fs::File>>,
    links: AtomicU64,
    /// Taps of its links dropped: a device untapped drops its link's tap
    taps_dropped: AtomicU64,
    /// `save` creates the file and then fails
    fail_save: AtomicBool,
    /// The taps panic on messages (a host's bug)
    panic_messages: AtomicBool,
    started: std::time::Instant,
    changed: Arc<tokio::sync::Notify>,
}

impl MemCapture {
    fn push(&self, link: LinkNo, stream: TapStream, item: Vec<u8>) {
        if self.ended.load(Ordering::Relaxed) {
            return;
        }
        let ram_max = self
            .options
            .lock()
            .unwrap()
            .ram_max
            .map_or(u64::MAX, |r| r.get());
        let mut ring = self.ring.lock().unwrap();
        let rec = Rec {
            no: ring.next_no,
            link,
            stream,
            tick: self.started.elapsed().as_nanos() as u64,
            item,
        };
        ring.next_no += 1;
        ring.bytes += rec.item.len() as u64;
        if self.saved.lock().unwrap().is_some() {
            ring.after_save.push(rec.clone());
        }
        ring.recs.push_back(rec);
        while ring.bytes > ram_max {
            let old = ring.recs.pop_front().expect("bytes are of records kept");
            ring.bytes -= old.item.len() as u64;
            ring.dropped_ram += 1;
        }
        drop(ring);
        self.notify.notify_waiters();
        self.changed.notify_waiters();
    }

    fn count(&self, stream: TapStream) -> usize {
        let ring = self.ring.lock().unwrap();
        ring.recs.iter().filter(|r| r.stream == stream).count()
    }

    fn maps(&self) -> Vec<ClientMap> {
        self.maps.lock().unwrap().clone()
    }
}

fn write_recs(file: &mut std::fs::File, recs: &[Rec]) -> std::io::Result<()> {
    for r in recs {
        writeln!(
            file,
            "{} {:?} {:?} {}",
            r.no,
            r.link,
            r.stream,
            r.item.len()
        )?;
    }
    Ok(())
}

impl HostCapture for MemCapture {
    fn source(&self) -> TapSource {
        let cap = self.self_arc();
        TapSource::new(move || {
            let link = LinkNo(cap.links.fetch_add(1, Ordering::Relaxed) as u32);
            Box::new(MemTap {
                cap: cap.clone(),
                link,
            })
        })
    }

    fn set_options(&self, options: &CaptureOptions) {
        *self.options.lock().unwrap() = *options;
        self.changed.notify_waiters();
    }

    fn client_map(&self, map: ClientMap) {
        self.maps.lock().unwrap().push(map);
        self.changed.notify_waiters();
    }

    fn save(&self, to: &SavePath) -> Result<Saved, String> {
        let mut file = to.create().map_err(|e| e.to_string())?;
        if self.fail_save.load(Ordering::Relaxed) {
            let _ = file.write_all(b"half a file");
            return Err("disk full".into());
        }
        let ring = self.ring.lock().unwrap();
        let recs: Vec<Rec> = ring.recs.iter().cloned().collect();
        write_recs(&mut file, &recs).map_err(|e| e.to_string())?;
        *self.saved.lock().unwrap() = Some(file);
        Ok(Saved {
            records: recs.len() as u64,
            dropped: ring.dropped_ram,
        })
    }

    fn watch(&self, pressure: Pressure) -> Result<Box<dyn HostWatch>, String> {
        let next = self.ring.lock().unwrap().next_no;
        Ok(Box::new(MemWatch {
            cap: self.self_arc(),
            next,
            pressure,
            announced: Default::default(),
        }))
    }

    fn stats(&self) -> CaptureStats {
        let ring = self.ring.lock().unwrap();
        CaptureStats {
            links: self.links.load(Ordering::Relaxed) as u32,
            records: Fixed::new(ring.next_no),
            bytes: Bytes::new(ring.bytes),
            ram: Bytes::new(ring.bytes),
            dropped_producer: Fixed::new(0),
            dropped_ram: Fixed::new(ring.dropped_ram),
        }
    }

    fn end(&self) {
        self.ended.store(true, Ordering::Relaxed);
        if let Some(mut file) = self.saved.lock().unwrap().take() {
            let after = std::mem::take(&mut self.ring.lock().unwrap().after_save);
            let _ = write_recs(&mut file, &after);
        }
        self.notify.notify_waiters();
        self.changed.notify_waiters();
    }

    fn discard(&self) {
        self.discarded.store(true, Ordering::Relaxed);
        self.end();
        let mut ring = self.ring.lock().unwrap();
        ring.recs.clear();
        ring.bytes = 0;
        drop(ring);
        self.changed.notify_waiters();
    }
}

impl MemCapture {
    fn self_arc(&self) -> Arc<MemCapture> {
        self.me
            .upgrade()
            .expect("a capture in use is held by the hub")
    }
}

/// The tap of one link: encodes each record and pushes it, never waiting on anything but the ring's short lock.
struct MemTap {
    cap: Arc<MemCapture>,
    link: LinkNo,
}

impl Drop for MemTap {
    fn drop(&mut self) {
        self.cap.taps_dropped.fetch_add(1, Ordering::Relaxed);
        self.cap.changed.notify_waiters();
    }
}

impl MemTap {
    fn push(&self, stream: TapStream, item: &impl SerializeShrinkWrap) {
        let mut buf = vec![0u8; 64 * 1024];
        if let Ok(bytes) = item.to_ww_bytes(&mut buf) {
            self.cap.push(self.link, stream, bytes.to_vec());
        }
    }
}

impl Tap for MemTap {
    fn wants_frames(&self) -> bool {
        self.cap.options.lock().unwrap().record.frames
    }

    fn wants_messages(&self) -> bool {
        self.cap.options.lock().unwrap().record.messages
    }

    fn session(&mut self, session: &Session<'_>) {
        self.push(TapStream::Session, session);
    }

    fn frame(&mut self, dir: Dir, bytes: &[u8]) {
        let stream = match dir {
            Dir::Tx => TapStream::TxFrames,
            Dir::Rx => TapStream::RxFrames,
        };
        self.cap.push(self.link, stream, bytes.to_vec());
    }

    fn message(&mut self, dir: Dir, _kind: u8, bytes: &[u8]) {
        assert!(
            !self.cap.panic_messages.load(Ordering::Relaxed),
            "a capture host's bug"
        );
        let stream = match dir {
            Dir::Tx => TapStream::TxMessages,
            Dir::Rx => TapStream::RxMessages,
        };
        self.cap.push(self.link, stream, bytes.to_vec());
    }

    fn link_event(&mut self, event: &LinkEvent<'_>) {
        self.push(TapStream::Events, event);
    }
}

/// A cursor over a [MemCapture]: from the record after the newest at its start.
struct MemWatch {
    cap: Arc<MemCapture>,
    next: u64,
    pressure: Pressure,
    announced: std::collections::BTreeSet<LinkNo>,
}

impl MemWatch {
    fn batch(&mut self, max_bytes: usize) -> Option<RecordBatchOwned> {
        let ring = self.cap.ring.lock().unwrap();
        let mut entries = vec![];
        let oldest = ring.recs.front().map_or(ring.next_no, |r| r.no);
        // real time skips to the newest whatever went meanwhile, at the RAM limit or not
        if let Pressure::RealTime {
            behind_max: Behind::Records(max),
        } = self.pressure
        {
            let behind = ring.next_no - self.next;
            if behind > max.get() {
                let skip_to = ring.next_no - max.get();
                entries.push(WatchEntryOwned::Gap {
                    link: LinkNo(0),
                    stream: TapStream::Events,
                    records: Fixed::new(skip_to - self.next),
                    from: Nanos::new(0),
                    to: Nanos::new(0),
                    cause: GapCause::FellBehind,
                });
                self.next = skip_to;
            }
        }
        if self.next < oldest {
            // dropped at the RAM limit before this watch read them
            entries.push(WatchEntryOwned::Gap {
                link: LinkNo(0),
                stream: TapStream::Events,
                records: Fixed::new(oldest - self.next),
                from: Nanos::new(0),
                to: Nanos::new(0),
                cause: GapCause::RamLimit,
            });
            self.next = oldest;
        }
        let mut len = 64;
        for r in ring.recs.iter().skip((self.next - oldest) as usize) {
            if entries.len() + 2 > MAX_BATCH_ENTRIES || len + r.item.len() + 32 > max_bytes {
                break;
            }
            if self.announced.insert(r.link) {
                entries.push(WatchEntryOwned::Link {
                    link: r.link,
                    key: MaxLen::new_unchecked(vec![]),
                });
            }
            len += r.item.len() + 32;
            entries.push(WatchEntryOwned::Record {
                link: r.link,
                stream: r.stream,
                tick: Nanos::new(r.tick),
                item: MaxLen::new_unchecked(r.item.clone()),
            });
            self.next = r.no + 1;
        }
        (!entries.is_empty()).then(|| RecordBatchOwned {
            entries: MaxLen::new_unchecked(entries),
        })
    }
}

impl HostWatch for MemWatch {
    fn next(
        &mut self,
        max_bytes: usize,
    ) -> Pin<Box<dyn Future<Output = Option<RecordBatchOwned>> + Send + '_>> {
        Box::pin(async move {
            loop {
                let cap = self.cap.clone();
                let notified = cap.notify.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if self.cap.ended.load(Ordering::Relaxed) {
                    return None;
                }
                if let Some(batch) = self.batch(max_bytes) {
                    return Some(batch);
                }
                notified.await;
            }
        })
    }
}

// ---- the bench ----

fn recording(name: &str) -> (Bench, Arc<MemHost>) {
    let host = Arc::new(MemHost::default());
    let bench = start_hub(
        name,
        Duration::from_secs(30),
        None,
        Default::default(),
        Some(host.clone()),
    );
    (bench, host)
}

fn options(frames: bool, messages: bool) -> CaptureOptions {
    CaptureOptions {
        record: RecordWhat { frames, messages },
        links: LinkIds::ByPeer,
        ram_max: None,
    }
}

impl Bench {
    async fn start_capture(&self, p: &Program, options: CaptureOptions) -> CaptureId {
        p.hub
            .captures()
            .start(MaxLen::new(self.key.clone()).unwrap(), options)
            .call()
            .await
            .expect("capture started")
    }
}

/// Every capture, page by page.
async fn entries_of(hub: &HubClient) -> Vec<ww_hub_api::CaptureEntryOwned> {
    let (mut all, mut after) = (vec![], None);
    loop {
        let page = hub.captures().captures(after).call().await.unwrap();
        all.extend(page.entries.into_inner());
        after = all.last().map(|e| e.id);
        if !page.more {
            return all;
        }
    }
}

async fn entries(p: &Program) -> Vec<ww_hub_api::CaptureEntryOwned> {
    entries_of(&p.hub).await
}

/// The program name of the client on connection `id`.
async fn program_of(p: &Program, id: u32) -> String {
    let clients = p.hub.registry().clients().call().await.unwrap();
    clients
        .into_iter()
        .find(|c| c.id == id)
        .map(|c| c.program)
        .unwrap_or_default()
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ww-hub-captures-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

// ---- tests ----

/// A hub without a capture host says so: `NotAvailable` naming the hub, `HubStatus::captures` false.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn not_available_without_a_host() {
    let bench = start("cap-none", Duration::from_secs(30));
    let a = bench.program("alice").await;
    assert!(!a.hub.status().call().await.unwrap().captures);
    let e = device_error(
        a.hub
            .captures()
            .start(MaxLen::new(bench.key.clone()).unwrap(), options(true, true))
            .call()
            .await
            .expect_err("no host"),
    );
    match &e {
        CaptureErrorOwned::NotAvailable { hub, .. } => {
            assert_eq!(hub.hub_version, ww_hub::HUB_VERSION);
        }
        other => panic!("{other:?}"),
    }
    let text = e.to_string();
    assert!(
        text.contains("built without it") && text.contains("ww hub stop"),
        "{text}"
    );
    let e = device_error(
        a.hub
            .captures()
            .captures(None)
            .call()
            .await
            .expect_err("no host"),
    );
    assert!(matches!(e, CaptureErrorOwned::NotAvailable { .. }), "{e:?}");
    bench.stop().await;
}

/// Start taps the device (an open one now, through `Command::SetTap`), stop takes the tap off.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn start_and_stop_tap_an_open_device() {
    let (bench, host) = recording("cap-open");
    let a = bench.program("alice").await;
    assert!(a.hub.status().call().await.unwrap().captures);
    // the device is open through a's tunnel before the capture
    assert_eq!(a.dev.add(1, 2).call().await.unwrap(), 3);
    let id = bench.start_capture(&a, options(true, true)).await;
    let cap = host.only();
    assert_eq!(cap.key, bench.key);
    // the tap set later starts with the link's session and its readiness
    host.until("session", || cap.count(TapStream::Session) == 1)
        .await;
    assert_eq!(a.dev.add(2, 3).call().await.unwrap(), 5);
    host.until("request and answer", || {
        cap.count(TapStream::TxMessages) >= 1 && cap.count(TapStream::RxMessages) >= 1
    })
    .await;
    let listed = entries(&a).await;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, id);
    assert_eq!(listed[0].clients.len(), 1);
    assert_eq!(program_of(&a, listed[0].clients[0]).await, "alice");

    a.hub.captures().stop(id).call().await.unwrap();
    // unsaved: what it recorded goes
    assert!(cap.discarded.load(Ordering::Relaxed));
    assert!(entries(&a).await.is_empty());
    // the device drops the link's tap
    host.until("untapped", || {
        cap.taps_dropped.load(Ordering::Relaxed) == cap.links.load(Ordering::Relaxed)
    })
    .await;
    let before = cap.ring.lock().unwrap().next_no;
    assert_eq!(a.dev.add(3, 4).call().await.unwrap(), 7);
    assert_eq!(
        cap.ring.lock().unwrap().next_no,
        before,
        "no tap after the stop"
    );
    // stopped twice: no part left in it
    let e = device_error(a.hub.captures().stop(id).call().await.expect_err("gone"));
    assert_eq!(e, CaptureErrorOwned::UnknownCapture);
    bench.stop().await;
}

/// A capture of a device the hub doesn't hold opens it with the tap, from the link's first frame, and keeps it open.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn start_opens_the_device_tapped() {
    let (bench, host) = recording("cap-closed");
    // the hub's API only: nothing opened the device before
    let socket = bench.socket();
    let hub = HubClient::config(|c| c.hub_api_at(socket.clone()))
        .connect()
        .await
        .unwrap();
    hub.captures()
        .start(
            MaxLen::new(bench.key.clone()).unwrap(),
            options(true, false),
        )
        .call()
        .await
        .unwrap();
    let cap = host.only();
    host.until("session at open", || cap.count(TapStream::Session) >= 1)
        .await;
    let opened = cap.ring.lock().unwrap().recs.iter().any(|r| {
        r.stream == TapStream::Events
            && LinkEvent::from_ww_bytes(&r.item).is_ok_and(|e| e == LinkEvent::Connected)
    });
    assert!(opened, "recorded from the link's start");
    // messages not asked for
    assert_eq!(cap.count(TapStream::TxMessages), 0);
    // the hub's exit ends every capture
    bench.stop().await;
    assert!(cap.ended.load(Ordering::Relaxed));
}

/// Two programs share one capture: one id, what they record united, the capture ends with the last one, also when
/// a program just goes away.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_callers_share_one_capture() {
    let (bench, host) = recording("cap-shared");
    let a = bench.program("alice").await;
    let b = bench.program("bob").await;
    let id = bench.start_capture(&a, options(true, false)).await;
    let same = bench.start_capture(&b, options(false, true)).await;
    assert_eq!(id, same);
    let cap = host.only();
    let merged = *cap.options.lock().unwrap();
    assert_eq!(
        merged.record,
        RecordWhat {
            frames: true,
            messages: true
        }
    );
    assert_eq!(
        merged.ram_max.map(|r| r.get()),
        Some(ww_hub_api::RAM_MAX_DEFAULT)
    );
    assert_eq!(entries(&a).await[0].clients.len(), 2);

    a.hub.captures().stop(id).call().await.unwrap();
    assert!(!cap.ended.load(Ordering::Relaxed), "bob still records");
    assert_eq!(
        cap.options.lock().unwrap().record,
        RecordWhat {
            frames: false,
            messages: true
        }
    );
    // bob goes away without stopping: the capture goes with him
    drop(b);
    host.until("ended with the last client", || {
        cap.ended.load(Ordering::Relaxed)
    })
    .await;
    assert!(cap.discarded.load(Ordering::Relaxed), "unsaved: discarded");
    assert!(entries(&a).await.is_empty());
    bench.stop().await;
}

/// `set_ram_max` reaches the host; `discard` ends the capture for everyone. Both only for a client with a part in it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ram_max_and_discard() {
    let (bench, host) = recording("cap-discard");
    let a = bench.program("alice").await;
    let b = bench.program("bob").await;
    let c = bench.program("carol").await;
    let id = bench.start_capture(&a, options(true, true)).await;
    bench.start_capture(&b, options(true, true)).await;
    let cap = host.only();
    let ram = RamMax::try_new(1 << 20).ok().unwrap();
    let not_allowed = |e| matches!(device_error(e), CaptureErrorOwned::NotAllowed { .. });
    assert!(not_allowed(
        c.hub
            .captures()
            .set_ram_max(id, ram)
            .call()
            .await
            .expect_err("no part")
    ));
    assert!(not_allowed(
        c.hub
            .captures()
            .discard(id)
            .call()
            .await
            .expect_err("no part")
    ));
    assert!(!cap.discarded.load(Ordering::Relaxed));
    a.hub.captures().set_ram_max(id, ram).call().await.unwrap();
    assert_eq!(cap.options.lock().unwrap().ram_max, Some(ram));
    assert_eq!(entries(&b).await[0].options.ram_max, Some(ram));

    a.hub.captures().discard(id).call().await.unwrap();
    assert!(cap.discarded.load(Ordering::Relaxed));
    assert!(entries(&b).await.is_empty());
    let e = device_error(b.hub.captures().stop(id).call().await.expect_err("gone"));
    assert_eq!(e, CaptureErrorOwned::UnknownCapture);
    bench.stop().await;
}

/// `save` writes a new file at an absolute path and nothing else; the file is closed when the capture ends.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn save_writes_only_a_new_file() {
    let (bench, host) = recording("cap-save");
    let dir = temp("save");
    let a = bench.program("alice").await;
    let id = bench.start_capture(&a, options(true, true)).await;
    assert_eq!(a.dev.add(1, 1).call().await.unwrap(), 2);
    let cap = host.only();
    host.until("records", || cap.count(TapStream::RxMessages) >= 1)
        .await;
    let save = |path: String| {
        let hub = &a.hub;
        async move {
            hub.captures()
                .save(id, MaxLen::new(path).unwrap())
                .call()
                .await
        }
    };

    let refused = |r: Result<_, _>| device_error(r.expect_err("refused"));
    assert!(matches!(
        refused(save("relative.capture".into()).await),
        CaptureErrorOwned::BadPath { .. }
    ));
    assert!(matches!(
        refused(save(format!("{}/../x", dir.display())).await),
        CaptureErrorOwned::BadPath { .. }
    ));
    assert!(matches!(
        refused(save(format!("{}/nope/x", dir.display())).await),
        CaptureErrorOwned::BadPath { .. }
    ));
    let existing = dir.join("existing");
    std::fs::write(&existing, b"keep me").unwrap();
    assert_eq!(
        refused(save(existing.display().to_string()).await),
        CaptureErrorOwned::Exists
    );
    assert_eq!(std::fs::read(&existing).unwrap(), b"keep me");

    // a save that fails takes its half-written file away: the same path works next time
    let file = dir.join("run.capture");
    cap.fail_save.store(true, Ordering::Relaxed);
    assert!(matches!(
        refused(save(file.display().to_string()).await),
        CaptureErrorOwned::Host { .. }
    ));
    assert!(!file.exists());
    assert_eq!(entries(&a).await[0].saved_to, None);
    cap.fail_save.store(false, Ordering::Relaxed);
    let info = save(file.display().to_string()).await.expect("saved");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&file).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "the user's alone");
    }
    assert_eq!(info.path, file.display().to_string());
    assert!(info.records.get() >= 2);
    let at_save = std::fs::read_to_string(&file).unwrap().lines().count();
    assert_eq!(at_save as u64, info.records.get());
    assert_eq!(
        entries(&a).await[0].saved_to.as_deref(),
        Some(file.display().to_string().as_str())
    );
    // once
    let other = dir.join("other.capture");
    assert!(matches!(
        refused(save(other.display().to_string()).await),
        CaptureErrorOwned::Saved { .. }
    ));
    assert!(!other.exists());
    // what came after the save is in the file once the capture ends
    assert_eq!(a.dev.add(2, 2).call().await.unwrap(), 4);
    a.hub.captures().stop(id).call().await.unwrap();
    assert!(!cap.discarded.load(Ordering::Relaxed), "saved: ended, kept");
    let at_end = std::fs::read_to_string(&file).unwrap().lines().count();
    assert!(at_end > at_save, "{at_end} > {at_save}");
    let _ = std::fs::remove_dir_all(&dir);
    bench.stop().await;
}

/// The hub forwards each request's client and seq to the capture as the request goes out, so one that is never
/// answered is in the map too.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn client_map_at_send() {
    let (bench, host) = recording("cap-map");
    let a = bench.program("alice").await;
    assert_eq!(a.dev.add(1, 2).call().await.unwrap(), 3);
    bench.start_capture(&a, options(false, true)).await;
    let cap = host.only();
    host.until("session", || cap.count(TapStream::Session) == 1)
        .await;
    let before = cap.maps().len();
    assert_eq!(a.dev.add(5, 5).call().await.unwrap(), 10);
    host.until("answered request mapped", || cap.maps().len() == before + 1)
        .await;
    // never answered: the device drops it
    bench.device.drop_requests(true);
    let pending = a.dev.add(6, 6).send().await.unwrap();
    host.until("unanswered request mapped", || {
        cap.maps().len() == before + 2
    })
    .await;
    bench.device.drop_requests(false);
    drop(pending);
    let maps = cap.maps();
    let (answered, unanswered) = (maps[before], maps[before + 1]);
    assert_eq!(answered.client, unanswered.client, "one tunnel");
    assert_ne!(answered.device_seq, unanswered.device_seq);
    assert_ne!(answered.client_seq, unanswered.client_seq);
    bench.stop().await;
}

/// A watch reads the capture as its pressure says: real time skips what it fell behind (a gap), everything reads
/// what is still in RAM and reports what the RAM limit dropped. An unread watch never holds up the device.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn watches_skip_never_stall_the_device() {
    let (bench, host) = recording("cap-watch");
    let mut a = bench.program("alice").await;
    let id = bench.start_capture(&a, options(true, true)).await;
    let cap = host.only();
    host.until("session", || cap.count(TapStream::Session) >= 1)
        .await;

    let behind_max = Fixed::<_, One, u64>::new(10);
    let realtime = a
        .hub
        .captures()
        .watch(
            id,
            Pressure::RealTime {
                behind_max: Behind::Records(behind_max),
            },
        )
        .call()
        .await
        .unwrap();
    let everything = a
        .hub
        .captures()
        .watch(id, Pressure::Everything)
        .call()
        .await
        .unwrap();
    assert_ne!(realtime, everything);
    // nobody reads either watch meanwhile: the device's calls go on
    let ram = RamMax::try_new(ww_hub_api::RAM_MAX_MIN).ok().unwrap();
    a.hub.captures().set_ram_max(id, ram).call().await.unwrap();
    for base in 0..40 {
        calls(&mut a.dev, base * 100).await;
    }
    assert!(
        cap.ring.lock().unwrap().dropped_ram > 0,
        "past the RAM limit"
    );
    assert_eq!(entries(&a).await[0].watches, 2);

    let mut rt = a
        .hub
        .captures()
        .watches(realtime.0)
        .records()
        .call()
        .await
        .unwrap();
    let batch = rt.recv().await.unwrap();
    let gap = batch.entries.iter().find_map(|e| match e {
        WatchEntryOwned::Gap { cause, records, .. } => Some((*cause, records.get())),
        _ => None,
    });
    let (cause, skipped) = gap.expect("a gap first");
    assert_eq!(cause, GapCause::FellBehind);
    assert!(skipped > 100, "{skipped}");
    let records = batch
        .entries
        .iter()
        .filter(|e| matches!(e, WatchEntryOwned::Record { .. }))
        .count();
    assert!(records <= 10, "only the newest: {records}");

    let mut all = a
        .hub
        .captures()
        .watches(everything.0)
        .records()
        .call()
        .await
        .unwrap();
    let batch = all.recv().await.unwrap();
    match &batch.entries[0] {
        WatchEntryOwned::Gap { cause, .. } => assert_eq!(*cause, GapCause::RamLimit),
        other => panic!("{other:?}"),
    }

    // live: a new record reaches an open watch
    assert_eq!(a.dev.add(100, 1).call().await.unwrap(), 101);
    loop {
        let batch = rt.recv().await.expect("live records");
        let last = batch.entries.iter().rev().find_map(|e| match e {
            WatchEntryOwned::Record { stream, .. } => Some(*stream),
            _ => None,
        });
        if last == Some(TapStream::RxMessages) {
            break;
        }
    }
    // the watches end with the capture
    a.hub.captures().stop(id).call().await.unwrap();
    host.until("watches gone", || cap.ended.load(Ordering::Relaxed))
        .await;
    bench.stop().await;
}

/// More watches than `MAX_WATCHES` on one connection are refused.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn watches_are_bounded() {
    let (bench, _host) = recording("cap-many");
    let a = bench.program("alice").await;
    let id = bench.start_capture(&a, options(false, false)).await;
    for _ in 0..ww_hub_api::MAX_WATCHES {
        a.hub
            .captures()
            .watch(id, Pressure::Everything)
            .call()
            .await
            .unwrap();
    }
    let e = device_error(
        a.hub
            .captures()
            .watch(id, Pressure::Everything)
            .call()
            .await
            .expect_err("too many"),
    );
    assert_eq!(e, CaptureErrorOwned::TooMany);
    bench.stop().await;
}

/// A relaying hub (a client saying it is a hub, the federation path) may only start and stop captures: no save,
/// watch, list, discard or RAM limit.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn relaying_hub_may_not_save_or_watch() {
    let (bench, _host) = recording("cap-relay");
    let dir = temp("relay");
    let meta = ClientMeta::new()
        .program("other hub")
        .kind(wire_weaver_client::ClientKind::Hub);
    let socket = bench.socket();
    let hub = HubClient::config(|c| c.hub_api_at(socket.clone()).client_meta(meta.clone()))
        .connect()
        .await
        .unwrap();
    let id = hub
        .captures()
        .start(MaxLen::new(bench.key.clone()).unwrap(), options(true, true))
        .call()
        .await
        .unwrap();
    let file = dir.join("x.capture");
    let e = device_error(
        hub.captures()
            .save(id, MaxLen::new(file.display().to_string()).unwrap())
            .call()
            .await
            .expect_err("refused"),
    );
    assert!(matches!(e, CaptureErrorOwned::NotAllowed { .. }), "{e:?}");
    assert!(!file.exists());
    let e = device_error(
        hub.captures()
            .watch(id, Pressure::Everything)
            .call()
            .await
            .expect_err("refused"),
    );
    assert!(matches!(e, CaptureErrorOwned::NotAllowed { .. }), "{e:?}");
    let refused = |e| matches!(device_error(e), CaptureErrorOwned::NotAllowed { .. });
    assert!(refused(
        hub.captures()
            .captures(None)
            .call()
            .await
            .expect_err("refused")
    ));
    assert!(refused(
        hub.captures()
            .discard(id)
            .call()
            .await
            .expect_err("refused")
    ));
    let ram = RamMax::try_new(1 << 20).ok().unwrap();
    assert!(refused(
        hub.captures()
            .set_ram_max(id, ram)
            .call()
            .await
            .expect_err("refused")
    ));
    hub.captures().stop(id).call().await.unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    bench.stop().await;
}

/// A program killed with a capture and an open watch leaves nothing: the capture ends, its watch count drops.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn killed_client_leaves_nothing() {
    let (bench, host) = recording("cap-killed");
    let b = bench.program("bob").await;
    let (socket, key) = (bench.socket(), bench.key.clone());
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let hub = HubClient::config(|c| {
                c.hub_api_at(socket.clone())
                    .client_meta(ClientMeta::new().program("alice"))
            })
            .connect()
            .await
            .unwrap();
            let id = hub
                .captures()
                .start(MaxLen::new(key).unwrap(), options(true, true))
                .call()
                .await
                .unwrap();
            let w = hub
                .captures()
                .watch(id, Pressure::Everything)
                .call()
                .await
                .unwrap();
            let stream = hub.captures().watches(w.0).records().call().await.unwrap();
            // the runtime goes with everything on it, as a killed process would
            std::mem::forget(stream);
            std::mem::forget(hub);
        });
    })
    .join()
    .unwrap();
    let cap = host.only();
    host.until("ended with its client", || {
        cap.ended.load(Ordering::Relaxed)
    })
    .await;
    assert!(entries(&b).await.is_empty());
    bench.stop().await;
}

/// A capture started right after one of the same device ended keeps its taps: the late end of the first one never
/// takes them off.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_new_capture_keeps_its_taps() {
    let (bench, host) = recording("cap-again");
    let a = bench.program("alice").await;
    assert_eq!(a.dev.add(1, 2).call().await.unwrap(), 3);
    let rounds = 5;
    for n in 0..rounds {
        let id = bench.start_capture(&a, options(false, true)).await;
        if n + 1 < rounds {
            a.hub.captures().stop(id).call().await.unwrap();
        }
    }
    let last = host.nth(rounds - 1);
    // every earlier capture's tap goes; the last one's stays and records
    host.until("earlier taps dropped", || {
        (0..rounds - 1).all(|n| {
            let cap = host.nth(n);
            cap.taps_dropped.load(Ordering::Relaxed) == cap.links.load(Ordering::Relaxed)
        })
    })
    .await;
    assert_eq!(a.dev.add(2, 3).call().await.unwrap(), 5);
    host.until("the last capture records", || {
        last.count(TapStream::RxMessages) >= 1
    })
    .await;
    assert_eq!(last.taps_dropped.load(Ordering::Relaxed), 0);
    bench.stop().await;
}

/// A tap that panics (a capture host's bug) is dropped; the device's link goes on.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_panicking_tap_leaves_the_link_going() {
    let (bench, host) = recording("cap-panic");
    let a = bench.program("alice").await;
    assert_eq!(a.dev.add(1, 2).call().await.unwrap(), 3);
    bench.start_capture(&a, options(true, true)).await;
    let cap = host.only();
    host.until("session", || cap.count(TapStream::Session) == 1)
        .await;
    cap.panic_messages.store(true, Ordering::Relaxed);
    assert_eq!(a.dev.add(2, 3).call().await.unwrap(), 5);
    host.until("tap dropped", || {
        cap.taps_dropped.load(Ordering::Relaxed) == cap.links.load(Ordering::Relaxed)
    })
    .await;
    assert_eq!(a.dev.add(3, 4).call().await.unwrap(), 7);
    assert_eq!(entries(&a).await.len(), 1);
    bench.stop().await;
}
