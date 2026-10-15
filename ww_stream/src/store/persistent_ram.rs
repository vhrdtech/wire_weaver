//! [PersistentRamRing]: a [RamRing](super::RamRing) in RAM that survives a soft reset.
//!
//! The ring lives in a [RamArea], placed by the firmware in RAM that its startup code leaves alone (an `.uninit`
//! section): a fault handler that resets the chip (`SCB::sys_reset`) leaves the area as it was, and the next boot
//! takes the events up again. A power cut loses it, and RAM after a power-on holds garbage, which is never taken for
//! events.

use super::ram_ring::{Check, MAX_HEAD, RECORD_OVERHEAD, Ring, write};
use super::{FullPolicy, PushError, RamRingError, StoreState, StoredEvent, StreamStore};
use crate::StreamHead;
use core::mem::MaybeUninit;
use crc::{CRC_32_ISCSI, Crc};
use wire_weaver::shrink_wrap::{DeserializeShrinkWrap, UVlq32};

const CRC: Crc<u32> = Crc::<u32>::new(&CRC_32_ISCSI);
const MAGIC: u32 = u32::from_le_bytes(*b"WWPR");
const VERSION: u8 = 1;
/// Bytes of one header slot.
pub const SLOT_LEN: usize = 64;

/// CRC-32C of each record.
pub(super) struct Crc32c;

impl Check for Crc32c {
    const LEN: usize = 4;
    fn sum(parts: &[&[u8]]) -> u32 {
        let mut digest = CRC.digest();
        for part in parts {
            digest.update(part);
        }
        digest.finalize()
    }
}

/// Bytes of a [PersistentRamRing] record's own header: [RECORD_OVERHEAD] and a CRC (u32).
pub const PERSISTENT_RECORD_OVERHEAD: usize = RECORD_OVERHEAD + 4;

/// The RAM a [PersistentRamRing] keeps its events in: two header slots and `N` bytes of ring. Only bytes, so
/// whatever the RAM holds is a valid `RamArea`; the store checks it before trusting it.
///
/// Place it where the startup code doesn't zero or copy it, e.g. with `cortex-m-rt`:
///
/// ```ignore
/// #[unsafe(link_section = ".uninit.ww_logs")]
/// static mut LOG_AREA: MaybeUninit<RamArea<4096>> = MaybeUninit::uninit();
///
/// // once, at boot
/// let area = unsafe { RamArea::from_uninit(&mut *core::ptr::addr_of_mut!(LOG_AREA)) };
/// let store = PersistentRamRing::attach(area);
/// ```
#[repr(C)]
#[derive(Clone)]
pub struct RamArea<const N: usize> {
    slots: [[u8; SLOT_LEN]; 2],
    buf: [u8; N],
}

impl<const N: usize> Default for RamArea<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> RamArea<N> {
    /// Bytes of the area: two slots and the ring.
    pub const SIZE: usize = 2 * SLOT_LEN + N;

    /// An area of zeros: [PersistentRamRing::attach] starts a new, empty ring in it.
    pub const fn new() -> Self {
        RamArea {
            slots: [[0; SLOT_LEN]; 2],
            buf: [0; N],
        }
    }

    /// The area in RAM the startup code left alone, with whatever it holds: from before a soft reset, or garbage
    /// after a power-on.
    ///
    /// # Safety
    ///
    /// `area` is RAM that holds a value of some kind, as an `.uninit` section on a microcontroller does (its cells
    /// hold what they held before, or random bits after a power-on), not memory the compiler knows nothing was ever
    /// written to. Every bit pattern is a valid `RamArea`, the store checks the content before using it. Call it
    /// once per area, before anything else touches it.
    pub unsafe fn from_uninit(area: &mut MaybeUninit<Self>) -> &mut Self {
        // hide where the pointer comes from, so the compiler can't reason about what the area holds
        let ptr = core::hint::black_box(area.as_mut_ptr());
        // SAFETY: the caller guarantees the memory holds bits; RamArea is plain bytes, any of them are valid
        unsafe { &mut *ptr }
    }

