#![cfg_attr(not(test), no_std)]
//! The `defmt` global logger of a WireWeaver device: every frame goes into a lock-free ring in RAM, and from there,
//! whole frames at a time, into the reliable `ww_log::LogDefmt::frames` stream; with the `rtt` feature
//! each frame is also teed to an RTT up channel, so `probe-rs` and `cnt run` keep showing the logs with a probe
//! attached. One global logger serves both.
//!
//! ```rust,ignore
//! // the global logger and its ring of 2 KiB
//! ww_defmt_logger::global_logger!(LOG, 2048);
//!
//! // at boot: the RTT channel named "defmt" (rtt feature), and a session so the store keeps what is logged before
//! // a host connects
//! let channels = rtt_target::rtt_init! { up: { 0: { size: 1024, name: "defmt" } 1: { .. } } down: { .. } };
//! LOG.set_rtt_channel(channels.up.0);
//! log_stream.start_session(1).await?;
//!
//! // in the event loop, and before the stream's events are sent
//! LOG.pump(&mut log_stream).await?;
//! ```
//!
//! # How a frame travels
//!
//! 1. A `defmt` macro anywhere (thread mode, any interrupt) takes a critical section in [DefmtLogger::acquire],
//!    encodes the frame (rzCOBS or raw, as `defmt` is built) and copies it into the ring and the RTT channel.
//! 2. [DefmtLogger::release] commits the whole frame to the ring at once, with its length in front: the ring never
//!    holds half a frame. A frame that doesn't fit the free space, or is longer than
//!    [MAX_FRAMES_LEN](ww_log::MAX_FRAMES_LEN), is dropped whole and counted.
//! 3. [DefmtLogger::pump], called from the event loop, takes the committed frames out of the ring without a critical
//!    section, packs up to 512 bytes of whole frames into one `DefmtFrames` item and pushes it into the stream's
//!    store, `units` = frames. Frames the ring dropped become the stream's `lost` exactly where they were dropped, so
//!    the host knows how many log messages it misses and where.
//!
//! The store then keeps the frames until the host acknowledges them, also while no host is connected: start a
//! session at boot ([Reliable::start_session]) and the logs from before the host connects arrive when it does.
//! What happens when the store is full is its `FullPolicy`; `DropOldest` keeps the latest logs.
//!
//! # RAM
//!
//! The ring (`N` bytes, a power of two; each frame takes its encoded length plus [FRAME_OVERHEAD] bytes) and about
//! 1 KiB for packing and serializing an item. The ring only has to cover the logs made between two `pump` calls;
//! the store holds the rest.
//!
//! # Contexts
//!
//! Logging works from any context; frames from an interrupt never interleave with a frame being written, as with
//! `defmt-rtt`. A `defmt` macro inside a `defmt` macro (a `Format` impl that logs) panics. `pump` may run in any
//! single context at a time; a second, concurrent `pump` returns at once without taking frames.
//!
//! `defmt::flush()` doesn't wait for anything: the ring is emptied by `pump`, never from inside a log call.

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use critical_section::RestoreState;
use wire_weaver::shrink_wrap::{MaxLen, RefVec, SerializeShrinkWrap};
use ww_log::{DefmtEncoding, DefmtFrames, MAX_FRAMES_LEN};
use ww_stream::store::StreamStore;
use ww_stream::tx::Reliable;

#[doc(hidden)]
pub use defmt;

/// Bytes a frame takes in the ring besides its own: its length (2) and the logger's count of dropped frames when it
/// was committed (4).
pub const FRAME_OVERHEAD: usize = 6;

/// Room for one serialized `DefmtFrames` with [MAX_FRAMES_LEN] bytes of frames.
const SER_LEN: usize = MAX_FRAMES_LEN + 16;

