//! The I2C slave engine.

use crate::map::{MapError, RegisterMap};
use crate::queue::{AnswerQueue, QueueError};
use crate::{ConfigError, Ring, Slot, record_byte};
use wire_weaver::shrink_wrap::{MaxLen, RefVec};
use ww_i2c::{
    I2cRecordKind, I2cSlaveAnswers, I2cSlaveConfig, I2cSlaveEvent, I2cSlaveTransfer,
    I2cSlaveUnderrun, MAX_RECORD_LEN,
};
use ww_si::{Prefix, Second};

/// What to put on the bus for a byte the master reads, from [I2cSlave::read].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ReadByte {
    /// Send this byte.
    Byte(u8),
    /// No answer is queued and the policy is `Stretch`: keep SCL low (DW_apb_i2c and STM32 do so on their own while
    /// the TX register is empty), arm a timer for [I2cSlave::stretch_deadline], and call [I2cSlave::resume] when it
    /// fires and after each answer the host queues; it gives the byte to send.
    Stretch,
}

/// How the master's reads are answered, from the config.
#[derive(Copy, Clone)]
enum Mode {
    Off,
    Registers {
        pointer_bytes: u8,
        len: u32,
        /// 0: no page.
        page: u32,
        read_only: bool,
    },
    Queue {
        policy: Underrun,
    },
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum Underrun {
    Fill(u8),
    /// Limit in µs.
    Stretch {
        max: u32,
        fill: u8,
    },
    Nack,
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum Segment {
    /// Between transfers.
    None,
    Write,
    Read,
}

/// Where the bytes of the read going on come from, in queue mode.
#[derive(Copy, Clone, PartialEq, Eq)]
enum Source {
    /// The first byte wasn't asked for yet.
    Unset,
    /// The current answer of the queue, then the fill byte.
    Answer,
    /// The fill byte only (no answer, or it came too late).
    Fill,
}

/// One transfer (or a `ReadWaiting` before it), written in place.
pub(crate) struct I2cRecord<const B: usize> {
    start: u32,
    end: u32,
    read: bool,
    kind: I2cRecordKind,
    pointer: Option<u32>,
    /// Bytes that went by; those past `B` are not kept (truncated).
    count: u32,
    filled: u16,
    stretched: Option<u32>,
    /// A `ReadWaiting` comes before this transfer and wasn't read yet.
    waiting: bool,
    /// When the stretch of that `ReadWaiting` began.
    waiting_at: u32,
    bytes: [u8; B],
}

impl<const B: usize> Slot for I2cRecord<B> {
    const EMPTY: Self = I2cRecord {
        start: 0,
        end: 0,
        read: false,
        kind: I2cRecordKind::Complete,
        pointer: None,
        count: 0,
        filled: 0,
        stretched: None,
        waiting: false,
        waiting_at: 0,
        bytes: [0; B],
    };
}

/// I2C slave engine, see the [crate docs](crate).
///
/// - `MAP`: register map bytes (`I2cSlaveCapabilities::map_len`, 0 for none);
/// - `STAGE`: bytes of host map updates waiting for a stop, 6 more than the data per update;
/// - `Q` answers (`I2cSlaveCapabilities::queue_len`, 0 for none) of up to `A` bytes;
/// - `EV` events of up to `REC` bytes each (at most `MAX_RECORD_LEN`); longer transfers are `Truncated`.
///
/// Interrupt side, per transfer: [address](Self::address) (or let [write](Self::write) / [read](Self::read) see the
/// direction), [write](Self::write) per byte the master wrote, [read](Self::read) per byte it reads (and
/// [resume](Self::resume) while stretching), [stop](Self::stop) at the stop condition. A repeated start shows as the
/// next `address` (or a byte of the other direction); [restart](Self::restart) marks it where the peripheral reports
/// it. Task side: [configure](Self::configure), [map_write](Self::map_write) / [map_read](Self::map_read),
/// [push_answer](Self::push_answer), [peek_event](Self::peek_event) / [pop_event](Self::pop_event).
pub struct I2cSlave<
    const MAP: usize,
    const STAGE: usize,
    const Q: usize,
    const A: usize,
    const EV: usize,
    const REC: usize,
> {
    map: RegisterMap<MAP, STAGE>,
    queue: AnswerQueue<Q, A>,
    events: Ring<I2cRecord<REC>, EV>,
    mode: Mode,
    segment: Segment,
    /// From the first address to the stop: host map updates wait meanwhile.
    in_transaction: bool,
    // register map
    pointer: u32,
    page_start: u32,
    pointer_got: u8,
    pointer_acc: u32,
    // queue
    source: Source,
    answer_pos: u16,
    /// Bytes of this read sent as the fill byte.
    filled: u16,
    stretching: bool,
    stretch_since: u32,
    /// A `ReadWaiting` of a transfer the ring had no room for.
    orphan_waiting: bool,
}

impl<
    const MAP: usize,
    const STAGE: usize,
    const Q: usize,
    const A: usize,
    const EV: usize,
    const REC: usize,
> Default for I2cSlave<MAP, STAGE, Q, A, EV, REC>
{
    fn default() -> Self {
        Self::new()
    }
}

impl<
    const MAP: usize,
    const STAGE: usize,
    const Q: usize,
    const A: usize,
    const EV: usize,
    const REC: usize,
> I2cSlave<MAP, STAGE, Q, A, EV, REC>
{
    const SIZES_OK: () = {
        assert!(REC <= MAX_RECORD_LEN, "REC above ww_i2c::MAX_RECORD_LEN");
        assert!(EV > 0, "EV: at least one event");
        assert!(A <= u16::MAX as usize, "A: answers up to 65535 bytes");
        assert!(MAP <= u32::MAX as usize);
    };

    /// Off until [configure](Self::configure), with the map zeroed; a `const fn`, for a `static`.
    pub const fn new() -> Self {
        #[allow(clippy::let_unit_value)]
        let _ = Self::SIZES_OK;
        I2cSlave {
            map: RegisterMap::new(),
            queue: AnswerQueue::new(),
            events: Ring::new(),
            mode: Mode::Off,
            segment: Segment::None,
            in_transaction: false,
            pointer: 0,
            page_start: 0,
            pointer_got: 0,
            pointer_acc: 0,
            source: Source::Unset,
            answer_pos: 0,
            filled: 0,
            stretching: false,
            stretch_since: 0,
            orphan_waiting: false,
        }
    }

    // ---- task side ----

    /// Starts slave mode with `config` (`slave_events` opened): checks it against the engine's sizes, forgets the
    /// events of a previous run and resets the pointer to 0. The map keeps its contents, answers queued stay.
    /// Whether the peripheral can do the address, `io_level`, a 10-bit address or the stretch limit is the
    /// firmware's to check (its `I2cSlaveCapabilities`).
    pub fn configure(&mut self, config: &I2cSlaveConfig) -> Result<(), ConfigError> {
        let mode = match config.answers {
            I2cSlaveAnswers::Registers {
                pointer_bytes,
                len,
                page,
                read_only,
            } => {
                if MAP == 0 || pointer_bytes > 4 {
                    return Err(ConfigError::Unsupported);
                }
                if len as usize > MAP {
                    return Err(ConfigError::BufferTooBig);
                }
                if len == 0 {
                    return Err(ConfigError::InvalidArgument("register map len is 0"));
                }
                if page == Some(0) {
                    return Err(ConfigError::InvalidArgument("page of 0 bytes"));
                }
                Mode::Registers {
                    pointer_bytes,
                    len,
                    page: page.unwrap_or(0) as u32,
                    read_only,
                }
            }
            I2cSlaveAnswers::Queue { empty } => {
                if Q == 0 || A == 0 {
                    return Err(ConfigError::Unsupported);
                }
                let policy = match empty {
                    I2cSlaveUnderrun::Fill(fill) => Underrun::Fill(fill),
                    I2cSlaveUnderrun::Stretch { max, fill } => match max.to_u32_in(Prefix::Micro) {
                        Some(max) => Underrun::Stretch { max, fill },
                        None => {
                            return Err(ConfigError::InvalidArgument("stretch limit out of range"));
                        }
                    },
                    I2cSlaveUnderrun::Nack => Underrun::Nack,
                    I2cSlaveUnderrun::Unknown(_) => return Err(ConfigError::Unsupported),
                };
                Mode::Queue { policy }
            }
            I2cSlaveAnswers::Unknown(_) => return Err(ConfigError::Unsupported),
        };
        self.reset_bus_state();
        self.events.reset();
        self.mode = mode;
        Ok(())
    }

    /// Stops slave mode (`slave_events` closed): the address isn't acknowledged any more, queued answers are
    /// dropped, staged map updates applied.
    pub fn disable(&mut self) {
        self.mode = Mode::Off;
        self.reset_bus_state();
        self.queue.reset();
    }

    fn reset_bus_state(&mut self) {
        self.segment = Segment::None;
        self.in_transaction = false;
        self.map.apply_staged();
        self.pointer = 0;
        self.page_start = 0;
        self.pointer_got = 0;
        self.source = Source::Unset;
        self.answer_pos = 0;
        self.filled = 0;
        self.stretching = false;
        self.orphan_waiting = false;
        self.queue.finish();
    }

    /// Slave mode is on.
    pub fn is_enabled(&self) -> bool {
        !matches!(self.mode, Mode::Off)
    }

    /// No transaction going on: a [map_write](Self::map_write) now takes effect at once.
    pub fn is_idle(&self) -> bool {
        !self.in_transaction
    }

    /// `slave_map_write`: writes into the map now if no transaction goes on, else at its stop. `Busy` when the
    /// staging room is full: wait for [is_idle](Self::is_idle) (the firmware's stop interrupt) and try again.
    pub fn map_write(&mut self, offset: u32, data: &[u8]) -> Result<(), MapError> {
        self.map.write(offset, data, !self.in_transaction)
    }

    /// `slave_map_read`: the map as the master sees it (staged updates not yet applied are not in it).
    pub fn map_read(&self, offset: u32, out: &mut [u8]) -> Result<(), MapError> {
        self.map.read(offset, out)
    }

    /// The whole map, as the master sees it.
    pub fn map(&self) -> &[u8; MAP] {
        &self.map.bytes
    }

    /// A host answer from `slave_answers`. After it, while [is_stretching](Self::is_stretching), call
    /// [resume](Self::resume) for the byte to send; with `Nack`, the address is acknowledged again
    /// ([acks_address](Self::acks_address)).
    pub fn push_answer(&mut self, answer: &[u8]) -> Result<(), QueueError> {
        self.queue.push(answer)
    }

    /// Room for answers: the credits the `slave_answers` sink may give the host.
    pub fn free_answer_slots(&self) -> usize {
        self.queue.free()
    }

    /// Answers waiting for a read (not counting one being sent).
    pub fn answers_waiting(&self) -> usize {
        self.queue.waiting()
    }

    /// `slave_clear_answers`: drops the waiting answers (not one being sent) and returns how many.
    pub fn clear_answers(&mut self) -> u32 {
        self.queue.clear()
    }

    /// The oldest event not read yet, with its time (the start condition of a transfer, or when stretching began
    /// for `ReadWaiting`). Stays until [pop_event](Self::pop_event), so a full stream can take it later.
    pub fn peek_event(&self) -> Option<(Second, I2cSlaveEvent<'_>)> {
        if let Some(r) = self.events.oldest() {
            if r.waiting {
                return Some((Second::micro(r.waiting_at), I2cSlaveEvent::ReadWaiting));
            }
            let kept = (r.count as usize).min(REC);
            let kind = if r.count as usize > REC && r.kind != I2cRecordKind::BusError {
                I2cRecordKind::Truncated
            } else {
                r.kind
            };
            let transfer = I2cSlaveTransfer {
                end: Second::micro(r.end),
                read: r.read,
                kind,
                pointer: r.pointer,
                bytes: MaxLen::new_unchecked(RefVec::new_bytes(&r.bytes[..kept])),
                filled: r.filled,
                stretched: r.stretched.map(Second::micro),
            };
            return Some((Second::micro(r.start), I2cSlaveEvent::Transfer(transfer)));
        }
        if let Some(r) = self.events.current_ref()
            && r.waiting
        {
            return Some((Second::micro(r.waiting_at), I2cSlaveEvent::ReadWaiting));
        }
        if self.orphan_waiting {
            return Some((
                Second::micro(self.stretch_since),
                I2cSlaveEvent::ReadWaiting,
            ));
        }
        None
    }

    /// Drops the event [peek_event](Self::peek_event) gave.
    pub fn pop_event(&mut self) {
        if let Some(r) = self.events.oldest_mut() {
            if r.waiting {
                r.waiting = false;
            } else {
                self.events.pop();
            }
            return;
        }
        if let Some(r) = self.events.current()
            && r.waiting
        {
            r.waiting = false;
            return;
        }
        self.orphan_waiting = false;
    }

    /// Transfers not recorded since the last call because the event ring was full: the loss to report on the
    /// counted stream.
    pub fn take_dropped(&mut self) -> u32 {
        self.events.take_dropped()
    }

    /// Whether the address should be acknowledged now: off, or `Nack` with no answer queued, it shouldn't. For a
    /// peripheral that acknowledges by itself (DW_apb_i2c), enable its slave address by this after
    /// [configure](Self::configure), [push_answer](Self::push_answer) and [stop](Self::stop).
    pub fn acks_address(&self) -> bool {
        match self.mode {
            Mode::Off => false,
            Mode::Queue {
                policy: Underrun::Nack,
            } => self.queue.waiting() > 0 || self.queue.has_current(),
            _ => true,
        }
    }

    // ---- interrupt side ----

    /// The master addressed this device (start or repeated start), `read` from the R/W bit; ends a transfer going
    /// on as a repeated start. Returns whether to acknowledge, see [acks_address](Self::acks_address); a NACKed
    /// address is not reported.
    #[inline]
    pub fn address(&mut self, read: bool, now: u32) -> bool {
        if self.segment != Segment::None {
            self.end_segment(I2cRecordKind::RepeatedStart, now);
        }
        if !self.acks_address() {
            return false;
        }
        self.open(read, now);
        true
    }

    /// A repeated start the peripheral reports (DW_apb_i2c `RESTART_DET`): ends the transfer going on.
    #[inline]
    pub fn restart(&mut self, now: u32) {
        if self.segment != Segment::None {
            self.end_segment(I2cRecordKind::RepeatedStart, now);
        }
    }

    /// A byte the master wrote. Opens a write transfer if [address](Self::address) wasn't called (ending a read as a
    /// repeated start).
    #[inline(always)]
    pub fn write(&mut self, byte: u8, now: u32) {
        if let Mode::Off = self.mode {
            return;
        }
        if self.segment != Segment::Write {
            if self.segment == Segment::Read {
                self.end_segment(I2cRecordKind::RepeatedStart, now);
            }
            self.open(false, now);
        }
        if let Mode::Registers {
            pointer_bytes,
            len,
            page,
            read_only,
        } = self.mode
        {
            if self.pointer_got < pointer_bytes {
                self.pointer_acc = (self.pointer_acc << 8) | byte as u32;
                self.pointer_got += 1;
                if self.pointer_got == pointer_bytes {
                    self.set_pointer(self.pointer_acc, len, page);
                }
            } else {
                if !read_only {
                    self.map.bytes[self.pointer as usize] = byte;
                }
                let next = self.pointer + 1;
                self.pointer = if page != 0 {
                    if next - self.page_start == page || next == len {
                        self.page_start
                    } else {
                        next
                    }
                } else if next == len {
                    0
                } else {
                    next
                };
            }
        }
        if let Some(r) = self.events.current() {
            record_byte(&mut r.bytes, &mut r.count, byte);
        }
    }

    /// The master reads a byte: what to send, or to stretch. Opens a read transfer if [address](Self::address)
    /// wasn't called (ending a write as a repeated start).
    #[inline(always)]
    pub fn read(&mut self, now: u32) -> ReadByte {
        if let Mode::Off = self.mode {
            return ReadByte::Byte(0xFF);
        }
        if self.segment != Segment::Read {
            if self.segment == Segment::Write {
                self.end_segment(I2cRecordKind::RepeatedStart, now);
            }
            self.open(true, now);
        }
        let byte = match self.mode {
            Mode::Registers { len, .. } => {
                let b = self.map.bytes[self.pointer as usize];
                let next = self.pointer + 1;
                self.pointer = if next == len { 0 } else { next };
                b
            }
            Mode::Queue { policy } => {
                if self.source == Source::Unset {
                    if self.queue.take() {
                        self.source = Source::Answer;
                    } else if let Underrun::Stretch { .. } = policy {
                        if !self.stretching {
                            self.start_stretch(now);
                        }
                        return ReadByte::Stretch;
                    } else {
                        self.source = Source::Fill;
                    }
                }
                self.queue_byte(policy)
            }
            Mode::Off => 0xFF,
        };
        if let Some(r) = self.events.current() {
            record_byte(&mut r.bytes, &mut r.count, byte);
        }
        ReadByte::Byte(byte)
    }

    #[inline(always)]
    fn queue_byte(&mut self, policy: Underrun) -> u8 {
        let answer = self.queue.current();
        if self.source == Source::Answer && (self.answer_pos as usize) < answer.len() {
            let b = answer[self.answer_pos as usize];
            self.answer_pos += 1;
            b
        } else {
            self.filled += 1;
            match policy {
                Underrun::Fill(f) | Underrun::Stretch { fill: f, .. } => f,
                Underrun::Nack => 0xFF,
            }
        }
    }

    #[inline(never)]
    fn start_stretch(&mut self, now: u32) {
        self.stretching = true;
        self.stretch_since = now;
        match self.events.current() {
            Some(r) => {
                r.waiting = true;
                r.waiting_at = now;
            }
            None => self.orphan_waiting = true,
        }
    }

    /// SCL is held waiting for an answer ([ReadByte::Stretch]).
    pub fn is_stretching(&self) -> bool {
        self.stretching
    }

    /// When the stretch going on has to end (wrapping µs), for a timer; `None` when not stretching.
    pub fn stretch_deadline(&self) -> Option<u32> {
        match self.mode {
            Mode::Queue {
                policy: Underrun::Stretch { max, .. },
            } if self.stretching => Some(self.stretch_since.wrapping_add(max)),
            _ => None,
        }
    }

    /// While stretching: the byte to send now, when an answer came (its first byte) or the limit passed (the fill
    /// byte, and the rest of this read too; the late answer goes to the next read). `None`: keep stretching.
    pub fn resume(&mut self, now: u32) -> Option<u8> {
        let Mode::Queue {
            policy: policy @ Underrun::Stretch { max, .. },
        } = self.mode
        else {
            return None;
        };
        if !self.stretching {
            return None;
        }
        if self.queue.take() {
            self.source = Source::Answer;
        } else if now.wrapping_sub(self.stretch_since) >= max {
            self.source = Source::Fill;
        } else {
            return None;
        }
        self.end_stretch(now);
        let byte = self.queue_byte(policy);
        if let Some(r) = self.events.current() {
            record_byte(&mut r.bytes, &mut r.count, byte);
        }
        Some(byte)
    }

    fn end_stretch(&mut self, now: u32) {
        self.stretching = false;
        let held = now.wrapping_sub(self.stretch_since);
        if let Some(r) = self.events.current() {
            r.stretched = Some(r.stretched.unwrap_or(0).saturating_add(held));
        }
    }

    /// The last `n` bytes [read](Self::read) gave were not clocked out (left in a TX FIFO the firmware loaded ahead,
    /// the master NACKed earlier): they are taken back, so the pointer, the answer and the record count only what
    /// the master got. Call before [stop](Self::stop).
    pub fn unsent(&mut self, n: u32) {
        if self.segment != Segment::Read {
            return;
        }
        match self.mode {
            Mode::Registers { len, .. } => {
                let n = n % len;
                self.pointer = if self.pointer >= n {
                    self.pointer - n
                } else {
                    self.pointer + len - n
                };
            }
            Mode::Queue { .. } => {
                let n = n.min(u16::MAX as u32) as u16;
                let from_fill = n.min(self.filled);
                self.filled -= from_fill;
                let from_answer = (n - from_fill).min(self.answer_pos);
                self.answer_pos -= from_answer;
            }
            Mode::Off => {}
        }
        if let Some(r) = self.events.current() {
            r.count = r.count.saturating_sub(n);
        }
    }

    /// The stop condition: ends the transaction, applies staged host map updates. Wake the task after it: an
    /// event is there to send.
    #[inline]
    pub fn stop(&mut self, now: u32) {
        if self.segment != Segment::None {
            self.end_segment(I2cRecordKind::Complete, now);
        }
        self.end_transaction();
    }

    /// A start or stop in the middle of a byte, or a stuck line: ends the transaction, the transfer going on is
    /// reported as `BusError`.
    pub fn bus_error(&mut self, now: u32) {
        if self.segment != Segment::None {
            self.end_segment(I2cRecordKind::BusError, now);
        }
        self.end_transaction();
    }

    fn end_transaction(&mut self) {
        self.in_transaction = false;
        self.map.apply_staged();
    }

    #[inline]
    fn open(&mut self, read: bool, now: u32) {
        self.segment = if read { Segment::Read } else { Segment::Write };
        self.in_transaction = true;
        let mut pointer = None;
        if let Mode::Registers {
            pointer_bytes,
            page,
            ..
        } = self.mode
        {
            self.pointer_got = if read { pointer_bytes } else { 0 };
            self.pointer_acc = 0;
            if !read && page != 0 {
                self.page_start = self.pointer - self.pointer % page;
            }
            pointer = Some(self.pointer);
        }
        self.source = Source::Unset;
        self.answer_pos = 0;
        self.filled = 0;
        if let Some(r) = self.events.open() {
            r.start = now;
            r.read = read;
            r.pointer = pointer;
            r.count = 0;
            r.filled = 0;
            r.stretched = None;
            r.waiting = false;
        }
    }

    #[inline(never)]
    fn set_pointer(&mut self, value: u32, len: u32, page: u32) {
        self.pointer = value % len;
        if page != 0 {
            self.page_start = self.pointer - self.pointer % page;
        }
        if let Some(r) = self.events.current() {
            r.pointer = Some(self.pointer);
        }
    }

    #[inline]
    fn end_segment(&mut self, kind: I2cRecordKind, now: u32) {
        if self.stretching {
            self.end_stretch(now);
        }
        let filled = self.filled;
        if let Some(r) = self.events.current() {
            r.end = now;
            r.kind = kind;
            r.filled = filled;
        }
        self.events.commit();
        self.queue.finish();
        self.segment = Segment::None;
    }
}
