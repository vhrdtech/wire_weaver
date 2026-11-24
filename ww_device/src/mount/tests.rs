//! A module slot driven by hand with stand-in modules: a well-behaved one, one that misbehaves in every way the slot
//! checks, one that fails.

extern crate std;
use std::vec;
use std::vec::Vec;

use shrink_wrap::prelude::{DeserializeShrinkWrap, SerializeShrinkWrap};
use shrink_wrap::tail_bytes::TailBytes;
use shrink_wrap::{RefVec, UNib32, UVlq32, UVlq32Backfill};
use wire_weaver::ww_version::{FullVersion, Version, WireVersion};
use wire_weaver::{BlockingEventOut, Context, Priority, SendError};
use ww_client_server::mount::{MountAnswer, MountRequest};
use ww_client_server::{
    ErrorKind, Event, EventKind, PathKind, Request, RequestKind, ResourceIndex, StreamSideband,
    err_seq, resource_path,
};

use super::*;

/// The slot's `api` on the front: `mount!(hub)` at id 3.
const API: [ResourceIndex; 2] = resource_path([3, 1]);
const MSG: usize = 128;
const OUT: usize = 256;

/// A model of the crate `mod_api` 0.2.1 at wire w1.4, with nothing in it.
fn model() -> Vec<u8> {
    model_of(1)
}

/// [model] of the version 0.2.`patch`.
fn model_of(patch: u32) -> Vec<u8> {
    model_named(FullVersion::new("mod_api", Version::new(0, 2, patch)))
}

/// [model] of the crate `api`.
fn model_named(api: FullVersion<'_>) -> Vec<u8> {
    let crates = [api];
    let bundle = ww_self::ApiBundle {
        magic: ww_self::MAGIC,
        ww_self_version: ww_self::VERSION,
        root: ww_self::ApiLevel {
            docs: RefVec::Slice { slice: &[] },
            crate_idx: UNib32(0),
            trait_name: "Mod",
            items: RefVec::Slice { slice: &[] },
            compact_id: None,
            meta: None,
        },
        types: RefVec::Slice { slice: &[] },
        traits: RefVec::Slice { slice: &[] },
        ext_crates: RefVec::Slice { slice: &crates },
        root_omitted: None,
        wire: Some(WireVersion::new(1, 4)),
        ext_wire: None,
    };
    let mut buf = [0u8; 256];
    bundle.to_ww_bytes(&mut buf).unwrap().to_vec()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Behave {
    /// A value under the request's seq with the path the module saw, after a stream item at `[2]`.
    Echo,
    /// No reply now: the module answers later, from `poll`.
    Defer,
    /// Everything the slot checks, then a good reply.
    Misbehave,
    /// A reply outside of the buffer the module was given.
    ForeignReply,
    /// A failure.
    Trap,
    /// More events than the outbox holds.
    Flood,
}

#[derive(Debug)]
struct Fake {
    model: Vec<u8>,
    behave: Behave,
    /// Requests as the module got them: their seq and path.
    seen: Vec<(u32, Vec<u32>)>,
    /// Events the module sends at its next poll.
    later: Vec<Vec<u8>>,
}

impl Fake {
    fn new(behave: Behave) -> Self {
        Fake {
            model: model(),
            behave,
            seen: vec![],
            later: vec![],
        }
    }
}

fn event(seq: u32, kind: EventKind<'_>) -> Vec<u8> {
    let mut buf = [0u8; 256];
    Event {
        seq: UVlq32(seq),
        result: Ok(kind),
    }
    .to_ww_bytes(&mut buf)
    .unwrap()
    .to_vec()
}

fn value(seq: u32, data: &[u8]) -> Vec<u8> {
    event(
        seq,
        EventKind::Value {
            data: TailBytes(data),
        },
    )
}

fn stream_data(path: &[ResourceIndex]) -> Vec<u8> {
    event(
        0,
        EventKind::StreamData {
            path: RefVec::Slice { slice: path },
            data: TailBytes(&[7]),
        },
    )
}

impl Module for Fake {
    fn model(&self) -> &[u8] {
        &self.model
    }

    fn process<'r>(
        &mut self,
        request: &[u8],
        events: &mut Outbox<'_>,
        reply: &'r mut [u8],
    ) -> Result<&'r [u8], ModuleError> {
        let request = Request::from_ww_bytes(request).unwrap();
        let PathKind::Absolute { path } = &request.path_kind else {
            panic!("the slot sends absolute paths");
        };
        let path: Vec<u32> = path.iter().map(|i| i.unwrap().get().0).collect();
        let seq = request.seq.0;
        self.seen.push((seq, path.clone()));
        let path_bytes: Vec<u8> = path.iter().map(|i| *i as u8).collect();
        let answer = value(seq, &path_bytes);
        match self.behave {
            Behave::Echo => {
                events.push(&stream_data(&resource_path([2]))).unwrap();
            }
            Behave::Defer => {
                self.later.push(answer);
                return Ok(&[]);
            }
            Behave::Misbehave => {
                let long: Vec<ResourceIndex> = (0..PATH_IDS_MAX as u32)
                    .map(|i| ResourceIndex::new(UNib32(i)))
                    .collect();
                for bad in [
                    vec![0xff, 0xff, 0xff],
                    value(0, &[1]),
                    value(seq + 1, &[2]),
                    stream_data(&long),
                ] {
                    events.push(&bad).unwrap();
                }
            }
            Behave::ForeignReply => {
                static FOREIGN: [u8; 3] = [1, 2, 3];
                return Ok(&FOREIGN);
            }
            Behave::Trap => return Err(ModuleError::Fault),
            Behave::Flood => {
                let item = stream_data(&resource_path([2]));
                while events.push(&item).is_ok() {}
            }
        }
        let to = reply
            .get_mut(..answer.len())
            .ok_or(ModuleError::ReplyTooLong)?;
        to.copy_from_slice(&answer);
        Ok(to)
    }

    fn poll(&mut self, events: &mut Outbox<'_>) -> Result<(), ModuleError> {
        for message in self.later.drain(..) {
            events.push(&message).unwrap();
        }
        Ok(())
    }
}