/// Makes a [DefmtLogger] with a ring of `$size` bytes the `#[defmt::global_logger]` of the firmware, as a `static`
/// named `$name` to call [DefmtLogger::pump] and [DefmtLogger::set_rtt_channel] on.
///
/// The crate using it must depend on `defmt` (the `#[defmt::global_logger]` expansion names it), and must not link
/// another global logger (`defmt-rtt`, `rtt-target`'s `defmt` feature).
#[macro_export]
macro_rules! global_logger {
    ($name:ident, $size:expr) => {
        static $name: $crate::DefmtLogger<{ $size }> = $crate::DefmtLogger::new();

        #[$crate::defmt::global_logger]
        struct WwDefmtGlobalLogger;

        unsafe impl $crate::defmt::Logger for WwDefmtGlobalLogger {
            fn acquire() {
                $name.acquire()
            }
            unsafe fn flush() {}
            unsafe fn release() {
                // safety: defmt calls it once after each acquire
                unsafe { $name.release() }
            }
            unsafe fn write(bytes: &[u8]) {
                // safety: defmt calls it only between acquire and release
                unsafe { $name.write(bytes) }
            }
        }
    };
}

/// The encoding `defmt` is built with (its `encoding-raw` / `encoding-rzcobs` cargo feature), for
/// `LogDefmt::DEFMT`. Found out by encoding a one-byte frame: raw passes it through, rzCOBS ends it with a `0x00`.
pub fn encoding() -> DefmtEncoding {
    let mut encoder = defmt::Encoder::new();
    let mut out = [0u8; 8];
    let mut len = 0;
    let mut collect = |b: &[u8]| {
        for &byte in b {
            if len < out.len() {
                out[len] = byte;
            }
            len += 1;
        }
    };
    encoder.start_frame(&mut collect);
    encoder.write(&[0x55], &mut collect);
    encoder.end_frame(&mut collect);
    if len == 1 && out[0] == 0x55 {
        DefmtEncoding::Raw
    } else {
        DefmtEncoding::Rzcobs
    }
}

/// The frame being written, owned by whoever holds the logger (critical section and `taken`).
struct Frame {
    /// Ring position of the frame's header
    start: usize,
    /// Ring position of the next byte
    cursor: usize,
    /// The consumer's position when last looked at: free space is at least `N - (cursor - read)`
    read: usize,
    /// The frame didn't fit and is dropped
    dropped: bool,
}

/// The ring, its producer (`defmt`'s `Logger` calls) and its consumer ([pump](Self::pump)), see the
/// [crate docs](crate). `N` is a power of two; usually made a `static` by [global_logger!].
pub struct DefmtLogger<const N: usize> {
    taken: AtomicBool,
    cs_restore: UnsafeCell<RestoreState>,
    encoder: UnsafeCell<defmt::Encoder>,
    frame: UnsafeCell<Frame>,
    #[cfg(feature = "rtt")]
    rtt: UnsafeCell<Option<rtt_target::UpChannel>>,

    ring: UnsafeCell<[u8; N]>,
    /// End of the committed frames, free running (wraps at `usize::MAX`); written by the producer only
    write: AtomicUsize,
    /// Start of the frames not taken yet, free running; written by the consumer only
    read: AtomicUsize,
    /// Frames dropped since boot, wrapping; written by the producer only
    dropped: AtomicU32,

    pumping: AtomicBool,
    consumer: UnsafeCell<Consumer>,
}

/// The consumer's own state, owned by whoever holds `pumping`.
struct Consumer {
    /// Dropped frames already reported to the stream
    dropped_seen: u32,
    pack: [u8; MAX_FRAMES_LEN],
    ser: [u8; SER_LEN],
}

// safety: the producer state is only touched inside a critical section with `taken` set, the consumer state only
// with `pumping` set; the ring between them is split by `write` / `read` with release / acquire ordering.
unsafe impl<const N: usize> Sync for DefmtLogger<N> {}

impl<const N: usize> Default for DefmtLogger<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> DefmtLogger<N> {
    const CHECK: () = assert!(
        N.is_power_of_two() && N >= FRAME_OVERHEAD + 16 && N <= (1 << 30),
        "DefmtLogger: N must be a power of two of at least 32 bytes"
    );
    const MASK: usize = N - 1;

