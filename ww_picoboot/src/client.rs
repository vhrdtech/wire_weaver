use std::time::Duration;

use crate::Error;
use crate::image::Image;
use crate::io::{IoError, PicobootIo};
use crate::proto::{self, Cmd, CmdId, FLASH_PAGE_SIZE, FLASH_SECTOR_SIZE, Reboot2, Status};

/// What [Picoboot::flash] is doing, for a progress bar.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Stage {
    Erase,
    Write,
    Verify,
}

/// Progress of [Picoboot::flash]: `done` of `total` bytes of the current [Stage].
pub trait Progress {
    fn update(&mut self, stage: Stage, done: usize, total: usize);
}

impl Progress for () {
    fn update(&mut self, _: Stage, _: usize, _: usize) {}
}

impl<F: FnMut(Stage, usize, usize)> Progress for F {
    fn update(&mut self, stage: Stage, done: usize, total: usize) {
        self(stage, done, total)
    }
}

/// A PICOBOOT connection: one command at a time, each with its own token.
pub struct Picoboot<I> {
    io: I,
    token: u32,
}

impl<I: PicobootIo> Picoboot<I> {
    /// Start talking over `io`; resets the interface first, as a previous user may have left a command half done.
    pub async fn new(mut io: I) -> Result<Self, Error> {
        io.reset_interface().await?;
        Ok(Picoboot { io, token: 1 })
    }

    pub fn into_inner(self) -> I {
        self.io
    }

    /// Run one command: the 32-byte command, the data phase if any, then the zero-length status phase in the
    /// opposite direction. A refused command (stall) is turned into [Error::Command] with the device's status, and
    /// the interface is reset so the next command works.
    async fn cmd(&mut self, cmd: Cmd, data_out: &[u8]) -> Result<Vec<u8>, Error> {
        let token = self.token;
        self.token = self.token.wrapping_add(1);
        let id = cmd.id;
        let bytes = cmd.encode(token);
        match self
            .cmd_phases(&bytes, id, cmd.transfer_len as usize, data_out)
            .await
        {
            Ok(data) => Ok(data),
            Err(IoError::Stall) => {
                let status = match self.io.cmd_status().await {
                    Ok(raw) => proto::CmdStatus::decode(&raw).status,
                    Err(_) => Status::UnknownError,
                };
                self.io.reset_interface().await?;
                Err(Error::Command {
                    cmd: id.name(),
                    status,
                })
            }
            Err(e) => Err(e.into()),
        }
    }

    async fn cmd_phases(
        &mut self,
        cmd: &[u8; 32],
        id: CmdId,
        transfer_len: usize,
        data_out: &[u8],
    ) -> Result<Vec<u8>, IoError> {
        self.io.bulk_out(cmd).await?;
        let mut data = vec![];
        if id.is_in() {
            if transfer_len > 0 {
                data = self.io.bulk_in(transfer_len).await?;
            }
            self.io.bulk_out(&[]).await?;
        } else {
            if transfer_len > 0 {
                self.io.bulk_out(data_out).await?;
            }
            self.io.bulk_in(0).await?;
        }
        Ok(data)
    }

    /// Lock out the UF2 drive while flashing (`eject` also unmounts it on the host).
    pub async fn exclusive_access(&mut self, exclusive: bool, eject: bool) -> Result<(), Error> {
        let mode = match (exclusive, eject) {
            (false, _) => 0,
            (true, false) => 1,
            (true, true) => 2,
        };
        self.cmd(Cmd::new(CmdId::ExclusiveAccess, &[mode], 0), &[])
            .await
            .map(drop)
    }

    /// Leave flash XIP mode, so it can be erased and written.
    pub async fn exit_xip(&mut self) -> Result<(), Error> {
        self.cmd(Cmd::new(CmdId::ExitXip, &[], 0), &[])
            .await
            .map(drop)
    }

    /// Erase `len` bytes at `addr`, both multiples of the 4 KiB sector.
    pub async fn flash_erase(&mut self, addr: u32, len: u32) -> Result<(), Error> {
        self.cmd(Cmd::range(CmdId::FlashErase, addr, len, 0), &[])
            .await
            .map(drop)
    }