    /// The area as bytes ([Self::SIZE]), to carry its image over a simulated reset in tests.
    pub fn image(&self) -> &[u8] {
        // SAFETY: repr(C) of byte arrays only: no padding, every byte initialized
        unsafe { core::slice::from_raw_parts((self as *const Self).cast::<u8>(), Self::SIZE) }
    }

    /// Overwrite the area with `image` (as from [Self::image]: what the RAM held across a reset, or garbage).
    ///
    /// # Panics
    ///
    /// When `image` isn't [Self::SIZE] bytes long.
    pub fn load_image(&mut self, image: &[u8]) {
        assert_eq!(image.len(), Self::SIZE, "RamArea image of a different size");
        let (slots, buf) = image.split_at(2 * SLOT_LEN);
        self.slots[0].copy_from_slice(&slots[..SLOT_LEN]);
        self.slots[1].copy_from_slice(&slots[SLOT_LEN..]);
        self.buf.copy_from_slice(buf);
    }
}

/// What [PersistentRamRing::attach] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RamRecovery {
    /// No ring was in the area (a power-on, garbage, another layout or size): a new, empty one was started, session
    /// 0, clock epoch 0.
    pub formatted: bool,
    /// Bytes of records cut off at boot because they didn't check out: the area was written over before the reset.
    /// Their events are gone, the stream continues after them.
    pub truncated_bytes: u32,
    /// Clock epoch of this boot: one more than the boot before, 0 for a new ring.
    pub clock_epoch: u32,
}

/// Header slot, decoded.
struct Slot {
    write_seq: u64,
    session: u32,
    epoch: u32,
    start: usize,
    used: usize,
    next: u64,
    dropped_units: u64,
}

impl Slot {
    fn encode<const N: usize>(&self) -> [u8; SLOT_LEN] {
        let mut b = [0u8; SLOT_LEN];
        b[0..4].copy_from_slice(&MAGIC.to_le_bytes());
        b[4] = VERSION;
        b[8..12].copy_from_slice(&(N as u32).to_le_bytes());
        b[12..16].copy_from_slice(&self.session.to_le_bytes());
        b[16..24].copy_from_slice(&self.write_seq.to_le_bytes());
        b[24..28].copy_from_slice(&self.epoch.to_le_bytes());
        b[28..32].copy_from_slice(&(self.start as u32).to_le_bytes());
        b[32..36].copy_from_slice(&(self.used as u32).to_le_bytes());
        b[40..48].copy_from_slice(&self.next.to_le_bytes());
        b[48..56].copy_from_slice(&self.dropped_units.to_le_bytes());
        let crc = CRC.checksum(&b[..SLOT_LEN - 4]);
        b[SLOT_LEN - 4..].copy_from_slice(&crc.to_le_bytes());
        b
    }

    fn decode<const N: usize>(b: &[u8; SLOT_LEN]) -> Option<Slot> {
        let u32_at = |i: usize| u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
        let u64_at = |i: usize| u64::from_le_bytes(b[i..i + 8].try_into().unwrap());
        if u32_at(0) != MAGIC
            || b[4] != VERSION
            || u32_at(8) as usize != N
            || CRC.checksum(&b[..SLOT_LEN - 4]) != u32_at(SLOT_LEN - 4)
        {
            return None;
        }
        let slot = Slot {
            write_seq: u64_at(16),
            session: u32_at(12),
            epoch: u32_at(24),
            start: u32_at(28) as usize,
            used: u32_at(32) as usize,
            next: u64_at(40),
            dropped_units: u64_at(48),
        };
        (slot.start < N.max(1) && slot.used <= N).then_some(slot)
    }
}

