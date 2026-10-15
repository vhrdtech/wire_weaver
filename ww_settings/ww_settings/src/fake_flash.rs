//! [FakeFlash]: NOR flash in RAM for tests, with power cuts at any word write or erased byte, ECC words, and a
//! hard error on a second write to a word.
//!
//! A power cut is armed with [cut_after](FakeFlash::cut_after)`(steps)`: one step is one word written or one byte
//! erased. The step that runs out is left half done, every operation after it fails with
//! [FakeError::PowerCut] until [power_on](FakeFlash::power_on), which keeps the contents, as a reboot does.
//!
//! - A cut word write clears a random part of the bits it was going to clear (programming only turns 1s into 0s).
//! - A cut erase leaves the bytes before the cut erased, the byte at the cut a random mix of old bits and 1s, and
//!   the rest untouched, in the [EraseOrder] chosen (real flash doesn't promise any order).
//! - With [ecc](FakeFlash::ecc) on (STM32G0: 8-byte words, STM32H7: 32-byte words), a word a cut left half
//!   written or half erased reads as all zeros: what an ECC-aware flash driver returns for a word with an
//!   uncorrectable error, instead of a fault (see the docs page). Off: the half-done bits read as they are.
//! - A write to a word that isn't erased, written or not, is [FakeError::NotErased]: the STM32 flash controllers
//!   refuse it (G0 `PROGERR`), and on ECC flash a second write corrupts the word.

use embedded_storage_async::nor_flash::{
    ErrorType, NorFlash, NorFlashError, NorFlashErrorKind, ReadNorFlash,
};

/// Order in which an erase clears a page's bytes, which matters only for a cut erase.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EraseOrder {
    /// First byte first.
    Forward,
    /// Last byte first.
    Backward,
}

/// Errors of [FakeFlash].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FakeError {
    /// The power is off since a cut.
    PowerCut,
    /// A word was written that isn't erased (written before, or left half done by a cut).
    NotErased(u32),
    /// Address or length not aligned to the word / page.
    NotAligned,
    /// Outside the flash.
    OutOfBounds,
}

impl NorFlashError for FakeError {
    fn kind(&self) -> NorFlashErrorKind {
        match self {
            FakeError::NotAligned => NorFlashErrorKind::NotAligned,
            FakeError::OutOfBounds => NorFlashErrorKind::OutOfBounds,
            _ => NorFlashErrorKind::Other,
        }
    }
}

/// What a [FakeFlash] did.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub word_writes: u64,
    pub page_erases: u64,
    /// Steps taken: words written and bytes erased.
    pub steps: u64,
}

/// NOR flash in RAM: `WORD` bytes written at once (the write and ECC unit), `PAGE` bytes erased at once,
/// addresses from `base`.
#[derive(Clone)]
pub struct FakeFlash<const WORD: usize, const PAGE: usize> {
    base: u32,
    data: Vec<u8>,
    /// per word: written since its erase, or left half done (can't be written)
    used: Vec<bool>,
    /// per word: ECC broken by a cut
    broken: Vec<bool>,
    /// Broken words read as zeros (ECC flash with an ECC-aware driver).
    pub ecc: bool,
    pub erase_order: EraseOrder,
    budget: Option<u64>,
    off: bool,
    stats: Stats,
    rng: u64,
}

impl<const WORD: usize, const PAGE: usize> FakeFlash<WORD, PAGE> {
    /// `pages` erased pages at `base`.
    pub fn new(base: u32, pages: usize) -> Self {
        assert!(PAGE.is_multiple_of(WORD));
        let len = pages * PAGE;
        FakeFlash {
            base,
            data: vec![0xff; len],
            used: vec![false; len / WORD],
            broken: vec![false; len / WORD],
            ecc: WORD >= 8,
            erase_order: EraseOrder::Forward,
            budget: None,
            off: false,
            stats: Stats::default(),
            rng: 0x2545_f491_4f6c_dd1d,
        }
    }

    /// Cut the power once `steps` more steps (words written, bytes erased) are taken: the last one is left half
    /// done.
    pub fn cut_after(&mut self, steps: u64) {
        self.budget = Some(steps);
    }

    /// Seed for the bits a cut leaves.
    pub fn seed(&mut self, seed: u64) {
        self.rng = seed.max(1);
    }

    /// The power was cut.
    pub fn is_off(&self) -> bool {
        self.off
    }

    /// Power back on with the contents as they are: a reboot.
    pub fn power_on(&mut self) {
        self.off = false;
        self.budget = None;
    }

    pub fn stats(&self) -> Stats {
        self.stats
    }

    /// The raw contents, as a dump of the flash would show them.
    pub fn image(&self) -> &[u8] {
        &self.data
    }

    /// A flash with these contents (from [image](Self::image) or a dump), nothing half done.
    pub fn from_image(base: u32, image: &[u8]) -> Self {
        let mut f = Self::new(base, image.len() / PAGE);
        f.data.copy_from_slice(image);
        for (w, chunk) in image.chunks(WORD).enumerate() {
            f.used[w] = chunk.iter().any(|b| *b != 0xff);
        }
        f
    }