/// What the parent sends: each message and whether it went as a reply.
#[derive(Default)]
struct Collect(Vec<(Vec<u8>, bool)>);

impl BlockingEventOut for Collect {
    fn send_with_blocking<F>(&mut self, ser: F) -> Result<(), SendError>
    where
        F: FnOnce(&mut [u8]) -> Result<usize, ShrinkWrapError>,
    {
        let mut scratch = [0u8; 256];
        let len = ser(&mut scratch)?;
        self.0.push((scratch[..len].to_vec(), false));
        Ok(())
    }

    fn send_reply_bytes_blocking(
        &mut self,
        _priority: Priority,
        message: &[u8],
    ) -> Result<(), SendError> {
        self.0.push((message.to_vec(), true));
        Ok(())
    }
}

/// A message as the client reads it.
#[derive(Debug, PartialEq, Eq)]
enum Seen {
    Value(u32, Vec<u8>),
    Data(Vec<u32>),
    Close(Vec<u32>),
    Error(u32, u32, &'static str),
}

fn seen(out: &Collect) -> Vec<Seen> {
    out.0
        .iter()
        .map(|(bytes, _)| {
            let event = Event::from_ww_bytes(bytes).unwrap();
            let ids = |path: &RefVec<'_, ResourceIndex>| {
                path.iter()
                    .map(|i| i.unwrap().get().0)
                    .collect::<Vec<u32>>()
            };
            match event.result {
                Ok(EventKind::Value { data }) => Seen::Value(event.seq.0, data.0.to_vec()),
                Ok(EventKind::StreamData { path, .. }) => Seen::Data(ids(&path)),
                Ok(EventKind::StreamSideband {
                    path,
                    sideband: StreamSideband::Close,
                }) => Seen::Close(ids(&path)),
                Err(e) => Seen::Error(
                    event.seq.0,
                    e.err_seq.0,
                    match e.kind {
                        ErrorKind::Unavailable => "unavailable",
                        ErrorKind::Overloaded => "overloaded",
                        ErrorKind::ResponseSerFailed => "too long",
                        _ => "other",
                    },
                ),
                other => panic!("unexpected {other:?}"),
            }
        })
        .collect()
}

