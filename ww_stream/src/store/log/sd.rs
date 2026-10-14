//! [SdMedium]: a [LogMedium] on an `embedded-sdmmc` [BlockDevice] (SD card, MMC, a disk image).

use super::LogMedium;
use embedded_sdmmc_types::blockdevice::{Block, BlockDevice, BlockIdx};

/// A [LogMedium] on blocks `start..start + blocks` of a [BlockDevice] (an `embedded_sdmmc::SdCard`, or anything
/// else implementing the trait). Keeps the last block it read or wrote, so reading records one after the other
/// costs one block read per 512 bytes.
///
/// The `BlockDevice` trait is blocking: each call waits for the card. The log assumes a block write either happens
/// or doesn't when the power fails, as SD cards do for single blocks.
pub struct SdMedium<D: BlockDevice> {
    dev: D,
    start: u32,
    blocks: u32,
    cache: Block,
    cached: Option<u32>,
}

impl<D: BlockDevice> SdMedium<D> {
    /// The log takes blocks `start..start + blocks` of `dev`: a partition, or the whole card (`0` and
    /// `dev.num_blocks()`).
    pub fn new(dev: D, start: u32, blocks: u32) -> Self {
        SdMedium {
            dev,
            start,
            blocks,
            cache: Block::new(),
            cached: None,
        }
    }

    /// The block device, to hand it back.
    pub fn into_inner(self) -> D {
        self.dev
    }
}

impl<D: BlockDevice> LogMedium for SdMedium<D> {
    type Error = D::Error;
    const WRITE_SIZE: usize = Block::LEN;
    const ERASE_SIZE: usize = Block::LEN;
    const NEEDS_ERASE: bool = false;

    fn capacity(&self) -> u64 {
        self.blocks as u64 * Block::LEN as u64
    }

    async fn read(&mut self, offset: u64, buf: &mut [u8]) -> Result<(), D::Error> {
        let mut done = 0;
        while done < buf.len() {
            let at = offset + done as u64;
            let idx = (at / Block::LEN as u64) as u32;
            let in_block = (at % Block::LEN as u64) as usize;
            if self.cached != Some(idx) {
                self.cached = None;
                self.dev.read(
                    core::slice::from_mut(&mut self.cache),
                    BlockIdx(self.start + idx),
                )?;
                self.cached = Some(idx);
            }
            let n = (buf.len() - done).min(Block::LEN - in_block);
            buf[done..done + n].copy_from_slice(&self.cache.contents[in_block..in_block + n]);
            done += n;
        }
        Ok(())
    }

    async fn write(&mut self, offset: u64, data: &[u8]) -> Result<(), D::Error> {
        let first = (offset / Block::LEN as u64) as u32;
        for (i, chunk) in data.chunks(Block::LEN).enumerate() {
            let idx = first + i as u32;
            self.cached = None;
            self.cache.contents.copy_from_slice(chunk);
            self.dev.write(
                core::slice::from_ref(&self.cache),
                BlockIdx(self.start + idx),
            )?;
            self.cached = Some(idx);
        }
        Ok(())
    }

    async fn erase(&mut self, _from: u64, _to: u64) -> Result<(), D::Error> {
        Ok(())
    }
}