    fn rand(&mut self) -> u8 {
        // xorshift64
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        (x >> 24) as u8
    }

    /// Take a step: `false` when this is the one the power is cut at.
    fn step(&mut self) -> bool {
        self.stats.steps += 1;
        match self.budget.as_mut() {
            Some(0) => {
                self.off = true;
                false
            }
            Some(n) => {
                *n -= 1;
                if *n == 0 {
                    self.off = true;
                    false
                } else {
                    true
                }
            }
            None => true,
        }
    }

    fn range(&self, offset: u32, len: usize) -> Result<core::ops::Range<usize>, FakeError> {
        let start = offset
            .checked_sub(self.base)
            .ok_or(FakeError::OutOfBounds)? as usize;
        let end = start.checked_add(len).ok_or(FakeError::OutOfBounds)?;
        if end > self.data.len() {
            return Err(FakeError::OutOfBounds);
        }
        Ok(start..end)
    }
}

impl<const WORD: usize, const PAGE: usize> ErrorType for FakeFlash<WORD, PAGE> {
    type Error = FakeError;
}

impl<const WORD: usize, const PAGE: usize> ReadNorFlash for FakeFlash<WORD, PAGE> {
    const READ_SIZE: usize = 1;

    async fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Self::Error> {
        if self.off {
            return Err(FakeError::PowerCut);
        }
        let r = self.range(offset, bytes.len())?;
        for (i, b) in r.clone().zip(bytes.iter_mut()) {
            *b = if self.ecc && self.broken[i / WORD] {
                0
            } else {
                self.data[i]
            };
        }
        Ok(())
    }

    fn capacity(&self) -> usize {
        self.data.len()
    }
}

impl<const WORD: usize, const PAGE: usize> NorFlash for FakeFlash<WORD, PAGE> {
    const WRITE_SIZE: usize = WORD;
    const ERASE_SIZE: usize = PAGE;

    async fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Self::Error> {
        if self.off {
            return Err(FakeError::PowerCut);
        }
        if !(offset as usize).is_multiple_of(WORD) || !bytes.len().is_multiple_of(WORD) {
            return Err(FakeError::NotAligned);
        }
        let r = self.range(offset, bytes.len())?;
        for (k, chunk) in bytes.chunks(WORD).enumerate() {
            let start = r.start + k * WORD;
            let w = start / WORD;
            if self.used[w] || self.data[start..start + WORD].iter().any(|b| *b != 0xff) {
                return Err(FakeError::NotErased(self.base + start as u32));
            }
            self.used[w] = true;
            self.stats.word_writes += 1;
            if self.step() {
                for (i, b) in chunk.iter().enumerate() {
                    self.data[start + i] &= *b;
                }
            } else {
                // half programmed: a random part of the bits to clear is cleared
                for (i, b) in chunk.iter().enumerate() {
                    let mask = self.rand();
                    self.data[start + i] &= *b | mask;
                }
                self.broken[w] = true;
                return Err(FakeError::PowerCut);
            }
        }
        Ok(())
    }

    async fn erase(&mut self, from: u32, to: u32) -> Result<(), Self::Error> {
        if self.off {
            return Err(FakeError::PowerCut);
        }
        if !(from as usize).is_multiple_of(PAGE) || !(to as usize).is_multiple_of(PAGE) || to < from
        {
            return Err(FakeError::NotAligned);
        }
        let r = self.range(from, (to - from) as usize)?;
        for page in r.step_by(PAGE) {
            self.stats.page_erases += 1;
            for k in 0..PAGE {
                let i = match self.erase_order {
                    EraseOrder::Forward => page + k,
                    EraseOrder::Backward => page + PAGE - 1 - k,
                };
                if self.step() {
                    self.data[i] = 0xff;
                } else {
                    let mask = self.rand();
                    self.data[i] |= mask;
                    // the word with the cut byte is half erased; words done before it are clean
                    let w = i / WORD;
                    self.broken[w] = true;
                    self.used[w] = true;
                    self.mark_erased_words(page, k, i);
                    return Err(FakeError::PowerCut);
                }
            }
            self.mark_erased_words(page, PAGE, usize::MAX);
        }
        Ok(())
    }
}

impl<const WORD: usize, const PAGE: usize> FakeFlash<WORD, PAGE> {
    /// After `done` bytes of the page at `page` were erased (the byte at `cut` half): every word whose bytes are
    /// all erased is clean again.
    fn mark_erased_words(&mut self, page: usize, done: usize, cut: usize) {
        let (lo, hi) = match self.erase_order {
            EraseOrder::Forward => (page, page + done),
            EraseOrder::Backward => (page + PAGE - done, page + PAGE),
        };
        let first = lo.div_ceil(WORD);
        let last = hi / WORD;
        for w in first..last {
            if cut / WORD != w {
                self.used[w] = false;
                self.broken[w] = false;
            }
        }
    }
}