type Slot = ModuleSlot<Fake, MSG, OUT>;

/// Send a request of `kind` with `seq` at `below` under the slot.
fn send(
    slot: &mut Slot,
    out: &mut Collect,
    seq: u32,
    below: &[u32],
    kind: RequestKind<'_>,
) -> MountAnswer {
    let mut path: Vec<ResourceIndex> = API.to_vec();
    path.extend(below.iter().map(|i| ResourceIndex::new(UNib32(*i))));
    let request = Request {
        seq: UVlq32Backfill(seq),
        priority: Priority::NORMAL.to_wire(),
        context: Default::default(),
        path_kind: PathKind::absolute(&path),
        kind,
    };
    let mount = MountRequest {
        request: &request,
        path: &path[API.len()..],
    };
    let mut cx = Context::new(seq, (), out);
    slot.forward_blocking(&mut cx, &mount)
}

fn call(slot: &mut Slot, out: &mut Collect, seq: u32, below: &[u32]) -> MountAnswer {
    send(
        slot,
        out,
        seq,
        below,
        RequestKind::Call {
            args: TailBytes(&[]),
        },
    )
}

fn loaded(behave: Behave) -> Slot {
    let mut slot = Slot::new();
    assert!(slot.load(Fake::new(behave)).unwrap().is_none());
    slot
}

#[test]
fn empty_slot_is_unavailable_and_absent() {
    let mut slot = Slot::new();
    let mut out = Collect::default();
    assert_eq!(call(&mut slot, &mut out, 5, &[0]), MountAnswer::Unavailable);
    assert!(out.0.is_empty());
    assert!(matches!(slot.presence(), MountPresence::Absent));
    assert!(!slot.take_changed());
}

/// The presence names the model's crate, version and wire, and the hash the slot computed of the model, never one
/// the module claims.
#[test]
fn presence_comes_from_the_model() {
    let mut slot = loaded(Behave::Echo);
    assert!(slot.take_changed());
    assert!(!slot.take_changed());
    let MountPresence::Up { api, hash, wire } = slot.presence() else {
        panic!("up");
    };
    assert_eq!(api, FullVersion::new("mod_api", Version::new(0, 2, 1)));
    assert_eq!(wire, Some(WireVersion::new(1, 4)));
    let hash: Vec<u8> = hash.no_docs.hash.iter().map(Result::unwrap).collect();
    assert_eq!(hash, api_hash(&model()));

    let mut bad = Fake::new(Behave::Echo);
    bad.model = vec![1, 2, 3];
    let (error, _) = slot.load(bad).unwrap_err();
    assert_eq!(error, LoadError::Model);
    assert!(slot.is_loaded(), "a refused module leaves the loaded one");
    assert!(!slot.take_changed());
}

/// The module sees the path below the slot; its events come back under the slot's path, its reply as is.
#[test]
fn requests_go_in_without_the_prefix_and_events_come_out_with_it() {
    let mut slot = loaded(Behave::Echo);
    let mut out = Collect::default();
    assert_eq!(
        call(&mut slot, &mut out, 5, &[4, 0]),
        MountAnswer::Forwarded
    );
    assert_eq!(slot.module().unwrap().seen, [(5, vec![4, 0])]);
    assert_eq!(
        seen(&out),
        [Seen::Data(vec![3, 1, 2]), Seen::Value(5, vec![4, 0])]
    );
    assert!(out.0[1].1, "the reply goes as a reply");
    assert_eq!(slot.counters(), ModuleCounters::default());
}

