//! The SPI slave engine.

use crate::map::{MapError, RegisterMap};
use crate::queue::{AnswerQueue, QueueError};
use crate::{ConfigError, Ring, Slot, record_byte};
use wire_weaver::shrink_wrap::{MaxLen, RefVec};
use ww_si::Second;
use ww_spi::{SPI_MAX, SpiRwBit, SpiSlaveAnswers, SpiSlaveConfig, SpiSlaveFrame};

#[derive(Copy, Clone)]
enum Mode {
    Off,
    Queue {
        idle: u8,
        repeat_last: bool,
    },
    Registers {
        addr_bytes: u8,
        /// Bit of the first byte that says read or write, 0xFF for read-only.
        rw_mask: u8,
        /// The bit set means read.
        set_is_read: bool,
        dummy_bytes: u8,
        idle: u8,
        len: u32,
    },
}

/// One frame, written in place.
pub(crate) struct SpiRecord<const B: usize> {
    start: u32,
    end: u32,
    overrun: bool,
    underrun: bool,
    addr: Option<u32>,
    mosi_count: u32,
    miso_count: u32,
    mosi: [u8; B],
    miso: [u8; B],
}

impl<const B: usize> Slot for SpiRecord<B> {
    const EMPTY: Self = SpiRecord {
        start: 0,
        end: 0,
        overrun: false,
        underrun: false,
        addr: None,
        mosi_count: 0,
        miso_count: 0,
        mosi: [0; B],
        miso: [0; B],
    };
}

/// SPI slave engine, see the [crate docs](crate).
///
/// - `MAP`: register map bytes (`SpiSlaveCapabilities::map_len`, 0 for none);
/// - `STAGE`: bytes of host map updates waiting for CS to be released, 6 more than the data per update;
/// - `Q` answers (`SpiSlaveCapabilities::queue_len`, 0 for none) of up to `A` bytes;
/// - `EV` frames of up to `REC` bytes each way (at most `SPI_MAX`); longer frames are `truncated`.
///
/// Interrupt side: [miso](Self::miso) for each byte to load into the TX FIFO (also ahead of the frame, before CS is
/// asserted, as a FIFO wants), [cs_asserted](Self::cs_asserted), [mosi](Self::mosi) per byte received,
/// [cs_released](Self::cs_released); [overrun](Self::overrun) when the RX FIFO overflowed. A frame's answer is
/// picked at its first `miso` call; [rewind](Self::rewind) takes back what was loaded ahead when the firmware can
/// empty its TX FIFO (an answer that came meanwhile then goes out in this frame).
pub struct SpiSlave<
    const MAP: usize,
    const STAGE: usize,
    const Q: usize,
    const A: usize,
    const EV: usize,
    const REC: usize,
> {
    map: RegisterMap<MAP, STAGE>,
    queue: AnswerQueue<Q, A>,
    events: Ring<SpiRecord<REC>, EV>,
    mode: Mode,
    /// CS is asserted.
    active: bool,
    /// The frame's record is open and its answer picked (by `miso` ahead of CS, or `cs_asserted`).
    framed: bool,
    /// MISO bytes given for this frame.
    tx: u32,
    /// MOSI bytes got in this frame.
    rx: u32,
    // queue
    /// This frame sends the current answer of the queue.
    answering: bool,
    /// ... which it took from the queue (not a repeat).
    took: bool,
    // registers
    is_read: bool,
    addr_acc: u32,
    /// The address is complete (`rx >= addr_bytes`).
    addr_known: bool,
    addr: u32,
    /// Next map byte to send, once the data started.
    read_ptr: u32,
    read_started: bool,
    /// Next map byte the master writes.
    write_ptr: u32,
}

impl<
    const MAP: usize,
    const STAGE: usize,
    const Q: usize,
    const A: usize,
    const EV: usize,
    const REC: usize,
