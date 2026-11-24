//! A mount slot served in process (SRV-81): code in the firmware's own process, a module, serves the API mounted at
//! a slot (SRV-76), with no link in between. The module is a WireWeaver server reached through bytes only, so a
//! wasm interpreter's host calls implement [Module] as well as a statically linked server ([Linked]) does.
//!
//! [ModuleSlot] holds one module for one front connection: the slot handler hands it each request under the slot's
//! `api` ([ModuleSlot::forward]), and the slot's backend gives it its turns ([ModuleSlot::poll]). It takes the slot's path off each request, calls the module, and sends the module's
//! replies, events and stream items to the client through the handler's [EventOut] with the slot's path in front.
//!
//! A loaded module is untrusted: everything it says is checked before it reaches the parent's [EventOut]. Its model
//! must decode as a `ww_self::ApiBundle`, and the slot hashes it itself: the module's word on its hash is never
//! taken. Each event must decode as an `Event`; a
//! reply or a request's stream data passes only under a seq the module was sent and hasn't answered; an event with
//! a path gets the slot's path in front, so it can't reach the parent's own tree; a joined path past
//! [PATH_IDS_MAX] ids or a message past the slot's buffers is dropped. A module that fails ([ModuleError::Fault]: a
//! wasm trap) is unloaded. Nothing a module sends makes the slot panic.
//!
//! Everything lives in the slot, which is set up in place in a `static` ([ModuleSlot::init]): no heap.

use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use shrink_wrap::prelude::{DeserializeShrinkWrap, ShrinkWrapError};
use shrink_wrap::{BufReader, RefVec};
use wire_weaver::ww_api::mount::MountPresence;
use wire_weaver::ww_version::{ApiHash, ApiHashPair, FullVersion, Version, WireVersion};
use wire_weaver::{BlockingEventOut, Context, EventOut, Priority, WireWeaverApiBackend};
use ww_client_server::mount::{
    MountAnswer, MountPrefix, MountRequest, PathArray, PathStore, RewriteError, request_at,
};
use ww_client_server::{Error, ErrorKind, RequestKind, ResourceIndex, StreamSideband, err_seq};
use ww_self::hash::{SIGNATURE_LEN, api_hash};

use crate::sub_range;

mod global_paths;
pub use global_paths::ModuleTraits;

/// Ids in the front's path of anything under a slot: the slot's `api` and the path below it.
pub const PATH_IDS_MAX: usize = 16;
/// Requests sent to a module and not answered yet; more are refused `Overloaded`.
pub const IN_FLIGHT_MAX: usize = 8;
/// Streams opened and trait levels observed under a slot, closed when the module goes.
pub const CLOSABLE_MAX: usize = 8;
/// Answers of the slot's own (refusals, requests a gone module left) waiting to be sent.
const ANSWERS_MAX: usize = 2 * IN_FLIGHT_MAX + 2;
/// Bytes of the length in front of each message of an [Outbox].
const OUTBOX_LEN_BYTES: usize = 2;
/// Bytes of a module's crate name, pre-release and build metadata together, kept by the slot for its `presence`.
pub const IDENTITY_TEXT_MAX: usize = 64;
const _: () = assert!(
    IDENTITY_TEXT_MAX <= u8::MAX as usize,
    "a part's length is a u8"
);

/// A WireWeaver server in the firmware's process, served at a mount slot through a [ModuleSlot]: requests go in as
/// bytes, events and replies come out as bytes. A wasm interpreter implements it from its host calls, [Linked] for a
/// server linked into the firmware.
pub trait Module {
    /// The module's model: its server's introspection data without docs and uncompressed, a `ww_self::ApiBundle`
    /// in shrink_wrap bytes with dependency types left out (the generated `WW_SELF_BYTES_NO_DOCS`). The slot reads
    /// the crate, version and wire version a slot's `presence` names from it, and hashes it for the presence's hash:
    /// the server's own `API_HASH_NO_DOCS`, so a generated client of the module's API takes it as known. The same
    /// bytes while the module is loaded.
    fn model(&self) -> &[u8];

    /// Serve `request`, a request at the module's own absolute path. Events go to `events`, the reply is written into
    /// `reply` and returned (empty when there is none yet, or for a request without a seq).
    ///
    /// # Errors
    ///
    /// [ModuleError::Fault] when the module failed (the slot unloads it), [ModuleError::ReplyTooLong] when the
    /// reply doesn't fit `reply`.
    fn process<'r>(
        &mut self,
        request: &[u8],
        events: &mut Outbox<'_>,
        reply: &'r mut [u8],
    ) -> Result<&'r [u8], ModuleError>;

    /// The module's turn outside of a request: stream items, observed property updates and deferred replies go to
    /// `events`. Nothing by default.
    ///
    /// # Errors
    ///
    /// [ModuleError::Fault] when the module failed: the slot unloads it.
    fn poll(&mut self, events: &mut Outbox<'_>) -> Result<(), ModuleError> {
        _ = events;
        Ok(())
    }
}

