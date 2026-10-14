//! [SimMedium]: a medium in RAM that can lose power at any write, for tests.

use super::LogMedium;
use std::vec::Vec;

/// A [LogMedium] in RAM, for tests: `W`-byte writes, `E`-byte erases, NOR-like (`NOR = true`, writes need erased
/// bytes, a cut write or erase leaves random bits) or SD-like (writes overwrite, a cut unit write happens or not).
///
/// [cut_power_at](Self::cut_power_at) makes the n-th write or erase unit from now the one the power fails in: it is
/// torn, and every later call fails with [SimError::PowerCut]. [image](Self::image) and
/// [from_image](Self::from_image) carry the content over a "reboot".
#[derive(Debug, Clone)]
pub struct SimMedium<const W: usize, const E: usize, const NOR: bool> {
    data: Vec<u8>,
    ops: u64,
    cut_at: Option<u64>,
    dead: bool,
    rng: u64,
}

/// SD card in RAM: 512-byte blocks.
pub type SimSd = SimMedium<512, 512, false>;
/// NOR flash in RAM: 4-byte writes, 256-byte erase sectors.
pub type SimNor = SimMedium<4, 256, true>;

/// Errors of a [SimMedium].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimError {
    /// The power is off.
    PowerCut,
    /// Out of bounds or not aligned.
    Bounds,
    /// A NOR write to bytes that aren't erased: a bug in the store.
    NotErased,
}

impl<const W: usize, const E: usize, const NOR: bool> SimMedium<W, E, NOR> {
    /// `size` bytes, erased (NOR) or filled with random leftovers (SD: an SD card is never blank).
    pub fn new(size: usize, seed: u64) -> Self {
        let mut sim = SimMedium {
            data: Vec::new(),
            ops: 0,
            cut_at: None,
            dead: false,
            rng: seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1,
        };
        sim.data = if NOR {
            std::vec![0xff; size]
        } else {
            (0..size).map(|_| sim.random() as u8).collect()
        };
        sim
    }

    /// Medium with `image` as its content and the power on.
    pub fn from_image(image: Vec<u8>, seed: u64) -> Self {
        SimMedium {
            data: image,
            ops: 0,
            cut_at: None,
            dead: false,
            rng: seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1,
        }
    }

    /// Content as it is now.
    pub fn image(&self) -> Vec<u8> {
        self.data.clone()
    }

    /// Write and erase units done so far.
    pub fn ops(&self) -> u64 {
        self.ops
    }

    /// The power fails in the `n`-th write or erase unit from the start (counted by [ops](Self::ops)).
    pub fn cut_power_at(&mut self, n: u64) {
        self.cut_at = Some(n);
    }

    /// The power has failed.
    pub fn is_dead(&self) -> bool {
        self.dead
    }

    fn random(&mut self) -> u64 {
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        self.rng.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// One more unit; `true` when the power fails in it.
    fn tick(&mut self) -> Result<bool, SimError> {
        if self.dead {
            return Err(SimError::PowerCut);
        }
        let cut = self.cut_at == Some(self.ops);
        self.ops += 1;
        Ok(cut)
    }

    fn check(&self, offset: u64, len: usize, align: usize) -> Result<usize, SimError> {
        let off = offset as usize;
        if !off.is_multiple_of(align) || !len.is_multiple_of(align) || off + len > self.data.len() {
            return Err(SimError::Bounds);
        }
        Ok(off)
    }
}

impl<const W: usize, const E: usize, const NOR: bool> LogMedium for SimMedium<W, E, NOR> {
    type Error = SimError;
    const WRITE_SIZE: usize = W;
    const ERASE_SIZE: usize = E;
    const NEEDS_ERASE: bool = NOR;

    fn capacity(&self) -> u64 {
        self.data.len() as u64
    }

    async fn read(&mut self, offset: u64, buf: &mut [u8]) -> Result<(), SimError> {
        if self.dead {
            return Err(SimError::PowerCut);
        }
        let off = self.check(offset, buf.len(), 1)?;
        buf.copy_from_slice(&self.data[off..off + buf.len()]);
        Ok(())
    }

    async fn write(&mut self, offset: u64, data: &[u8]) -> Result<(), SimError> {
        let off = self.check(offset, data.len(), W)?;
        for (i, unit) in data.chunks(W).enumerate() {
            let at = off + i * W;
            if NOR && self.data[at..at + W].iter().any(|&b| b != 0xff) {
                return Err(SimError::NotErased);
            }
            if self.tick()? {
                self.dead = true;
                if NOR {
                    // partly programmed: some of the bits that should go to 0 did
                    for (j, &b) in unit.iter().enumerate() {
                        let keep = self.random() as u8;
                        self.data[at + j] &= b | keep;
                    }
                } else if self.random() >> 63 == 1 {
                    self.data[at..at + W].copy_from_slice(unit);
                }
                return Err(SimError::PowerCut);
            }
            self.data[at..at + W].copy_from_slice(unit);
        }
        Ok(())
    }

    async fn erase(&mut self, from: u64, to: u64) -> Result<(), SimError> {
        let off = self.check(from, (to - from) as usize, E)?;
        for at in (off..to as usize).step_by(E) {
            if self.tick()? {
                self.dead = true;
                // partly erased: some bits went to 1
                for j in 0..E {
                    let set = self.random() as u8;
                    self.data[at + j] |= set;
                }
                return Err(SimError::PowerCut);
            }
            self.data[at..at + E].fill(0xff);
        }
        Ok(())
    }
}