    /// An empty ring.
    pub const fn new() -> Self {
        #[allow(clippy::let_unit_value)]
        let () = Self::CHECK;
        DefmtLogger {
            taken: AtomicBool::new(false),
            cs_restore: UnsafeCell::new(RestoreState::invalid()),
            encoder: UnsafeCell::new(defmt::Encoder::new()),
            frame: UnsafeCell::new(Frame {
                start: 0,
                cursor: 0,
                read: 0,
                dropped: false,
            }),
            #[cfg(feature = "rtt")]
            rtt: UnsafeCell::new(None),
            ring: UnsafeCell::new([0; N]),
            write: AtomicUsize::new(0),
            read: AtomicUsize::new(0),
            dropped: AtomicU32::new(0),
            pumping: AtomicBool::new(false),
            consumer: UnsafeCell::new(Consumer {
                dropped_seen: 0,
                pack: [0; MAX_FRAMES_LEN],
                ser: [0; SER_LEN],
            }),
        }
    }

    /// Tee every frame to this RTT up channel as well, from the next frame on. Name it `defmt` in `rtt_init!` so
    /// `probe-rs` decodes it; its mode decides whether a full channel blocks the logger (`BlockIfFull`, what
    /// `probe-rs` sets while attached) or cuts frames short.
    #[cfg(feature = "rtt")]
    pub fn set_rtt_channel(&self, channel: rtt_target::UpChannel) {
        critical_section::with(|_| {
            // safety: no log call runs inside this critical section, so nobody holds the channel
            unsafe { *self.rtt.get() = Some(channel) }
        })
    }

    /// Frames dropped since boot because the ring was full or they were too long, wrapping. Each was reported to
    /// the stream as lost by [pump](Self::pump) (or will be at the next one).
    pub fn dropped_frames(&self) -> u32 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// Bytes of committed frames in the ring, headers included: what the next [pump](Self::pump) takes.
    pub fn pending_bytes(&self) -> usize {
        self.write
            .load(Ordering::Acquire)
            .wrapping_sub(self.read.load(Ordering::Relaxed))
    }

    /// Starts a frame: `defmt::Logger::acquire`. Takes a critical section until [release](Self::release).
    ///
    /// # Panics
    ///
    /// When called again before `release` (a `defmt` macro inside a `defmt` macro).
    pub fn acquire(&self) {
        // safety: paired with the release in `release`
        let restore = unsafe { critical_section::acquire() };
        if self.taken.load(Ordering::Relaxed) {
            panic!("defmt logger taken reentrantly")
        }
        self.taken.store(true, Ordering::Relaxed);
        // safety: inside the critical section and `taken`, nobody else touches the producer state
        unsafe {
            self.cs_restore.get().write(restore);
            let start = self.write.load(Ordering::Relaxed);
            *self.frame.get() = Frame {
                start,
                cursor: start.wrapping_add(FRAME_OVERHEAD),
                read: self.read.load(Ordering::Acquire),
                dropped: false,
            };
            (*self.encoder.get()).start_frame(|b| self.stage(b));
        }
    }

    /// Bytes of the frame: `defmt::Logger::write`.
    ///
    /// # Safety
    ///
    /// Only between [acquire](Self::acquire) and [release](Self::release).
    pub unsafe fn write(&self, bytes: &[u8]) {
        // safety: the caller holds the logger
        unsafe { (*self.encoder.get()).write(bytes, |b| self.stage(b)) }
    }