> Default for SpiSlave<MAP, STAGE, Q, A, EV, REC>
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
> SpiSlave<MAP, STAGE, Q, A, EV, REC>
{
    const SIZES_OK: () = {
        assert!(REC <= SPI_MAX, "REC above ww_spi::SPI_MAX");
        assert!(EV > 0, "EV: at least one event");
        assert!(A <= u16::MAX as usize, "A: answers up to 65535 bytes");
        assert!(MAP <= u32::MAX as usize);
    };

    /// Off until [configure](Self::configure), with the map zeroed; a `const fn`, for a `static`.
    pub const fn new() -> Self {
        #[allow(clippy::let_unit_value)]
        let _ = Self::SIZES_OK;
        SpiSlave {
            map: RegisterMap::new(),
            queue: AnswerQueue::new(),
            events: Ring::new(),
            mode: Mode::Off,
            active: false,
            framed: false,
            tx: 0,
            rx: 0,
            answering: false,
            took: false,
            is_read: false,
            addr_acc: 0,
            addr_known: false,
            addr: 0,
            read_ptr: 0,
            read_started: false,
            write_ptr: 0,
        }
    }

    // ---- task side ----

    /// Starts slave mode with `config` (`slave_frames` opened): checks it against the engine's sizes and forgets
    /// the frames of a previous run. The map keeps its contents, answers queued stay. Only 8-bit words: a firmware
    /// with wider ones splits them into bytes, most significant first, and says so in its capabilities. Mode, bit
    /// order and `io_level` are the peripheral's.
    pub fn configure(&mut self, config: &SpiSlaveConfig) -> Result<(), ConfigError> {
        if config.word_bits != 8 {
            return Err(ConfigError::Unsupported);
        }
        let mode = match config.answers {
            SpiSlaveAnswers::Queue { idle, repeat_last } => {
                if Q == 0 || A == 0 {
                    return Err(ConfigError::Unsupported);
                }
                Mode::Queue { idle, repeat_last }
            }
            SpiSlaveAnswers::Registers(r) => {
                if MAP == 0 || r.addr_bytes > 4 {
                    return Err(ConfigError::Unsupported);
                }
                if r.len as usize > MAP {
                    return Err(ConfigError::BufferTooBig);
                }
                if r.len == 0 {
                    return Err(ConfigError::InvalidArgument("register map len is 0"));
                }
                let (rw_mask, set_is_read) = match r.rw {
                    SpiRwBit::ReadOnly => (0, true),
                    SpiRwBit::SetIsRead { bit } | SpiRwBit::ClearIsRead { bit } if bit > 7 => {
                        return Err(ConfigError::InvalidArgument("rw bit above 7"));
                    }
                    SpiRwBit::SetIsRead { bit } => (1 << bit, true),
                    SpiRwBit::ClearIsRead { bit } => (1 << bit, false),
                    SpiRwBit::Unknown(_) => return Err(ConfigError::Unsupported),
                };
                if rw_mask != 0 && r.addr_bytes == 0 {
                    return Err(ConfigError::InvalidArgument("rw bit without address bytes"));
                }
                Mode::Registers {
                    addr_bytes: r.addr_bytes,
                    rw_mask,
                    set_is_read,
                    dummy_bytes: r.dummy_bytes,
                    idle: r.idle,
                    len: r.len,
                }
            }
            SpiSlaveAnswers::Unknown(_) => return Err(ConfigError::Unsupported),
        };
        self.reset_bus_state();
        self.events.reset();
        self.mode = mode;
        Ok(())
    }

    /// Stops slave mode (`slave_frames` closed): queued answers are dropped, staged map updates applied.
    pub fn disable(&mut self) {
        self.mode = Mode::Off;
        self.reset_bus_state();
        self.queue.reset();
    }

    fn reset_bus_state(&mut self) {
        self.active = false;
        self.framed = false;
        self.events.discard();
        self.tx = 0;
        self.rx = 0;
        self.answering = false;
        self.map.apply_staged();
    }

    /// Slave mode is on.
    pub fn is_enabled(&self) -> bool {
        !matches!(self.mode, Mode::Off)
    }

    /// No frame going on or loaded ahead: a [map_write](Self::map_write) now takes effect at once.
    pub fn is_idle(&self) -> bool {
        !self.active && !self.framed
    }

    /// `slave_map_write`: writes into the map now if no frame goes on (or is loaded ahead), else when CS is
    /// released. `Busy` when the staging room is full: try again after the next [cs_released](Self::cs_released),
    /// which empties it (a firmware that loads its FIFO right after each frame is rarely [idle](Self::is_idle)).
    pub fn map_write(&mut self, offset: u32, data: &[u8]) -> Result<(), MapError> {
        let idle = self.is_idle();
        self.map.write(offset, data, idle)
    }

    /// `slave_map_read`: the map as the master sees it.
    pub fn map_read(&self, offset: u32, out: &mut [u8]) -> Result<(), MapError> {
        self.map.read(offset, out)
    }

    /// The whole map, as the master sees it.
    pub fn map(&self) -> &[u8; MAP] {
        &self.map.bytes
    }

    /// A host answer from `slave_answers`, for the next frame whose answer isn't picked yet.
    pub fn push_answer(&mut self, answer: &[u8]) -> Result<(), QueueError> {
        self.queue.push(answer)
    }

    /// Room for answers: the credits the `slave_answers` sink may give the host. With `repeat_last`, the answer
    /// kept for repeating takes a slot until the next one replaces it.
    pub fn free_answer_slots(&self) -> usize {
        self.queue.free()
    }

    /// Answers waiting for a frame (not counting one being sent or kept to repeat).
    pub fn answers_waiting(&self) -> usize {
        self.queue.waiting()
    }

    /// `slave_clear_answers`: drops the waiting answers and returns how many.
    pub fn clear_answers(&mut self) -> u32 {
        self.queue.clear()
    }

    /// The oldest frame not read yet, with the time CS was asserted. Stays until [pop_event](Self::pop_event).
    pub fn peek_event(&self) -> Option<(Second, SpiSlaveFrame<'_>)> {
        let r = self.events.oldest()?;
        let mosi = (r.mosi_count as usize).min(REC);
        let miso = (r.miso_count as usize).min(REC);
        let frame = SpiSlaveFrame {
            end: Second::micro(r.end),
            truncated: r.mosi_count as usize > REC,
            overrun: r.overrun,
            underrun: r.underrun,
            addr: r.addr,
            mosi: MaxLen::new_unchecked(RefVec::new_bytes(&r.mosi[..mosi])),
            miso: MaxLen::new_unchecked(RefVec::new_bytes(&r.miso[..miso])),
        };
        Some((Second::micro(r.start), frame))
    }

    /// Drops the frame [peek_event](Self::peek_event) gave.
    pub fn pop_event(&mut self) {
        self.events.pop();
    }

    /// Frames not recorded since the last call because the event ring was full: the loss to report on the counted
    /// stream.
    pub fn take_dropped(&mut self) -> u32 {
        self.events.take_dropped()
    }

    // ---- interrupt side ----

    /// The next byte to send on MISO: call it whenever the TX FIFO has room, ahead of CS too.
    #[inline(always)]
    pub fn miso(&mut self) -> u8 {
        if let Mode::Off = self.mode {
            return 0xFF;
        }
        if !self.framed {
            self.begin_frame();
        }
        let i = self.tx;
        self.tx += 1;
        let byte = match self.mode {
            Mode::Queue { idle, .. } => {
                let answer = self.queue.current();
                if self.answering && (i as usize) < answer.len() {
                    answer[i as usize]
                } else {
                    idle
                }
            }
            Mode::Registers {
                addr_bytes,
                dummy_bytes,
                idle,
                len,
                ..
            } => {
                let data_at = addr_bytes as u32 + dummy_bytes as u32;
                if i < data_at || !self.addr_known || !self.is_read {
                    idle
                } else {
                    if !self.read_started {
                        self.start_read(i - data_at, len);
                    }
                    let b = self.map.bytes[self.read_ptr as usize];
                    let next = self.read_ptr + 1;
                    self.read_ptr = if next == len { 0 } else { next };
                    b
                }
            }
            Mode::Off => 0xFF,
        };
        if let Some(r) = self.events.current() {
            record_byte(&mut r.miso, &mut r.miso_count, byte);
        }
        byte
    }

    /// The first data byte is wanted: the read pointer at the address plus the data bytes already gone by as idle
    /// (the address came too late for them).
    #[inline(never)]
    fn start_read(&mut self, late: u32, len: u32) {
        let p = self.addr + late % len;
        self.read_ptr = if p >= len { p - len } else { p };
        self.read_started = true;
    }

    /// CS went active.
    #[inline]
    pub fn cs_asserted(&mut self, now: u32) {
        if let Mode::Off = self.mode {
            return;
        }
        if !self.framed {
            self.begin_frame();
        }
        self.active = true;
        if let Some(r) = self.events.current() {
            r.start = now;
        }
    }

    /// A byte the master sent on MOSI.
    #[inline(always)]
    pub fn mosi(&mut self, byte: u8) {
        if let Mode::Off = self.mode {
            return;
        }
        if !self.framed {
            self.begin_frame();
        }
        let j = self.rx;
        self.rx += 1;
        if let Mode::Registers {
            addr_bytes,
            rw_mask,
            set_is_read,
            len,
            ..
        } = self.mode
        {
            if j < addr_bytes as u32 {
                let mut b = byte;
                if j == 0 && rw_mask != 0 {
                    self.is_read = (byte & rw_mask != 0) == set_is_read;
                    b &= !rw_mask;
                }
                self.addr_acc = (self.addr_acc << 8) | b as u32;
                if j + 1 == addr_bytes as u32 {
                    self.set_addr(self.addr_acc, len);
                }
            } else if !self.is_read {
                self.map.bytes[self.write_ptr as usize] = byte;
                let next = self.write_ptr + 1;
                self.write_ptr = if next == len { 0 } else { next };
            }
        }
        if let Some(r) = self.events.current() {
            record_byte(&mut r.mosi, &mut r.mosi_count, byte);
        }
    }

    #[inline(never)]
    fn set_addr(&mut self, value: u32, len: u32) {
        self.addr = value % len;
        self.write_ptr = self.addr;
        self.addr_known = true;
        if let Some(r) = self.events.current() {
            r.addr = Some(self.addr);
        }
    }

    /// The master sent bytes faster than the firmware took them: some MOSI bytes are lost.
    pub fn overrun(&mut self) {
        if let Some(r) = self.events.current() {
            r.overrun = true;
        }
    }

    /// CS was released: the frame ends (MISO bytes given past the bytes clocked are not counted as sent), staged map
    /// updates are applied. The next [miso](Self::miso) starts the next frame. Wake the task after it.
    #[inline]
    pub fn cs_released(&mut self, now: u32) {
        if !self.framed {
            return;
        }
        let rx = self.rx;
        if let Some(r) = self.events.current() {
            r.end = now;
            r.miso_count = r.miso_count.min(rx);
        }
        self.events.commit();
        if let Mode::Queue {
            repeat_last: false, ..
        } = self.mode
        {
            self.queue.finish();
        }
        self.active = false;
        self.framed = false;
        self.map.apply_staged();
    }

    /// Takes back the MISO bytes given ahead of a frame that hasn't started (CS not asserted, nothing received):
    /// for a firmware that emptied its TX FIFO, say because an answer came. The frame's answer is picked again
    /// at the next [miso](Self::miso).
    pub fn rewind(&mut self) {
        if !self.framed || self.active || self.rx > 0 {
            return;
        }
        if self.took {
            self.queue.untake();
        }
        self.events.discard();
        self.framed = false;
    }

    #[inline(never)]
    fn begin_frame(&mut self) {
        self.framed = true;
        self.tx = 0;
        self.rx = 0;
        let mut underrun = false;
        let mut addr = None;
        match self.mode {
            Mode::Queue { repeat_last, .. } => {
                self.took = self.queue.take();
                if self.took {
                    self.answering = true;
                } else {
                    underrun = true;
                    self.answering = repeat_last && self.queue.has_current();
                }
            }
            Mode::Registers {
                addr_bytes,
                rw_mask,
                ..
            } => {
                self.addr_acc = 0;
                self.read_started = false;
                self.is_read = rw_mask == 0;
                self.addr_known = addr_bytes == 0;
                if addr_bytes == 0 {
                    self.addr = 0;
                    self.write_ptr = 0;
                    addr = Some(0);
                }
            }
            Mode::Off => {}
        }
        if let Some(r) = self.events.open() {
            r.start = 0;
            r.end = 0;
            r.overrun = false;
            r.underrun = underrun;
            r.addr = addr;
            r.mosi_count = 0;
            r.miso_count = 0;
        }
    }
}