/// Events of one reliable stream in a [RamArea] that survives a soft reset.
///
/// The firmware places the area in RAM its startup code leaves alone (an `.uninit` section, see [RamArea]): a
/// fault handler that resets the chip (`SCB::sys_reset`) leaves it as it was and the next boot takes the events up
/// again. A power cut loses it, and garbage in RAM after a power-on is never taken for events.
///
/// Works like a [RamRing](super::RamRing) and keeps, across a reset, what a [LogStore](super::LogStore) keeps
/// across a reboot: session, positions, the released position, dropped units and the clock epoch. Each boot starts
/// a new clock epoch, the first event pushed after it carries it in `ext.clock_epoch` (unless the sender set it).
/// Call [Reliable::continue_stored_session](crate::tx::Reliable::continue_stored_session) after
/// [attach](Self::attach) to go on with the stored session.
///
/// Each event takes its item, its encoded head and [PERSISTENT_RECORD_OVERHEAD] bytes; every change writes a 64-byte
/// header slot as well.
///
/// Layout of a [RamArea], numbers little endian, CRC-32C:
///
/// ```text
/// slot A (64 bytes) | slot B (64 bytes) | ring (N bytes)
/// ```
///
/// - **Slots**: the ring's header, written to A and B in turn after every change: magic, layout version, `N`,
///   `write_seq`, session, clock epoch, offset and length of the kept records, next position, dropped units, CRC.
///   The good slot with the higher `write_seq` wins, so a reset in the middle of writing one leaves the other.
/// - **Records**: as in a [RamRing](super::RamRing) (length, `pos`, `end`, head length), plus a CRC of the record.
///
/// A push writes its record into free room first and then the slot that takes it in; when it makes room by dropping
/// the oldest events, a slot that forgets them is written before the new record goes over them. So a reset at any
/// point of a push, release or reset leaves the state before it or the one after it (or, for such a push, the
/// oldest events dropped and the new one not in), never a mix. At boot every record is
/// checked as well, and the ring is cut before the first bad one ([RamRecovery::truncated_bytes]): something that
/// went wild before the reset may have written over the area.
pub struct PersistentRamRing<'a, const N: usize> {
    area: &'a mut RamArea<N>,
    ring: Ring<Crc32c>,
    write_seq: u64,
    epoch: u32,
    epoch_pending: bool,
    recovery: RamRecovery,
}

impl<'a, const N: usize> PersistentRamRing<'a, N> {
    /// Take up the ring kept in `area` (after a soft reset), or start a new, empty one when it holds none (after a
    /// power-on). Starts the next clock epoch either way, see [Self::recovery]. Drops the newest event when full.
    pub fn attach(area: &'a mut RamArea<N>) -> Self {
        const { assert!(N <= u32::MAX as usize, "RamArea too big") };
        let found = [
            Slot::decode::<N>(&area.slots[0]),
            Slot::decode::<N>(&area.slots[1]),
        ]
        .into_iter()
        .flatten()
        .max_by_key(|s| s.write_seq);
        let mut store = PersistentRamRing {
            area,
            ring: Ring::new(),
            write_seq: 0,
            epoch: 0,
            epoch_pending: false,
            recovery: RamRecovery::default(),
        };
        match found {
            None => store.recovery.formatted = true,
            Some(slot) => {
                store.write_seq = slot.write_seq;
                store.epoch = slot.epoch.wrapping_add(1);
                store.epoch_pending = true;
                store.ring.session = slot.session;
                store.ring.start = slot.start;
                store.ring.used = slot.used;
                store.ring.next = slot.next;
                store.ring.dropped_units = slot.dropped_units;
                store.recovery.truncated_bytes = store.check_records() as u32;
            }
        }
        store.recovery.clock_epoch = store.epoch;
        store.commit();
        store
    }

    /// What to do when an event doesn't fit, see [FullPolicy].
    pub fn with_policy(mut self, policy: FullPolicy) -> Self {
        self.ring.policy = policy;
        self
    }

    /// What [Self::attach] found.
    pub fn recovery(&self) -> RamRecovery {
        self.recovery
    }

    /// Clock epoch of this boot: one more than the boot before.
    pub fn clock_epoch(&self) -> u32 {
        self.epoch
    }

    /// Events kept.
    pub fn is_empty(&self) -> bool {
        self.ring.used == 0
    }