/// Why a [Module] couldn't serve a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ModuleError {
    /// The module failed (a wasm trap, out of fuel): it is unloaded.
    Fault,
    /// The reply doesn't fit the slot's buffer: the request is answered `ResponseSerFailed`.
    ReplyTooLong,
}

/// A server generated with `use_async = false` and linked into the firmware, as a [Module].
pub struct Linked<S> {
    /// The generated server.
    pub server: S,
    /// Its model: the generated `WW_SELF_BYTES_NO_DOCS`, see [Module::model].
    pub model: &'static [u8],
}

impl<S: WireWeaverApiBackend<Medium = ()>> Module for Linked<S> {
    fn model(&self) -> &[u8] {
        self.model
    }

    fn process<'r>(
        &mut self,
        request: &[u8],
        events: &mut Outbox<'_>,
        reply: &'r mut [u8],
    ) -> Result<&'r [u8], ModuleError> {
        self.server
            .process_bytes(events, (), request, reply)
            .map_err(|_| ModuleError::ReplyTooLong)
    }
}

/// Where a [Module] sends its events: messages one after another in the slot's buffer, each behind its length.
/// A message that doesn't fit is dropped and counted ([ModuleCounters::dropped]).
pub struct Outbox<'b> {
    buf: &'b mut [u8],
    len: usize,
    dropped: u32,
}

impl<'b> Outbox<'b> {
    fn new(buf: &'b mut [u8]) -> Self {
        Outbox {
            buf,
            len: 0,
            dropped: 0,
        }
    }

    /// Queue `message`, an encoded `Event`: what a wasm interpreter's host call does with the bytes the module
    /// hands it.
    ///
    /// # Errors
    ///
    /// [OutboxFull] when it doesn't fit; it is counted as dropped.
    pub fn push(&mut self, message: &[u8]) -> Result<(), OutboxFull> {
        self.send_with_blocking(|free| {
            let to = free
                .get_mut(..message.len())
                .ok_or(ShrinkWrapError::OutOfBoundsWriteRawSlice)?;
            to.copy_from_slice(message);
            Ok(message.len())
        })
        .map_err(|_| OutboxFull)
    }
}

/// An [Outbox] has no room for a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct OutboxFull;

impl BlockingEventOut for Outbox<'_> {
    fn send_with_blocking<F>(&mut self, ser: F) -> Result<(), wire_weaver::SendError>
    where
        F: FnOnce(&mut [u8]) -> Result<usize, ShrinkWrapError>,
    {
        let free = self.buf.get_mut(self.len..).unwrap_or_default();
        let Some((len, free)) = free.split_first_chunk_mut::<OUTBOX_LEN_BYTES>() else {
            self.dropped = self.dropped.saturating_add(1);
            return Err(wire_weaver::SendError::Serialize(
                ShrinkWrapError::OutOfBoundsWriteRawSlice,
            ));
        };
        let room = free.len().min(usize::from(u16::MAX));
        let message_len = match ser(free) {
            // a length past the room the writer had is a broken writer: the message is dropped, never read past
            Ok(message_len) if message_len <= room => message_len,
            Ok(_) | Err(_) => {
                self.dropped = self.dropped.saturating_add(1);
                return Err(wire_weaver::SendError::Serialize(
                    ShrinkWrapError::OutOfBoundsWriteRawSlice,
                ));
            }
        };
        *len = u16::try_from(message_len)
            .expect("bounded by room above")
            .to_le_bytes();
        self.len += OUTBOX_LEN_BYTES + message_len;
        Ok(())
    }
}

/// Why [ModuleSlot::load] refused a module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum LoadError {
    /// The model doesn't decode as a `ww_self::ApiBundle`, or names no crate for its root.
    Model,
    /// The crate name, pre-release and build metadata of the model are past [IDENTITY_TEXT_MAX] bytes together.
    NameTooLong,
}

/// What a [ModuleSlot] dropped or refused: a module's misbehavior shows here, never as a panic.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct ModuleCounters {
    /// Events of the module that don't decode, or don't fit the slot's buffer with the slot's path in front.
    pub malformed: u32,
    /// Replies and request data under a seq the module wasn't sent or already answered, and replies without a seq.
    pub stray: u32,
    /// Events that didn't fit the outbox.
    pub dropped: u32,
    /// Messages the parent's [EventOut] refused.
    pub unsent: u32,
    /// Requests refused before the module saw them: too many in flight, too long.
    pub refused: u32,
    /// Answers of the slot's own that didn't fit their queue: such a request is never answered.
    pub lost_answers: u32,
    /// Times the module failed and was unloaded.
    pub faults: u32,
}

/// An answer of the slot's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Refusal {
    /// The module is gone, or never was.
    Gone,
    /// Too many requests in flight.
    Overloaded,
    /// The request, the reply or the rewritten message is too long.
    TooLong,
}

