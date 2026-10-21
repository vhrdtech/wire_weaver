//! The DfuSe protocol on top of [DfuIo]: status polling, set address, erase, download, upload, leave.

use std::time::{Duration, Instant};

use crate::image::Image;
use crate::io::{DfuIo, IoError};
use crate::layout::MemoryLayout;
use crate::proto::*;
use crate::{Error, Progress, Stage};

/// How long the pieces of a flash run may take.
#[derive(Clone, Debug)]
pub struct Timeouts {
    /// One command until the device is ready again: a 128 KB sector erase takes seconds on an H7.
    pub command: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Timeouts {
            command: Duration::from_secs(60),
        }
    }
}

/// Options of [Dfuse::flash].
#[derive(Clone, Debug)]
pub struct FlashOptions {
    /// Programming unit the image is padded to with `0xFF`: 32 bytes is a flash word on the H7 and fine on the others.
    pub align: u32,
    /// Skip the read-back.
    pub no_verify: bool,
}

impl Default for FlashOptions {
    fn default() -> Self {
        FlashOptions {
            align: 32,
            no_verify: false,
        }
    }
}

/// A DfuSe device in DFU mode, one alternate setting (memory) of it.
pub struct Dfuse<I> {
    io: I,
    layout: MemoryLayout,
    transfer_size: usize,
    pub timeouts: Timeouts,
}

impl<I: DfuIo> Dfuse<I> {
    pub fn new(io: I) -> Result<Self, Error> {
        let layout = MemoryLayout::parse(io.layout_string())?;
        let transfer_size = io.transfer_size().max(1);
        Ok(Dfuse {
            io,
            layout,
            transfer_size,
            timeouts: Timeouts::default(),
        })
    }

    pub fn layout(&self) -> &MemoryLayout {
        &self.layout
    }

    pub fn transfer_size(&self) -> usize {
        self.transfer_size
    }

    pub fn into_io(self) -> I {
        self.io
    }

    /// `DFU_GETSTATUS`.
    pub async fn get_status(&mut self) -> Result<Status, Error> {
        let b = self.io.control_in(DFU_GETSTATUS, 0, Status::LEN).await?;
        Status::parse(&b).ok_or(Error::Protocol(format!("status of {} bytes", b.len())))
    }

    /// Get the device to `dfuIDLE` from whatever state a previous run left it in.
    pub async fn to_idle(&mut self) -> Result<(), Error> {
        for _ in 0..4 {
            let s = self.get_status().await?;
            match s.state {
                State::DfuIdle if s.status == DfuStatus::Ok => return Ok(()),
                State::Error | State::DfuIdle => self.io.control_out(DFU_CLRSTATUS, 0, &[]).await?,
                State::AppIdle | State::AppDetach => {
                    return Err(Error::Protocol(
                        "the device runs its application, not DFU".into(),
                    ));
                }
                _ => self.io.control_out(DFU_ABORT, 0, &[]).await?,
            }
        }
        Err(Error::Protocol("the device does not go to dfuIDLE".into()))
    }

    /// Poll `DFU_GETSTATUS` until the command in progress is done, wait as long as the device asks. An error status
    /// is cleared, so the device can be used again.
    async fn wait_done(&mut self, what: &'static str) -> Result<State, Error> {
        let deadline = Instant::now() + self.timeouts.command;
        loop {
            let s = self.get_status().await?;
            if s.status != DfuStatus::Ok || s.state == State::Error {
                _ = self.io.control_out(DFU_CLRSTATUS, 0, &[]).await;
                return Err(Error::Device {
                    what,
                    status: s.status,
                });
            }
            match s.state {
                State::DnBusy | State::DnloadSync | State::ManifestSync | State::Manifest => {
                    if Instant::now() >= deadline {
                        return Err(Error::Timeout(what));
                    }
                    tokio::time::sleep(Duration::from_millis(s.poll_timeout_ms as u64)).await;
                }
                state => return Ok(state),
            }
        }
    }

    async fn command(&mut self, what: &'static str, cmd: u8, addr: u32) -> Result<(), Error> {
        let mut data = [cmd, 0, 0, 0, 0];
        data[1..].copy_from_slice(&addr.to_le_bytes());
        self.io.control_out(DFU_DNLOAD, 0, &data).await?;
        // the command runs in the first status request, which answers dfuDNBUSY with the time it takes
        self.wait_done(what).await.map(drop)
    }

    /// DfuSe "set address pointer".
    pub async fn set_address(&mut self, addr: u32) -> Result<(), Error> {
        self.command("set address", CMD_SET_ADDRESS, addr).await
    }

    /// DfuSe "erase": the sector containing `addr`.
    pub async fn erase_sector(&mut self, addr: u32) -> Result<(), Error> {
        self.command("erase", CMD_ERASE, addr).await
    }

