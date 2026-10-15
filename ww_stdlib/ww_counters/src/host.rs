//! Host side (feature `host`): a device's counters as a [cnt_core::Source], so `cnt_core` decodes them with the
//! firmware's ELF and `cnt_view` shows them as it does over a probe (`ww counters`).
//!
//! The calls themselves go through a client the host generates for [Counters](crate::Counters) (with its API, or as
//! a global trait client like `ww counters` does), wrapped in [CountersDevice].

use crate::{CounterBuffer, CountersInfoOwned};
use anyhow::{anyhow, bail};
use cnt_core::{Counters, Source, Storage};
use std::path::Path;

/// The calls of [Counters](crate::Counters) on a connected device, blocking: implemented on a generated client with
/// `blocking_call()`.
pub trait CountersDevice {
    /// [Counters::read](crate::Counters::read): words of `buffer` from `offset`, empty past the used words.
    fn read(&mut self, buffer: CounterBuffer, offset: u16) -> anyhow::Result<Vec<u32>>;
    /// [Counters::reset](crate::Counters::reset).
    fn reset(&mut self, buffer: CounterBuffer) -> anyhow::Result<()>;
}

/// A [Source] reading a device's `cnt` buffers through [CountersDevice].
pub struct WwSource<D> {
    device: D,
    info: CountersInfoOwned,
}

impl<D: CountersDevice> WwSource<D> {
    /// The device and its [Counters::INFO](crate::Counters::INFO), read by the caller.
    pub fn new(device: D, info: CountersInfoOwned) -> Self {
        WwSource { device, info }
    }

    /// The device's buffers, format and build id.
    pub fn info(&self) -> &CountersInfoOwned {
        &self.info
    }

    /// The device's build id, `None` when it has none.
    pub fn build_id(&self) -> Option<&[u8]> {
        Some(self.info.build_id.as_slice()).filter(|id| !id.is_empty())
    }

    /// Check what the ELF's counters say against the device's INFO: format version and used words. A mismatch
    /// means the ELF is not the device's firmware (its build id was not checked, or forced).
    pub fn check(&self, counters: &Counters, elf_path: &Path) -> anyhow::Result<()> {
        if self.info.format != cnt_core::SIGNATURE_VERSION {
            bail!(
                "The device uses cnt format version {}, this tool reads version {}",
                self.info.format,
                cnt_core::SIGNATURE_VERSION
            );
        }
        let device = [
            (Storage::Ram, Some(self.info.ram)),
            (Storage::Bkp, self.info.backup),
        ];
        for (storage, buffer) in device {
            let elf = counters.blocks().find(|b| b.storage() == storage);
            let (Some(elf), Some(buffer)) = (elf, buffer) else {
                if elf.is_some() {
                    bail!(
                        "{} has {storage} counters, the device has no such buffer",
                        elf_path.display()
                    );
                }
                continue;
            };
            if elf.used_words() > u64::from(buffer.used_words) {
                bail!(
                    "{} uses {} {storage} words, the device {}: other firmware",
                    elf_path.display(),
                    elf.used_words(),
                    buffer.used_words
                );
            }
        }
        Ok(())
    }
}

fn buffer(storage: Storage) -> CounterBuffer {
    match storage {
        Storage::Ram => CounterBuffer::Ram,
        Storage::Bkp => CounterBuffer::Backup,
    }
}

impl<D: CountersDevice> Source for WwSource<D> {
    fn read(&mut self, storage: Storage, offset: u64, words: &mut [u32]) -> anyhow::Result<()> {
        let mut done = 0;
        while done < words.len() {
            let at = offset + done as u64;
            let at = u16::try_from(at).map_err(|_| anyhow!("word {at} is past what Counters::read reaches"))?;
            let page = self.device.read(buffer(storage), at)?;
            if page.is_empty() {
                bail!(
                    "The device has {} {storage} words in use, {} needed",
                    offset + done as u64,
                    offset + words.len() as u64
                );
            }
            let n = page.len().min(words.len() - done);
            words[done..done + n].copy_from_slice(&page[..n]);
            done += n;
        }
        Ok(())
    }

    fn reset(&mut self, storage: Storage) -> anyhow::Result<()> {
        self.device.reset(buffer(storage))
    }
}