    /// Ends the frame and commits it to the ring (or counts it as dropped): `defmt::Logger::release`.
    ///
    /// # Safety
    ///
    /// Only once after each [acquire](Self::acquire).
    pub unsafe fn release(&self) {
        if !self.taken.load(Ordering::Relaxed) {
            panic!("defmt release out of context")
        }
        // safety: the caller holds the logger
        unsafe {
            (*self.encoder.get()).end_frame(|b| self.stage(b));
            let frame = &*self.frame.get();
            let dropped = self.dropped.load(Ordering::Relaxed);
            if frame.dropped {
                self.dropped
                    .store(dropped.wrapping_add(1), Ordering::Release);
            } else {
                let len = frame
                    .cursor
                    .wrapping_sub(frame.start)
                    .wrapping_sub(FRAME_OVERHEAD) as u16;
                let mut header = [0u8; FRAME_OVERHEAD];
                header[..2].copy_from_slice(&len.to_le_bytes());
                header[2..].copy_from_slice(&dropped.to_le_bytes());
                self.copy_in(frame.start, &header);
                self.write.store(frame.cursor, Ordering::Release);
            }
            let restore = self.cs_restore.get().read();
            self.taken.store(false, Ordering::Relaxed);
            critical_section::release(restore);
        }
    }

    /// Encoded bytes of the current frame: into the ring if they fit, to RTT anyway.
    ///
    /// # Safety
    ///
    /// The caller holds the logger.
    unsafe fn stage(&self, bytes: &[u8]) {
        #[cfg(feature = "rtt")]
        // safety: the caller holds the logger, set_rtt_channel waits for the critical section
        if let Some(rtt) = unsafe { &mut *self.rtt.get() } {
            rtt.write(bytes);
        }
        // safety: the caller holds the logger
        let frame = unsafe { &mut *self.frame.get() };
        if frame.dropped {
            return;
        }
        let end = frame.cursor.wrapping_add(bytes.len());
        let len = end.wrapping_sub(frame.start) - FRAME_OVERHEAD;
        if len > MAX_FRAMES_LEN {
            frame.dropped = true;
            return;
        }
        if end.wrapping_sub(frame.read) > N {
            frame.read = self.read.load(Ordering::Acquire);
            if end.wrapping_sub(frame.read) > N {
                frame.dropped = true;
                return;
            }
        }
        // safety: [cursor, end) is free: the consumer doesn't read past `write`, and `end - read <= N`
        unsafe { self.copy_in(frame.cursor, bytes) };
        frame.cursor = end;
    }