/// Garbage, a value without a seq, a reply under a seq the module wasn't sent, a path too long: dropped and counted,
/// the good reply passes, and the parent never panics.
#[test]
fn a_misbehaving_module_is_checked() {
    let mut slot = loaded(Behave::Misbehave);
    let mut out = Collect::default();
    call(&mut slot, &mut out, 9, &[]);
    assert_eq!(seen(&out), [Seen::Value(9, vec![])]);
    let counters = slot.counters();
    assert_eq!((counters.malformed, counters.stray), (2, 2));
    assert!(slot.is_loaded());
}

/// A reply outside of the buffer the module was given is a broken module: unloaded, its request answered.
#[test]
fn a_foreign_reply_unloads_the_module() {
    let mut slot = loaded(Behave::ForeignReply);
    slot.take_changed();
    let mut out = Collect::default();
    call(&mut slot, &mut out, 4, &[]);
    assert_eq!(
        seen(&out),
        [Seen::Error(4, err_seq::MOUNT_BACKEND_GONE, "unavailable")]
    );
    assert!(!slot.is_loaded());
    assert!(slot.take_changed());
    assert_eq!(slot.counters().faults, 1);
}

/// A module that fails is unloaded: its open streams close, the requests it held are answered `Unavailable`, the
/// slot turns `Absent`.
#[test]
fn a_failing_module_is_unloaded() {
    let mut slot = loaded(Behave::Defer);
    let mut out = Collect::default();
    send(
        &mut slot,
        &mut out,
        6,
        &[2],
        RequestKind::StreamSideband {
            sideband: StreamSideband::Open,
        },
    );
    call(&mut slot, &mut out, 7, &[0]);
    assert!(out.0.is_empty(), "deferred");
    slot.module().unwrap().behave = Behave::Trap;
    slot.module().unwrap().later.clear();
    slot.take_changed();
    assert_eq!(call(&mut slot, &mut out, 8, &[0]), MountAnswer::Forwarded);
    let mut answers = seen(&out);
    answers.sort_by_key(|s| std::format!("{s:?}"));
    assert_eq!(
        answers,
        [
            Seen::Close(vec![3, 1, 2]),
            Seen::Error(6, err_seq::MOUNT_BACKEND_GONE, "unavailable"),
            Seen::Error(7, err_seq::MOUNT_BACKEND_GONE, "unavailable"),
            Seen::Error(8, err_seq::MOUNT_BACKEND_GONE, "unavailable"),
        ]
    );
    assert!(matches!(slot.presence(), MountPresence::Absent));
    assert!(slot.take_changed());
    assert_eq!(slot.counters().faults, 1);
}

/// A deferred reply passes once, from the module's poll; a second one under the same seq is stray.
#[test]
fn deferred_replies_come_from_poll() {
    let mut slot = loaded(Behave::Defer);
    let mut out = Collect::default();
    call(&mut slot, &mut out, 11, &[1]);
    assert!(out.0.is_empty());
    let answer = slot.module().unwrap().later[0].clone();
    slot.module().unwrap().later.push(answer);
    slot.poll_blocking(&mut out);
    assert_eq!(seen(&out), [Seen::Value(11, vec![1])]);
    assert_eq!(slot.counters().stray, 1);
}

/// Past `IN_FLIGHT_MAX` unanswered requests the slot refuses before the module sees one.
#[test]
fn too_many_in_flight_are_refused() {
    let mut slot = loaded(Behave::Defer);
    let mut out = Collect::default();
    for seq in 1..=IN_FLIGHT_MAX as u32 {
        call(&mut slot, &mut out, seq, &[]);
    }
    call(&mut slot, &mut out, 99, &[]);
    assert_eq!(
        seen(&out),
        [Seen::Error(99, err_seq::MOUNT_QUEUE_FULL, "overloaded")]
    );
    assert_eq!(slot.module().unwrap().seen.len(), IN_FLIGHT_MAX);
    assert_eq!(slot.counters().refused, 1);
}