    /// Walk the records the slot says are kept, keep them up to the first bad one; bytes cut off.
    fn check_records(&mut self) -> usize {
        type R = Ring<Crc32c>;
        let buf = &self.area.buf;
        let ring = &mut self.ring;
        let overhead = R::overhead();
        let mut off = ring.start;
        let mut walked = 0;
        let mut prev_end = None;
        while walked < ring.used {
            let rest = ring.used - walked;
            if rest < overhead {
                break;
            }
            let (rec, raw) = R::record_raw(buf, off);
            if rec.head_len > MAX_HEAD
                || rec.len < overhead + rec.head_len
                || rec.len > rest
                || rec.end < rec.pos
                || rec.end > ring.next
                || prev_end.is_some_and(|end| rec.pos < end)
            {
                break;
            }
            let body_off = (off + overhead) % N;
            let (a, b) = R::region(buf, body_off, rec.len - overhead);
            if Crc32c::sum(&[&raw, a, b]) != rec.check {
                break;
            }
            let mut head_buf = [0u8; MAX_HEAD];
            R::copy_out(buf, body_off, &mut head_buf[..rec.head_len]);
            match StreamHead::from_ww_bytes(&head_buf[..rec.head_len]) {
                Ok(head) if head.pos == rec.pos && head.end() == rec.end => {}
                _ => break,
            }
            ring.oldest.get_or_insert(rec.pos);
            ring.newest = Some(rec.pos);
            ring.newest_off = off;
            prev_end = Some(rec.end);
            walked += rec.len;
            off = (off + rec.len) % N;
        }
        let cut = ring.used - walked;
        ring.used = walked;
        if walked == 0 {
            ring.start = 0;
        }
        cut
    }

    fn commit(&mut self) {
        commit::<N>(
            &mut self.area.slots,
            &mut self.write_seq,
            &self.ring,
            self.epoch,
        );
    }
}

/// Write the ring's header into the slot not written last.
fn commit<const N: usize>(
    slots: &mut [[u8; SLOT_LEN]; 2],
    write_seq: &mut u64,
    ring: &Ring<Crc32c>,
    epoch: u32,
) {
    *write_seq += 1;
    let slot = Slot {
        write_seq: *write_seq,
        session: ring.session,
        epoch,
        start: ring.start,
        used: ring.used,
        next: ring.next,
        dropped_units: ring.dropped_units,
    }
    .encode::<N>();
    write(&mut slots[(*write_seq % 2) as usize], &slot);
}

impl<const N: usize> StreamStore for PersistentRamRing<'_, N> {
    type Error = RamRingError;

    async fn push(
        &mut self,
        head: &StreamHead,
        item: &[u8],
    ) -> Result<(), PushError<RamRingError>> {
        let mut head = *head;
        if self.epoch_pending && head.ext.is_none_or(|e| e.clock_epoch.is_none()) {
            let mut ext = head.ext.unwrap_or_default();
            ext.clock_epoch = Some(UVlq32(self.epoch));
            head.ext = Some(ext);
        }
        let slots = &mut self.area.slots;
        let write_seq = &mut self.write_seq;
        let epoch = self.epoch;
        self.ring.push(&mut self.area.buf, &head, item, |ring, _| {
            commit::<N>(slots, write_seq, ring, epoch)
        })?;
        self.commit();
        self.epoch_pending = false;
        Ok(())
    }

    async fn read<'b>(
        &mut self,
        pos: u64,
        buf: &'b mut [u8],
    ) -> Result<Option<StoredEvent<'b>>, RamRingError> {
        self.ring.read(&self.area.buf, pos, buf)
    }

    async fn release(&mut self, pos: u64) -> Result<(), RamRingError> {
        if self.ring.release(&self.area.buf, pos) {
            self.commit();
        }
        Ok(())
    }

    fn state(&self) -> StoreState {
        self.ring.state(N, true)
    }

    async fn reset(&mut self, session: u32) -> Result<(), RamRingError> {
        self.ring.reset(session);
        self.commit();
        Ok(())
    }
}
