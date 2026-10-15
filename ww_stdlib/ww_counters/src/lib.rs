#![cfg_attr(not(feature = "std"), no_std)]
//! The device's [`cnt`](https://crates.io/crates/cnt) counters over WireWeaver (STD-25).
//!
//! `cnt` counts into two buffers of 32-bit words, RAM (zeroed at boot) and backup (survives a reset); a `u64` counter
//! is two words, low first. What each word is (name, group, type, unit, severity) is only in the firmware's ELF, in
//! symbol names the linker places, so the device sends **raw words** and the host decodes them with the ELF, as
//! `cnt` does over a debug probe. The ELF is found by [CountersInfo::build_id] (GNU build id in flash):
//! `ww counters` and `cnt_core` look in `target/`, `WW_ELF_PATH` and `~/.cache/ww/elf`, and refuse an ELF of other
//! firmware.
//!
//! # Serving the counters
//!
//! The firmware serves `INFO` from a `fn` (its used words and build id are fixed at link time,
//! `const_model = "INFO=fn"`) and the methods with the helpers here, over `cnt`'s buffers, with a scratch of
//! [MAX_WORDS] words in the server struct (`scratch: [UVlq32; MAX_WORDS]`, 256 bytes). `watch` is optional and left
//! out here (`omit = ["counters.watch"]`):
//!
//! ```ignore
//! // `counters: ww_counters::Counters` in the API root, ww_codegen!(.., const_model = "INFO=fn")
//! impl api_impl::DeviceConsts for Server {
//!     fn counters_info(&self) -> CountersInfo<'_> {
//!         CountersInfo {
//!             format: cnt::SIGNATURE_VERSION,
//!             ram: BufferInfo::new(cnt::ram_used_words(), cnt::counters_ram_buffer()),
//!             backup: None, // or Some(BufferInfo::new(cnt::bkp_used_words(), cnt::counters_bkp_buffer()))
//!             build_id: MaxLen::new_unchecked(RefVec::new_bytes(ww_firmware_info::build_id())),
//!         }
//!     }
//! }
//!
//! fn counters_read(
//!     &mut self,
//!     _cx: &mut Context<'_, impl BlockingEventOut>,
//!     buffer: CounterBuffer,
//!     offset: u16,
//! ) -> RpcResult<Result<MaxLen<RefVec<'_, UVlq32>, MAX_WORDS>, CountersError<'_>>> {
//!     RpcResult::Ready(match buffer {
//!         CounterBuffer::Ram => {
//!             read_words(cnt::counters_ram_buffer(), cnt::ram_used_words(), offset, &mut self.scratch)
//!         }
//!         CounterBuffer::Backup => Err(CountersError::unsupported()),
//!     })
//! }
//!
//! fn counters_reset(&mut self, _cx: .., buffer: CounterBuffer) -> RpcResult<Result<(), CountersError<'_>>> {
//!     match buffer {
//!         CounterBuffer::Ram => reset_words(cnt::counters_ram_buffer()),
//!         CounterBuffer::Backup => return RpcResult::Ready(Err(CountersError::unsupported())),
//!     }
//!     RpcResult::Ready(Ok(()))
//! }
//! ```
//!
//! See `tests/counters` for a complete server.

#[cfg(feature = "host")]
pub mod host;

use core::sync::atomic::{AtomicU32, Ordering};
use wire_weaver::prelude::*;
use ww_error::CommonError;
pub use ww_si::Second;

/// Most words one [Counters::read] returns.
pub const MAX_WORDS: usize = 64;
/// Longest build id, in bytes (20 with `--build-id=sha1`).
pub const BUILD_ID_MAX_LEN: usize = 32;

/// The device's `cnt` counters: raw buffer words, decoded on the host with the firmware's ELF (`cnt_core`).
///
/// # Errors
///
/// [CountersError]: `Common(Unsupported)` for the backup buffer on a device without one, `Common(InvalidArgument)`
/// for an offset past the used words.
#[ww_trait]
pub trait Counters {
    /// Buffers, their size and use, the `cnt` format version and the build id: all fixed at link time.
    const INFO: CountersInfo<'i>;

    /// Words of a buffer from word `offset`, up to [MAX_WORDS] and never past its used words: empty at the end. The
    /// words of one call are copied at once, so a page is one snapshot (a `u64` counter can still tear between two
    /// calls, or within one if an interrupt carries into its high word; the host reads such a counter again).
    #[errors(Common(InvalidArgument, Unsupported))]
    fn read(
        buffer: CounterBuffer,
        offset: u16,
    ) -> Result<MaxLen<RefVec<'i, UVlq32>, MAX_WORDS>, CountersError<'i>>;

    /// Zero a buffer.
    #[errors(Common(Unsupported))]
    fn reset(buffer: CounterBuffer) -> Result<(), CountersError<'i>>;

    /// Words that changed since the last event, sent every `period` (at least the device's minimum, which opening
    /// answers): for slow links where polling [Counters::read] costs too much. A counted stream, unit: event.
    ///
    /// Optional: a device leaves it out (`omit = ["counters.watch"]`), polling `read` works everywhere. Opening
    /// fails with `Common(Unsupported)` or `Common(Busy)`.
    #[optional]
    #[stream(CounterChanges<'i>, counted)]
    fn watch(period: Second) -> Result<Second, CountersError<'i>>;
}

/// One of the two `cnt` buffers.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = u1,
    sized,
    no_future_version
)]
pub enum CounterBuffer {
    /// `_CNT_RAM_BUFFER`, zeroed at boot.
    Ram,
    /// `_CNT_BKP_BUFFER`, in memory that survives a reset (backup SRAM).
    Backup,
}