impl Refusal {
    fn error(self) -> Error<'static> {
        match self {
            Refusal::Gone => Error::new(err_seq::MOUNT_BACKEND_GONE, ErrorKind::Unavailable),
            Refusal::Overloaded => Error::new(err_seq::MOUNT_QUEUE_FULL, ErrorKind::Overloaded),
            Refusal::TooLong => Error::new(err_seq::MOUNT_TOO_LONG, ErrorKind::ResponseSerFailed),
        }
    }
}

/// A message the slot sends next, see [ModuleSlot::next_out].
struct Outgoing<'a> {
    bytes: &'a [u8],
    /// A reply, or data of a request: its priority, seq and whether it ends the request.
    reply: Option<Answering>,
}

/// What a reply, or data of a request, answers.
#[derive(Debug, Clone, Copy)]
struct Answering {
    priority: Priority,
    seq: u32,
    /// It ends the request: answered `ResponseSerFailed` when the parent refuses it.
    ends: bool,
}

/// A [Module] at one mount slot, for one front connection, with every buffer it needs: set it up in place in a
/// `static` with [ModuleSlot::init]. `MSG` is the longest message in and out (the front link's message limit), `OUT`
/// the bytes of the module's events per turn.
///
/// ```ignore
/// async fn hub_request(&mut self, cx: &mut Context<'_, impl EventOut>, request: MountRequest<'_, '_>) -> MountAnswer {
///     self.hub.forward(cx, &request).await
/// }
/// // the backend's poll
/// self.hub.poll(out).await;
/// if self.hub.take_changed() { self.mark_changed_hub_presence(); }
/// ```
pub struct ModuleSlot<M, const MSG: usize, const OUT: usize> {
    module: Option<M>,
    /// The hash of the loaded module's model.
    hash: [u8; SIGNATURE_LEN],
    /// The loaded module's API, read from its model at load: the model is never decoded again.
    identity: Identity,
    /// The front's path of the slot's `api`, from the requests under it.
    prefix: Option<MountPrefix<PathArray<PATH_IDS_MAX>>>,
    in_flight: [(u32, Priority); IN_FLIGHT_MAX],
    in_flight_len: usize,
    /// Front paths of the streams open and the trait levels observed under the slot.
    closable: [PathArray<PATH_IDS_MAX>; CLOSABLE_MAX],
    closable_len: usize,
    /// Front paths a gone module left open: each is sent a `Close` sideband.
    closing: [PathArray<PATH_IDS_MAX>; CLOSABLE_MAX],
    closing_len: usize,
    answers: [(u32, Priority, Refusal); ANSWERS_MAX],
    answers_len: usize,
    request_buf: [u8; MSG],
    reply_buf: [u8; MSG],
    /// The reply of the last request: where it starts in `reply_buf`, its length.
    reply: (usize, usize),
    /// The seq of the request `reply` answers.
    reply_seq: u32,
    /// The module's message being checked.
    current: (usize, usize),
    rewrite_buf: [u8; MSG],
    out_buf: [u8; OUT],
    out_len: usize,
    out_read: usize,
    changed: bool,
    counters: ModuleCounters,
}

impl<M: Module, const MSG: usize, const OUT: usize> Default for ModuleSlot<M, MSG, OUT> {
    fn default() -> Self {
        Self::new()
    }
}