/// Events past the outbox are dropped and counted; the reply still passes.
#[test]
fn a_flood_is_cut_at_the_outbox() {
    let mut slot = loaded(Behave::Flood);
    let mut out = Collect::default();
    call(&mut slot, &mut out, 3, &[]);
    let all = seen(&out);
    assert_eq!(all.last(), Some(&Seen::Value(3, vec![])));
    assert!(all.len() > 2);
    assert_eq!(slot.counters().dropped, 1);
}

/// A reload takes the old module's requests and streams with it, and names the new model's hash.
#[test]
fn a_reload_replaces_the_module() {
    let mut slot = loaded(Behave::Defer);
    let mut out = Collect::default();
    call(&mut slot, &mut out, 2, &[]);
    slot.take_changed();
    let mut next = Fake::new(Behave::Echo);
    next.model = model_of(2);
    let old = slot.load(next).unwrap().expect("the old module");
    assert_eq!(old.seen.len(), 1);
    assert!(slot.take_changed());
    let MountPresence::Up { hash, .. } = slot.presence() else {
        panic!("up");
    };
    let hash: Vec<u8> = hash.no_docs.hash.iter().map(Result::unwrap).collect();
    assert_ne!(hash, api_hash(&model()));
    slot.poll_blocking(&mut out);
    assert_eq!(
        seen(&out),
        [Seen::Error(2, err_seq::MOUNT_BACKEND_GONE, "unavailable")]
    );
}

/// A slot built in place over memory that held something else serves as one made by `new`.
#[test]
fn a_slot_built_in_place_serves() {
    let mut memory = std::boxed::Box::new(core::mem::MaybeUninit::<Slot>::uninit());
    // SAFETY: bytes written into a `MaybeUninit`, never read as a `Slot` before `init`
    unsafe {
        memory
            .as_mut_ptr()
            .cast::<u8>()
            .write_bytes(0xAA, size_of::<Slot>())
    };
    let slot = Slot::init(&mut memory);
    assert!(!slot.is_loaded());
    assert!(!slot.take_changed());
    assert_eq!(slot.counters(), ModuleCounters::default());
    assert!(slot.load(Fake::new(Behave::Echo)).unwrap().is_none());
    let mut out = Collect::default();
    assert_eq!(call(slot, &mut out, 5, &[4, 0]), MountAnswer::Forwarded);
    assert_eq!(
        seen(&out),
        [Seen::Data(vec![3, 1, 2]), Seen::Value(5, vec![4, 0])]
    );
}

/// The presence names what the model said at load, with its pre-release and build metadata: a module that changes
/// its model later changes nothing, and names past the slot's room are refused.
#[test]
fn presence_is_read_once_at_load() {
    let api = FullVersion::new(
        "mod_api",
        Version::full(0, 2, 1, Some("alpha.1"), Some("zstd.1.5")),
    );
    let mut fake = Fake::new(Behave::Echo);
    fake.model = model_named(api);
    let mut slot = Slot::new();
    assert!(slot.load(fake).unwrap().is_none());
    slot.module().unwrap().model = vec![1, 2, 3];
    let MountPresence::Up {
        api: named, wire, ..
    } = slot.presence()
    else {
        panic!("up");
    };
    assert_eq!(named, api);
    assert_eq!(wire, Some(WireVersion::new(1, 4)));

    let long = "x".repeat(IDENTITY_TEXT_MAX + 1);
    let mut fake = Fake::new(Behave::Echo);
    fake.model = model_named(FullVersion::new(&long, Version::new(0, 1, 0)));
    let (error, _) = slot.load(fake).unwrap_err();
    assert_eq!(error, LoadError::NameTooLong);
    assert!(slot.is_loaded(), "a refused module leaves the loaded one");
}