    /// # Safety
    ///
    /// `[pos, pos + bytes.len())` belongs to the caller.
    unsafe fn copy_in(&self, pos: usize, bytes: &[u8]) {
        let ring = self.ring.get().cast::<u8>();
        let i = pos & Self::MASK;
        let first = bytes.len().min(N - i);
        // safety: both pieces are inside the ring and owned by the caller
        unsafe {
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), ring.add(i), first);
            core::ptr::copy_nonoverlapping(bytes[first..].as_ptr(), ring, bytes.len() - first);
        }
    }

    /// # Safety
    ///
    /// `[pos, pos + out.len())` is committed and not released yet.
    unsafe fn copy_out(&self, pos: usize, out: &mut [u8]) {
        let ring = self.ring.get().cast::<u8>().cast_const();
        let i = pos & Self::MASK;
        let first = out.len().min(N - i);
        // safety: both pieces are inside the ring and committed
        unsafe {
            core::ptr::copy_nonoverlapping(ring.add(i), out.as_mut_ptr(), first);
            core::ptr::copy_nonoverlapping(ring, out[first..].as_mut_ptr(), out.len() - first);
        }
    }

    /// Moves the committed frames from the ring into `stream`'s store, whole frames, up to
    /// [MAX_FRAMES_LEN](ww_log::MAX_FRAMES_LEN) bytes of them per item, `units` = frames. Frames the
    /// ring dropped go into the stream's `lost` ([Reliable::lost]) in their place. Returns how many frames were
    /// moved; 0 at once while another `pump` runs.
    ///
    /// Never waits for the link: call it from the event loop as often as logs come (each turn, or on a timer), and
    /// before the stream is polled for events to send.
    ///
    /// Frames leave the ring once their item is pushed: a `pump` future dropped while the store writes (an SD
    /// store) may push that item again at the next call. RAM stores never wait.
    ///
    /// # Errors
    ///
    /// The store failed; the frames of that item are counted as lost by the stream, the rest stay in the ring.
    pub async fn pump<S: StreamStore, const K: usize>(
        &self,
        stream: &mut Reliable<S, K>,
    ) -> Result<usize, S::Error> {
        let taken = critical_section::with(|_| {
            if self.pumping.load(Ordering::Relaxed) {
                false
            } else {
                self.pumping.store(true, Ordering::Relaxed);
                true
            }
        });
        if !taken {
            return Ok(0);
        }
        let _unlock = Unlock(&self.pumping);
        // safety: `pumping` is ours until `_unlock` drops
        let consumer = unsafe { &mut *self.consumer.get() };
        self.pump_locked(consumer, stream).await
    }

    async fn pump_locked<S: StreamStore, const K: usize>(
        &self,
        consumer: &mut Consumer,
        stream: &mut Reliable<S, K>,
    ) -> Result<usize, S::Error> {
        let mut moved = 0;
        let mut packed = 0;
        let mut frames = 0u64;
        let mut read = self.read.load(Ordering::Relaxed);
        loop {
            let write = self.write.load(Ordering::Acquire);
            if read == write {
                break;
            }
            let mut header = [0u8; FRAME_OVERHEAD];
            // safety: a committed frame starts at `read`
            unsafe { self.copy_out(read, &mut header) };
            let len = u16::from_le_bytes([header[0], header[1]]) as usize;
            let dropped = u32::from_le_bytes([header[2], header[3], header[4], header[5]]);
            let lost = dropped.wrapping_sub(consumer.dropped_seen);
            if lost != 0 || packed + len > MAX_FRAMES_LEN {
                self.push(consumer, stream, packed, frames, read).await?;
                packed = 0;
                frames = 0;
            }
            if lost != 0 {
                stream.lost(lost as u64);
                consumer.dropped_seen = dropped;
            }
            // safety: the frame is committed, `len <= MAX_FRAMES_LEN - packed`
            unsafe {
                self.copy_out(
                    read.wrapping_add(FRAME_OVERHEAD),
                    &mut consumer.pack[packed..packed + len],
                )
            };
            packed += len;
            frames += 1;
            moved += 1;
            read = read.wrapping_add(FRAME_OVERHEAD + len);
        }
        self.push(consumer, stream, packed, frames, read).await?;
        // frames dropped after the last committed one: reported now when nothing was committed since, else they
        // come with the next frame's header
        let dropped = self.dropped.load(Ordering::Acquire);
        if self.write.load(Ordering::Acquire) == read {
            let lost = dropped.wrapping_sub(consumer.dropped_seen);
            if lost != 0 {
                stream.lost(lost as u64);
                consumer.dropped_seen = dropped;
            }
        }
        Ok(moved)
    }

    /// Pushes the packed frames, then gives their ring space back (up to `read`), also when the store failed (the
    /// stream counted them as lost then).
    async fn push<S: StreamStore, const K: usize>(
        &self,
        consumer: &mut Consumer,
        stream: &mut Reliable<S, K>,
        packed: usize,
        frames: u64,
        read: usize,
    ) -> Result<(), S::Error> {
        if frames == 0 {
            self.read.store(read, Ordering::Release);
            return Ok(());
        }
        let item = DefmtFrames {
            bytes: MaxLen::new_unchecked(RefVec::new_bytes(&consumer.pack[..packed])),
        };
        let bytes = item
            .to_ww_bytes(&mut consumer.ser)
            .expect("SER_LEN holds MAX_FRAMES_LEN bytes of frames");
        let result = stream.push(bytes, frames, None).await;
        self.read.store(read, Ordering::Release);
        result.map(|_| ())
    }
}

/// Releases `pumping`, also when the `pump` future is dropped halfway.
struct Unlock<'a>(&'a AtomicBool);

impl Drop for Unlock<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests;