impl<M: Module, const MSG: usize, const OUT: usize> ModuleSlot<M, MSG, OUT> {
    /// An empty slot, built in place: for a slot in a `static` of `MaybeUninit` (`static_cell::StaticCell::uninit`),
    /// nothing of it on the stack and nothing of it in flash. A `static` of [new](Self::new) is placed in `.data`,
    /// its whole size in flash too: the padding of its fields keeps it out of `.bss`.
    pub fn init(slot: &mut MaybeUninit<Self>) -> &mut Self {
        // every field is written below: a new one fails to compile here until it is
        let _ = |s: Self| {
            let ModuleSlot {
                module: _,
                hash: _,
                identity: _,
                prefix: _,
                in_flight: _,
                in_flight_len: _,
                closable: _,
                closable_len: _,
                closing: _,
                closing_len: _,
                answers: _,
                answers_len: _,
                request_buf: _,
                reply_buf: _,
                reply: _,
                reply_seq: _,
                current: _,
                rewrite_buf: _,
                out_buf: _,
                out_len: _,
                out_read: _,
                changed: _,
                counters: _,
            } = s;
        };
        let at = slot.as_mut_ptr();
        // SAFETY: every field written in place, each through its own pointer; an array of `N` items is `N` items one
        // after the other, and zero bytes are a valid `u8`
        unsafe {
            addr_of_mut!((*at).module).write(None);
            addr_of_mut!((*at).hash)
                .cast::<u8>()
                .write_bytes(0, SIGNATURE_LEN);
            addr_of_mut!((*at).identity).write(Identity::EMPTY);
            addr_of_mut!((*at).prefix).write(None);
            let in_flight = addr_of_mut!((*at).in_flight).cast::<(u32, Priority)>();
            for i in 0..IN_FLIGHT_MAX {
                in_flight.add(i).write((0, Priority::NORMAL));
            }
            addr_of_mut!((*at).in_flight_len).write(0);
            let closable = addr_of_mut!((*at).closable).cast::<PathArray<PATH_IDS_MAX>>();
            let closing = addr_of_mut!((*at).closing).cast::<PathArray<PATH_IDS_MAX>>();
            for i in 0..CLOSABLE_MAX {
                closable.add(i).write(PathArray::new());
                closing.add(i).write(PathArray::new());
            }
            addr_of_mut!((*at).closable_len).write(0);
            addr_of_mut!((*at).closing_len).write(0);
            let answers = addr_of_mut!((*at).answers).cast::<(u32, Priority, Refusal)>();
            for i in 0..ANSWERS_MAX {
                answers.add(i).write((0, Priority::NORMAL, Refusal::Gone));
            }
            addr_of_mut!((*at).answers_len).write(0);
            addr_of_mut!((*at).request_buf)
                .cast::<u8>()
                .write_bytes(0, MSG);
            addr_of_mut!((*at).reply_buf)
                .cast::<u8>()
                .write_bytes(0, MSG);
            addr_of_mut!((*at).reply).write((0, 0));
            addr_of_mut!((*at).reply_seq).write(0);
            addr_of_mut!((*at).current).write((0, 0));
            addr_of_mut!((*at).rewrite_buf)
                .cast::<u8>()
                .write_bytes(0, MSG);
            addr_of_mut!((*at).out_buf).cast::<u8>().write_bytes(0, OUT);
            addr_of_mut!((*at).out_len).write(0);
            addr_of_mut!((*at).out_read).write(0);
            addr_of_mut!((*at).changed).write(false);
            addr_of_mut!((*at).counters).write(ModuleCounters::default());
            slot.assume_init_mut()
        }
    }

    /// An empty slot.
    pub const fn new() -> Self {
        ModuleSlot {
            module: None,
            hash: [0; SIGNATURE_LEN],
            identity: Identity::EMPTY,
            prefix: None,
            in_flight: [(0, Priority::NORMAL); IN_FLIGHT_MAX],
            in_flight_len: 0,
            closable: [const { PathArray::new() }; CLOSABLE_MAX],
            closable_len: 0,
            closing: [const { PathArray::new() }; CLOSABLE_MAX],
            closing_len: 0,
            answers: [(0, Priority::NORMAL, Refusal::Gone); ANSWERS_MAX],
            answers_len: 0,
            request_buf: [0; MSG],
            reply_buf: [0; MSG],
            reply: (0, 0),
            reply_seq: 0,
            current: (0, 0),
            rewrite_buf: [0; MSG],
            out_buf: [0; OUT],
            out_len: 0,
            out_read: 0,
            changed: false,
            counters: ModuleCounters {
                malformed: 0,
                stray: 0,
                dropped: 0,
                unsent: 0,
                refused: 0,
                lost_answers: 0,
                faults: 0,
            },
        }
    }

    /// Load `module`, in place of the one loaded before, which is returned: its requests are answered `Unavailable`
    /// and its streams and observed levels closed, as by [unload](Self::unload). The slot's `presence` turns `Up`
    /// with the module's API and the hash of its model, so a client relearns this slot's API when it differs.
    ///
    /// # Errors
    ///
    /// [LoadError::Model] when the module's model doesn't decode, [LoadError::NameTooLong] when its names don't fit
    /// the slot; `module` comes back and the slot stays as it was.
    pub fn load(&mut self, module: M) -> Result<Option<M>, (LoadError, M)> {
        let identity = match Identity::of(module.model()) {
            Ok(identity) => identity,
            Err(e) => return Err((e, module)),
        };
        let hash = api_hash(module.model());
        let old = self.unload();
        self.hash = hash;
        self.identity = identity;
        self.module = Some(module);
        self.changed = true;
        Ok(old)
    }

    /// Unload the module and return it: each request it didn't answer is answered `Unavailable`, each stream opened
    /// and each trait level observed under the slot gets a `Close` sideband, at the next
    /// [forward](Self::forward) or [poll](Self::poll). The slot's `presence` turns `Absent`.
    pub fn unload(&mut self) -> Option<M> {
        let module = self.module.take()?;
        self.changed = true;
        for i in 0..self.in_flight_len {
            let (seq, priority) = self.in_flight[i];
            self.answer(seq, priority, Refusal::Gone);
        }
        self.in_flight_len = 0;
        for i in 0..self.closable_len {
            match self.closing.get_mut(self.closing_len) {
                Some(slot) => {
                    slot.clone_from(&self.closable[i]);
                    self.closing_len += 1;
                }
                None => self.counters.lost_answers = self.counters.lost_answers.saturating_add(1),
            }
        }
        self.closable_len = 0;
        // what the module sent and the slot didn't pass on yet is gone with it
        self.out_len = 0;
        self.out_read = 0;
        self.reply = (0, 0);
        Some(module)
    }