    /// Write `data` at `addr`, no erase, no alignment (the device checks what it wants).
    pub async fn write(
        &mut self,
        addr: u32,
        data: &[u8],
        progress: &mut impl FnMut(usize),
    ) -> Result<(), Error> {
        self.set_address(addr).await?;
        for (i, chunk) in data.chunks(self.transfer_size).enumerate() {
            let block = FIRST_DATA_BLOCK + i as u16;
            self.io.control_out(DFU_DNLOAD, block, chunk).await?;
            self.wait_done("write").await?;
            progress(chunk.len());
        }
        Ok(())
    }

    /// Read `len` bytes at `addr`.
    pub async fn read(
        &mut self,
        addr: u32,
        len: usize,
        progress: &mut impl FnMut(usize),
    ) -> Result<Vec<u8>, Error> {
        self.set_address(addr).await?;
        // the address pointer survives the abort, and UPLOAD is only accepted from dfuIDLE
        self.io.control_out(DFU_ABORT, 0, &[]).await?;
        let mut out = Vec::with_capacity(len);
        let mut block = FIRST_DATA_BLOCK;
        while out.len() < len {
            let want = self.transfer_size.min(len - out.len());
            let got = self.io.control_in(DFU_UPLOAD, block, want).await?;
            if got.is_empty() {
                return Err(Error::Protocol(format!(
                    "the device returned no data at {:#010x}",
                    addr as usize + out.len()
                )));
            }
            progress(got.len());
            out.extend_from_slice(&got);
            block += 1;
        }
        out.truncate(len);
        // a read past the end is a protocol violation, back to dfuIDLE for the next command
        self.to_idle().await?;
        Ok(out)
    }

    /// Erase what `image` touches, write it, read it back and compare. `image` is padded to `options.align`.
    pub async fn flash(
        &mut self,
        image: &Image,
        options: &FlashOptions,
        progress: &mut impl Progress,
    ) -> Result<(), Error> {
        let image = image.aligned(options.align)?;
        for s in &image.segments {
            if !self.layout.writable(s.addr, s.data.len()) {
                return Err(Error::OutOfMemory {
                    addr: s.addr,
                    len: s.data.len(),
                    layout: self.layout.name.clone(),
                });
            }
        }
        let mut sectors: Vec<(u32, u32)> = vec![];
        for s in &image.segments {
            let mut at = s.addr as u64;
            let end = at + s.data.len() as u64;
            while at < end {
                let (base, size, region) =
                    self.layout.sector_at(at as u32).expect("checked writable");
                if region.erasable && sectors.last().is_none_or(|l| l.0 != base) {
                    sectors.push((base, size));
                }
                at = base as u64 + size as u64;
            }
        }
        self.to_idle().await?;

        let erase_total: u64 = sectors.iter().map(|s| s.1 as u64).sum();
        let mut done = 0;
        progress.update(Stage::Erase, 0, erase_total);
        for (base, size) in sectors {
            self.erase_sector(base).await?;
            done += size as u64;
            progress.update(Stage::Erase, done, erase_total);
        }

        let total = image.len() as u64;
        let mut done = 0;
        progress.update(Stage::Write, 0, total);
        for s in &image.segments {
            self.write(s.addr, &s.data, &mut |n| {
                done += n as u64;
                progress.update(Stage::Write, done, total);
            })
            .await?;
        }

        if !options.no_verify {
            let mut done = 0;
            progress.update(Stage::Verify, 0, total);
            for s in &image.segments {
                let back = self
                    .read(s.addr, s.data.len(), &mut |n| {
                        done += n as u64;
                        progress.update(Stage::Verify, done, total);
                    })
                    .await?;
                if let Some(i) = back.iter().zip(&s.data).position(|(a, b)| a != b) {
                    return Err(Error::Verify {
                        addr: s.addr + i as u32,
                        wrote: s.data[i],
                        read: back[i],
                    });
                }
            }
        }
        Ok(())
    }

    /// Leave DFU mode: the bootloader jumps to the application at `addr` (its vector table). The device disconnects
    /// while doing so; this returns once the request is accepted.
    pub async fn leave(&mut self, addr: u32) -> Result<(), Error> {
        self.set_address(addr).await?;
        self.io.control_out(DFU_DNLOAD, 0, &[]).await?;
        // the manifestation starts with this status request, the device may be gone before it answers
        match self.get_status().await {
            Ok(_)
            | Err(Error::Io(
                IoError::Disconnected | IoError::Stall | IoError::Timeout | IoError::Other(_),
            )) => Ok(()),
            Err(e) => Err(e),
        }
    }
}