    /// Write `data` at `addr`: flash (erased before, 256-byte pages) or RAM.
    pub async fn write(&mut self, addr: u32, data: &[u8]) -> Result<(), Error> {
        let len = data.len() as u32;
        self.cmd(Cmd::range(CmdId::Write, addr, len, len), data)
            .await
            .map(drop)
    }

    /// Read `len` bytes at `addr`.
    pub async fn read(&mut self, addr: u32, len: u32) -> Result<Vec<u8>, Error> {
        let data = self
            .cmd(Cmd::range(CmdId::Read, addr, len, len), &[])
            .await?;
        if data.len() != len as usize {
            return Err(Error::Other(format!(
                "READ returned {} bytes instead of {len}",
                data.len()
            )));
        }
        Ok(data)
    }

    /// RP2350 `REBOOT2`; the device leaves BOOTSEL after `delay`.
    pub async fn reboot2(&mut self, reboot: Reboot2, delay: Duration) -> Result<(), Error> {
        self.cmd(reboot.cmd(delay), &[]).await.map(drop)
    }

    /// RP2350 `GET_INFO` with `PICOBOOT_GET_INFO_SYS`: the words the boot ROM returns for `flags`, the first word
    /// being the number of words that follow.
    pub async fn get_sys_info(&mut self, flags: u32) -> Result<Vec<u32>, Error> {
        let len = proto::GET_INFO_MAX_LEN;
        let data = self.cmd(proto::get_sys_info(flags, len), &[]).await?;
        let words: Vec<u32> = data
            .as_chunks::<4>()
            .0
            .iter()
            .map(|w| u32::from_le_bytes(*w))
            .collect();
        let n = words.first().copied().unwrap_or(0) as usize;
        Ok(words.into_iter().skip(1).take(n).collect())
    }

    /// The 64-bit chip id (RP2350), as the boot ROM shows it in its USB serial number.
    pub async fn chip_id(&mut self) -> Result<u64, Error> {
        let words = self.get_sys_info(proto::SYS_INFO_CHIP_INFO).await?;
        proto::chip_id_from_sys_info(&words)
            .ok_or_else(|| Error::Other(format!("unexpected GET_INFO reply {words:x?}")))
    }

    /// Erase the sectors `image` touches, write it, read it back. Bytes of those sectors outside the image end up
    /// erased (`0xFF`). Does not reboot.
    pub async fn flash(
        &mut self,
        image: &Image,
        progress: &mut impl Progress,
    ) -> Result<(), Error> {
        let sectors = image.sectors();
        let total = sectors.len() * FLASH_SECTOR_SIZE;
        self.exclusive_access(true, false).await?;
        self.exit_xip().await?;
        for (i, s) in sectors.iter().enumerate() {
            progress.update(Stage::Erase, i * FLASH_SECTOR_SIZE, total);
            self.flash_erase(s.addr, FLASH_SECTOR_SIZE as u32).await?;
        }
        progress.update(Stage::Erase, total, total);
        let mut done = 0;
        for s in &sectors {
            for (i, page) in s.data.chunks(FLASH_PAGE_SIZE).enumerate() {
                progress.update(Stage::Write, done, total);
                // an erased page stays as it is
                if page.iter().any(|b| *b != 0xFF) {
                    self.write(s.addr + (i * FLASH_PAGE_SIZE) as u32, page)
                        .await?;
                }
                done += page.len();
            }
        }
        progress.update(Stage::Write, total, total);
        for (i, s) in sectors.iter().enumerate() {
            progress.update(Stage::Verify, i * FLASH_SECTOR_SIZE, total);
            let read = self.read(s.addr, FLASH_SECTOR_SIZE as u32).await?;
            if let Some(at) = read.iter().zip(&s.data).position(|(a, b)| a != b) {
                return Err(Error::Verify {
                    addr: s.addr + at as u32,
                });
            }
        }
        progress.update(Stage::Verify, total, total);
        self.exclusive_access(false, false).await
    }
}