    /// Whether a module is loaded.
    pub fn is_loaded(&self) -> bool {
        self.module.is_some()
    }

    /// The loaded module's model, for global trait paths into it ([ModuleTraits]); `None` with nothing loaded.
    pub fn model(&self) -> Option<&[u8]> {
        self.module.as_ref().map(Module::model)
    }

    /// The loaded module.
    pub fn module(&mut self) -> Option<&mut M> {
        self.module.as_mut()
    }

    /// The slot's `presence`, what its `get_<slot>_presence` handler answers: `Up` with the crate, version and wire
    /// version of the module's model and the hash the slot computed of it, `Absent` with nothing loaded.
    pub fn presence(&self) -> MountPresence<'_> {
        if self.module.is_none() {
            return MountPresence::Absent;
        }
        let Some((api, wire)) = self.identity.get() else {
            return MountPresence::Absent;
        };
        MountPresence::Up {
            api,
            hash: ApiHashPair {
                no_docs: ApiHash {
                    hash: RefVec::Slice { slice: &self.hash },
                },
                with_docs: ApiHash {
                    hash: RefVec::Slice { slice: &[] },
                },
            },
            wire,
        }
    }

    /// Whether the slot's `presence` changed since the last call: a module was loaded or unloaded, or failed. The
    /// backend then calls `mark_changed_<slot>_presence()`.
    pub fn take_changed(&mut self) -> bool {
        core::mem::take(&mut self.changed)
    }

    /// What the slot dropped or refused so far.
    pub fn counters(&self) -> ModuleCounters {
        self.counters
    }

    /// Send `request` to the module and the module's answer to the client through `cx`, for the slot's
    /// `<slot>_request` handler of an async server. `Unavailable` with nothing loaded.
    pub async fn forward<O: EventOut, Med: Copy>(
        &mut self,
        cx: &mut Context<'_, O, Med>,
        request: &MountRequest<'_, '_>,
    ) -> MountAnswer {
        let answer = self.start(request);
        self.flush(cx).await;
        answer
    }

    /// [forward](Self::forward) for a server generated with `use_async = false`.
    pub fn forward_blocking<O: BlockingEventOut, Med: Copy>(
        &mut self,
        cx: &mut Context<'_, O, Med>,
        request: &MountRequest<'_, '_>,
    ) -> MountAnswer {
        let answer = self.start(request);
        self.flush_blocking(cx);
        answer
    }

    /// The module's turn outside of a request, and the slot's own answers: for the backend's poll.
    pub async fn poll(&mut self, out: &mut impl EventOut) {
        self.poll_module();
        self.flush(out).await;
    }

    /// [poll](Self::poll) for a blocking event loop.
    pub fn poll_blocking(&mut self, out: &mut impl BlockingEventOut) {
        self.poll_module();
        self.flush_blocking(out);
    }

    fn start(&mut self, request: &MountRequest<'_, '_>) -> MountAnswer {
        if self.module.is_none() {
            return MountAnswer::Unavailable;
        }
        let seq = request.request.seq.0;
        let priority = Priority::from(request.request.priority);
        match request.prefix_in(PathArray::new()) {
            Ok(Some(prefix)) => self.prefix = Some(prefix),
            Ok(None) | Err(_) => return MountAnswer::Unavailable,
        }
        // TODO(TR-24): a module's effect caps are checked here, before it sees the request; every effect passes now
        if seq != 0 && self.in_flight_len >= IN_FLIGHT_MAX {
            self.refuse(seq, priority, Refusal::Overloaded);
            return MountAnswer::Forwarded;
        }
        // a firmware reads no context, and a client's word on who it is never reaches the module
        let mut sent = request.request.clone();
        sent.context = Default::default();
        let base = self.request_buf.as_ptr() as usize;
        let range = match request_at(&sent, request.path, &mut self.request_buf) {
            Ok(bytes) => sub_range(base, MSG, bytes),
            Err(_) => None,
        };
        let Some(range) = range else {
            self.refuse(seq, priority, Refusal::TooLong);
            return MountAnswer::Forwarded;
        };
        if seq != 0 {
            self.in_flight[self.in_flight_len] = (seq, priority);
            self.in_flight_len += 1;
        }
        self.track(&request.request.kind, request.path);
        self.reply_seq = seq;
        if let Err(ModuleError::ReplyTooLong) = self.call_module(range)
            && self.take_in_flight(seq).is_some()
        {
            self.refuse(seq, priority, Refusal::TooLong);
        }
        // a module that failed is unloaded with its requests, this one included
        MountAnswer::Forwarded
    }

    /// Call the module with the request at `range` of `request_buf`; its events land after those not sent yet.
    fn call_module(&mut self, range: (usize, usize)) -> Result<(), ModuleError> {
        let Some(module) = self.module.as_mut() else {
            return Err(ModuleError::Fault);
        };
        let request = self
            .request_buf
            .get(range.0..range.0 + range.1)
            .unwrap_or_default();
        let free = self.out_buf.get_mut(self.out_len..).unwrap_or_default();
        let mut outbox = Outbox::new(free);
        let base = self.reply_buf.as_ptr() as usize;
        let result = module.process(request, &mut outbox, &mut self.reply_buf);
        let (written, dropped) = (outbox.len, outbox.dropped);
        self.counters.dropped = self.counters.dropped.saturating_add(dropped);
        let reply = match result {
            // a reply outside of the buffer it was given is a broken module
            Ok(reply) => sub_range(base, MSG, reply).ok_or(ModuleError::Fault),
            Err(e) => Err(e),
        };
        match reply {
            Ok(reply) => {
                self.out_len += written;
                self.reply = reply;
                Ok(())
            }
            Err(ModuleError::ReplyTooLong) => {
                self.out_len += written;
                Err(ModuleError::ReplyTooLong)
            }
            Err(ModuleError::Fault) => {
                self.fault();
                Err(ModuleError::Fault)
            }
        }
    }

    fn poll_module(&mut self) {
        let Some(module) = self.module.as_mut() else {
            return;
        };
        let free = self.out_buf.get_mut(self.out_len..).unwrap_or_default();
        let mut outbox = Outbox::new(free);
        let result = module.poll(&mut outbox);
        let (written, dropped) = (outbox.len, outbox.dropped);
        self.counters.dropped = self.counters.dropped.saturating_add(dropped);
        match result {
            Ok(()) => self.out_len += written,
            Err(_) => self.fault(),
        }
    }

    fn fault(&mut self) {
        self.counters.faults = self.counters.faults.saturating_add(1);
        self.unload();
    }

    /// Remember a stream opened or a trait level observed under the slot, to close it when the module goes.
    fn track(&mut self, kind: &RequestKind<'_>, below: &[ResourceIndex]) {
        let Some(prefix) = self.prefix.as_mut() else {
            return;
        };
        let Ok(path) = prefix.join(below) else {
            return;
        };
        let opens = match kind {
            RequestKind::StreamOpen { .. }
            | RequestKind::StreamSideband {
                sideband: StreamSideband::Open,
            } => true,
            RequestKind::StreamSideband {
                sideband: StreamSideband::Close,
            } => false,
            // `entries: [], patch: false` stops observing the level; one that doesn't decode may start it
            RequestKind::Observe { cfg } => !cfg
                .read()
                .is_ok_and(|cfg| cfg.entries.is_empty() && !cfg.patch),
            _ => return,
        };
        let known = self.closable[..self.closable_len]
            .iter()
            .position(|p| PathStore::as_slice(p) == path);
        match (opens, known) {
            (true, None) if self.closable_len < CLOSABLE_MAX => {
                let slot = &mut self.closable[self.closable_len];
                if fill(slot, path).is_ok() {
                    self.closable_len += 1;
                }
            }
            (false, Some(i)) => {
                self.closable.swap(i, self.closable_len - 1);
                self.closable_len -= 1;
            }
            _ => {}
        }
    }

    fn forget_closable(&mut self) {
        let Some(prefix) = self.prefix.as_ref() else {
            return;
        };
        let path = prefix.joined();
        if let Some(i) = self.closable[..self.closable_len]
            .iter()
            .position(|p| PathStore::as_slice(p) == path)
        {
            self.closable.swap(i, self.closable_len - 1);
            self.closable_len -= 1;
        }
    }

    fn take_in_flight(&mut self, seq: u32) -> Option<Priority> {
        let i = self.in_flight[..self.in_flight_len]
            .iter()
            .position(|(s, _)| *s == seq)?;
        let priority = self.in_flight[i].1;
        self.in_flight.swap(i, self.in_flight_len - 1);
        self.in_flight_len -= 1;
        Some(priority)
    }

    fn in_flight_priority(&self, seq: u32) -> Option<Priority> {
        self.in_flight[..self.in_flight_len]
            .iter()
            .find(|(s, _)| *s == seq)
            .map(|(_, p)| *p)
    }

    fn refuse(&mut self, seq: u32, priority: Priority, refusal: Refusal) {
        self.counters.refused = self.counters.refused.saturating_add(1);
        self.answer(seq, priority, refusal);
    }

    fn answer(&mut self, seq: u32, priority: Priority, refusal: Refusal) {
        if seq == 0 {
            return;
        }
        match self.answers.get_mut(self.answers_len) {
            Some(slot) => {
                *slot = (seq, priority, refusal);
                self.answers_len += 1;
            }
            None => self.counters.lost_answers = self.counters.lost_answers.saturating_add(1),
        }
    }

    async fn flush(&mut self, out: &mut impl EventOut) {
        while let Some(next) = self.next_out() {
            let reply = next.reply;
            let bytes = next.bytes;
            let sent = match reply {
                Some(answering) => out.send_reply_bytes(answering.priority, bytes).await,
                None => out.send_with(|scratch| copy_into(bytes, scratch)).await,
            };
            if sent.is_err() {
                self.unsent(reply);
            }
        }
    }

    fn flush_blocking(&mut self, out: &mut impl BlockingEventOut) {
        while let Some(next) = self.next_out() {
            let reply = next.reply;
            let sent = match reply {
                Some(answering) => out.send_reply_bytes_blocking(answering.priority, next.bytes),
                None => out.send_with_blocking(|scratch| copy_into(next.bytes, scratch)),
            };
            if sent.is_err() {
                self.unsent(reply);
            }
        }
    }

    /// The parent refused a message: a request it ended is answered `ResponseSerFailed`, unless that is what was
    /// refused.
    fn unsent(&mut self, reply: Option<Answering>) {
        self.counters.unsent = self.counters.unsent.saturating_add(1);
        if let Some(answering) = reply.filter(|a| a.ends) {
            self.answer(answering.seq, answering.priority, Refusal::TooLong);
        }
    }

    /// The next message to send: the `Close` sidebands a gone module left, the slot's own answers, then the module's
    /// events and reply, each checked and rewritten into `rewrite_buf`.
    fn next_out(&mut self) -> Option<Outgoing<'_>> {
        while self.closing_len > 0 {
            self.closing_len -= 1;
            let path = PathStore::as_slice(&self.closing[self.closing_len]);
            let ser = ww_client_server::util::ser_stream_sideband_event(
                &mut self.rewrite_buf,
                0,
                path,
                StreamSideband::Close,
            );
            match ser.map(<[u8]>::len) {
                Ok(len) => {
                    return Some(Outgoing {
                        bytes: &self.rewrite_buf[..len],
                        reply: None,
                    });
                }
                Err(_) => self.counters.lost_answers = self.counters.lost_answers.saturating_add(1),
            }
        }
        while self.answers_len > 0 {
            self.answers_len -= 1;
            let (seq, priority, refusal) = self.answers[self.answers_len];
            let ser =
                ww_client_server::util::ser_err_event(&mut self.rewrite_buf, seq, refusal.error());
            match ser.map(<[u8]>::len) {
                // never answered again when the parent refuses it: `false`
                Ok(len) => {
                    return Some(Outgoing {
                        bytes: &self.rewrite_buf[..len],
                        reply: Some(Answering {
                            priority,
                            seq,
                            ends: false,
                        }),
                    });
                }
                Err(_) => self.counters.lost_answers = self.counters.lost_answers.saturating_add(1),
            }
        }
        loop {
            let from_reply = match self.next_message() {
                Some(range) => {
                    self.current = range;
                    false
                }
                None if self.reply.1 > 0 => {
                    self.current = core::mem::take(&mut self.reply);
                    true
                }
                None => {
                    self.out_len = 0;
                    self.out_read = 0;
                    return None;
                }
            };
            if let Some((len, reply)) = self.check(from_reply) {
                return Some(Outgoing {
                    bytes: &self.rewrite_buf[..len],
                    reply,
                });
            }
        }
    }

    /// The range of the next message in the outbox.
    fn next_message(&mut self) -> Option<(usize, usize)> {
        let rest = self.out_buf.get(self.out_read..self.out_len)?;
        let (len, rest) = rest.split_first_chunk::<OUTBOX_LEN_BYTES>()?;
        let len = usize::from(u16::from_le_bytes(*len));
        if len > rest.len() {
            // the outbox wrote it: can't happen, and nothing past it is read
            self.out_read = self.out_len;
            return None;
        }
        let start = self.out_read + OUTBOX_LEN_BYTES;
        self.out_read = start + len;
        Some((start, len))
    }

    /// Check the module's message at `current` (of `reply_buf` when `from_reply`, else of `out_buf`) and rewrite it
    /// into `rewrite_buf`: its length there and, for a reply or request data, its priority, seq and whether it ends
    /// its request. `None` when it is dropped.
    fn check(&mut self, from_reply: bool) -> Option<(usize, Option<Answering>)> {
        let (start, len) = self.current;
        let from = if from_reply {
            &self.reply_buf[..]
        } else {
            &self.out_buf[..]
        };
        let bytes = from.get(start..start + len).unwrap_or_default();
        // the reply of a request answers it, whatever seq it names
        let peeked = if from_reply {
            Some(self.reply_seq)
        } else {
            BufReader::new(bytes).read_uvlq32().ok()
        };
        let rewritten = match self.prefix.as_mut() {
            Some(prefix) => prefix.rewrite(bytes, &mut self.rewrite_buf).ok(),
            None => None,
        };
        let Some(rewritten) = rewritten else {
            self.counters.malformed = self.counters.malformed.saturating_add(1);
            // a request whose answer can't pass is answered here, so the client doesn't wait for it
            if let Some(seq) = peeked.filter(|seq| *seq != 0)
                && let Some(priority) = self.take_in_flight(seq)
            {
                self.answer(seq, priority, Refusal::TooLong);
            }
            return None;
        };
        let (len, seq, last, path, closes) = (
            rewritten.bytes.len(),
            rewritten.seq,
            rewritten.last,
            rewritten.path,
            rewritten.closes,
        );
        // a module answers only what it was sent: a reply under another seq could pose as the answer to a request
        // of the same client to the parent itself
        let reply = match seq {
            0 if path => {
                if closes {
                    self.forget_closable();
                }
                None
            }
            seq if seq != 0 && last => self.take_in_flight(seq).map(|priority| Answering {
                priority,
                seq,
                ends: true,
            }),
            seq if seq != 0 => self.in_flight_priority(seq).map(|priority| Answering {
                priority,
                seq,
                ends: false,
            }),
            _ => {
                self.counters.stray = self.counters.stray.saturating_add(1);
                return None;
            }
        };
        if seq != 0 && reply.is_none() {
            self.counters.stray = self.counters.stray.saturating_add(1);
            return None;
        }
        Some((len, reply))
    }
}