/// The counter buffers of this firmware and how to decode them, see [Counters::INFO].
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Clone, Debug, PartialEq, Eq)
)]
pub struct CountersInfo<'i> {
    /// `cnt` format version (`cnt::SIGNATURE_VERSION`, in `_CNT_SIGNATURE`; 2 today): how the host reads the words.
    pub format: u16,
    /// The RAM buffer.
    pub ram: BufferInfo,
    /// The backup buffer, `None` when the firmware has none (`CNT_BKP_BUFFER_SIZE_WORDS` is 0).
    pub backup: Option<BufferInfo>,
    /// GNU build id of the firmware (20 bytes with sha1), to find its ELF; empty when the firmware didn't keep the
    /// note in flash (the host then can't check its ELF). The same as `ww_firmware_info::FirmwareInfo::BUILD_ID`.
    pub build_id: MaxLen<RefVec<'i, u8>, BUILD_ID_MAX_LEN>,
}

/// Size and use of one counter buffer, in 32-bit words.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct BufferInfo {
    /// Words taken by counters (`cnt::ram_used_words()`), fixed at link time: what a host reads.
    pub used_words: u16,
    /// Size of the buffer.
    pub capacity_words: u16,
}

/// Counter words that changed, see [Counters::watch].
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug))]
pub struct CounterChanges<'i> {
    pub buffer: CounterBuffer,
    pub words: MaxLen<RefVec<'i, WordChange>, MAX_WORDS>,
}

/// One word's new value.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    final_structure
)]
pub struct WordChange {
    /// Word index in the buffer.
    pub index: UNib32,
    pub value: UVlq32,
}

/// Errors of the counters calls.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Clone, Debug, PartialEq, Eq),
    display,
    ww_repr = unib32
)]
pub enum CountersError<'i> {
    /// Common error
    #[display("{0}")]
    #[common]
    Common(CommonError<'i>),
}

impl CountersError<'_> {
    /// `Common(Unsupported)`: e.g. the backup buffer of a device without one.
    pub const fn unsupported() -> Self {
        CountersError::Common(CommonError::Unsupported)
    }
}

impl BufferInfo {
    /// A buffer of `cnt` (`cnt::counters_ram_buffer()`) with `used` words in use (`cnt::ram_used_words()`).
    pub fn new(used: usize, buffer: &[AtomicU32]) -> Self {
        BufferInfo {
            used_words: used.min(u16::MAX as usize) as u16,
            capacity_words: buffer.len().min(u16::MAX as usize) as u16,
        }
    }
}

/// The answer of [Counters::read] from a `cnt` buffer: copies its words from `offset` (at most [MAX_WORDS], not past
/// `used`) into `scratch` at once and returns them. `InvalidArgument` for an offset past `used`.
pub fn read_words<'s>(
    buffer: &[AtomicU32],
    used: usize,
    offset: u16,
    scratch: &'s mut [UVlq32; MAX_WORDS],
) -> Result<MaxLen<RefVec<'s, UVlq32>, MAX_WORDS>, CountersError<'static>> {
    let used = used.min(buffer.len());
    let offset = offset as usize;
    if offset > used {
        return Err(CountersError::Common(CommonError::InvalidArgument {
            arg: 1,
            reason: MaxLen::new_unchecked("offset past the used words"),
        }));
    }
    let n = (used - offset).min(MAX_WORDS);
    for (dst, src) in scratch.iter_mut().zip(&buffer[offset..offset + n]) {
        *dst = UVlq32(src.load(Ordering::Relaxed));
    }
    Ok(MaxLen::new_unchecked(RefVec::Slice {
        slice: &scratch[..n],
    }))
}

/// [Counters::reset] of a `cnt` buffer: all its words to 0. An increment running at the same time (an interrupt) may
/// survive it.
pub fn reset_words(buffer: &[AtomicU32]) {
    for word in buffer {
        word.store(0, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buffer(words: &[u32]) -> Vec<AtomicU32> {
        words.iter().map(|w| AtomicU32::new(*w)).collect()
    }

    #[test]
    fn read_pages_up_to_used() {
        let words: Vec<u32> = (0..100).collect();
        let buf = buffer(&words);
        let mut scratch = [UVlq32(0); MAX_WORDS];
        let page = read_words(&buf, 70, 0, &mut scratch).unwrap();
        assert_eq!(page.len(), 64);
        let page = read_words(&buf, 70, 64, &mut scratch).unwrap();
        let got: Vec<u32> = page.iter().map(|w| w.unwrap().0).collect();
        assert_eq!(got, [64, 65, 66, 67, 68, 69]);
        assert_eq!(read_words(&buf, 70, 70, &mut scratch).unwrap().len(), 0);
        assert!(read_words(&buf, 70, 71, &mut scratch).is_err());
    }

    #[test]
    fn small_counters_take_a_byte() {
        let buf = buffer(&[0, 1, 5, 0, 300]);
        let mut scratch = [UVlq32(0); MAX_WORDS];
        let page = read_words(&buf, 5, 0, &mut scratch).unwrap();
        let mut bytes = [0u8; 32];
        let ser = page.to_ww_bytes(&mut bytes).unwrap();
        // length + 4 one-byte words + one two-byte word
        assert!(ser.len() <= 8, "{ser:?}");
    }

    #[test]
    fn reset_zeroes() {
        let buf = buffer(&[1, 2, 3]);
        reset_words(&buf);
        assert!(buf.iter().all(|w| w.load(Ordering::Relaxed) == 0));
    }
}