fn copy_into(message: &[u8], scratch: &mut [u8]) -> Result<usize, ShrinkWrapError> {
    let to = scratch
        .get_mut(..message.len())
        .ok_or(ShrinkWrapError::OutOfBoundsWriteRawSlice)?;
    to.copy_from_slice(message);
    Ok(message.len())
}

fn fill(store: &mut PathArray<PATH_IDS_MAX>, path: &[ResourceIndex]) -> Result<(), RewriteError> {
    store.truncate(0);
    for id in path {
        store.push(*id)?;
    }
    Ok(())
}

/// The crate, version and wire version of a model's root; `None` when it isn't a model.
/// The crate, version and wire version of a module's model, copied out of it.
#[derive(Clone, Copy)]
struct Identity {
    /// The crate name, then the pre-release, then the build metadata.
    text: [u8; IDENTITY_TEXT_MAX],
    crate_len: u8,
    pre_len: Option<u8>,
    build_len: Option<u8>,
    major: u32,
    minor: u32,
    patch: u32,
    wire: Option<WireVersion>,
}

impl Identity {
    const EMPTY: Identity = Identity {
        text: [0; IDENTITY_TEXT_MAX],
        crate_len: 0,
        pre_len: None,
        build_len: None,
        major: 0,
        minor: 0,
        patch: 0,
        wire: None,
    };

    /// Reads the identity of the root of `model`, a `ww_self::ApiBundle`.
    fn of(model: &[u8]) -> Result<Identity, LoadError> {
        let (api, wire) = root_identity(model).ok_or(LoadError::Model)?;
        let mut identity = Identity {
            major: api.version.major.0,
            minor: api.version.minor.0,
            patch: api.version.patch.0,
            wire,
            ..Identity::EMPTY
        };
        let mut at = 0;
        let mut put = |part: &str| -> Result<u8, LoadError> {
            let to = identity
                .text
                .get_mut(at..at + part.len())
                .ok_or(LoadError::NameTooLong)?;
            to.copy_from_slice(part.as_bytes());
            at += part.len();
            Ok(u8::try_from(part.len()).expect("bounded by IDENTITY_TEXT_MAX"))
        };
        let crate_len = put(api.crate_id)?;
        let pre_len = api.version.pre.map(&mut put).transpose()?;
        let build_len = api.version.build.map(&mut put).transpose()?;
        identity.crate_len = crate_len;
        identity.pre_len = pre_len;
        identity.build_len = build_len;
        Ok(identity)
    }

    /// The identity as a slot's `presence` names it; `None` only for text that isn't UTF-8, which [of](Self::of)
    /// never stores.
    fn get(&self) -> Option<(FullVersion<'_>, Option<WireVersion>)> {
        let mut rest = &self.text[..];
        let mut take = |len: u8| -> Option<&str> {
            let (part, after) = rest.split_at_checked(usize::from(len))?;
            rest = after;
            core::str::from_utf8(part).ok()
        };
        let crate_id = take(self.crate_len)?;
        let pre = match self.pre_len {
            Some(len) => Some(take(len)?),
            None => None,
        };
        let build = match self.build_len {
            Some(len) => Some(take(len)?),
            None => None,
        };
        let version = Version::full(self.major, self.minor, self.patch, pre, build);
        Some((FullVersion::new(crate_id, version), self.wire))
    }
}

fn root_identity(model: &[u8]) -> Option<(FullVersion<'_>, Option<WireVersion>)> {
    let bundle = ww_self::ApiBundle::from_ww_bytes(model).ok()?;
    if bundle.magic != ww_self::MAGIC {
        return None;
    }
    let index = usize::try_from(bundle.root.crate_idx.0).ok()?;
    let api = bundle.ext_crates.iter().nth(index)?.ok()?;
    let declared = bundle
        .ext_wire
        .and_then(|wires| wires.iter().nth(index).and_then(Result::ok).flatten());
    let wire = match declared {
        None if index == 0 => bundle.wire,
        declared => declared,
    };
    Some((api, wire))
}

#[cfg(test)]
mod tests;
