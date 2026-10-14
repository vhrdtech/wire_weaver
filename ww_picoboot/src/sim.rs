//! A simulated RP2350 boot ROM in BOOTSEL mode, at the level of PICOBOOT's USB transfers: the command packet, the
//! data phase, the zero-length status phase, stalls on refused commands, the interface reset and status requests. It
//! checks what the boot ROM checks (lengths, alignment, address ranges) and keeps flash as NOR flash behaves: erase
//! sets bytes to `0xFF`, programming only clears bits, so a write without an erase shows up as a verify error.
//!
//! [SimRp2350] is a cheap handle: clone it to keep looking at the flash and the reboot request while
//! [crate::Picoboot] owns another clone.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::io::{IoError, PicobootIo};
use crate::proto::{
    self, CMD_LEN, Cmd, CmdId, CmdStatus, FLASH_BASE, FLASH_PAGE_SIZE, FLASH_SECTOR_SIZE, Status,
};

/// A `REBOOT2` the simulated device accepted.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RebootRequest {
    /// `REBOOT2_FLAG_*`, [proto::REBOOT2_TYPE_MASK] selects the type.
    pub flags: u32,
    pub delay: Duration,
    /// `dParam0`: the flash address of the new image for `REBOOT2_FLAG_REBOOT_TYPE_FLASH_UPDATE`.
    pub param0: u32,
}

#[derive(Clone)]
pub struct SimRp2350 {
    state: Arc<Mutex<State>>,
}

enum Phase {
    /// Waiting for a command.
    Idle,
    /// Receiving `len` bytes for `cmd`.
    DataOut { cmd: Cmd, buf: Vec<u8> },
    /// Data ready for the host.
    DataIn(Vec<u8>),
    /// Done, the host reads the zero-length status packet.
    AckIn,
    /// Done, the host sends the zero-length status packet.
    AckOut,
}

struct State {
    flash: Vec<u8>,
    chip_id: u64,
    phase: Phase,
    stalled: bool,
    status: CmdStatus,
    exclusive: u8,
    reboot: Option<RebootRequest>,
    commands: Vec<CmdId>,
    fail_next: Option<(CmdId, Status)>,
}

impl SimRp2350 {
    /// A device with `flash_size` bytes of erased flash.
    pub fn new(chip_id: u64, flash_size: usize) -> Self {
        assert!(flash_size.is_multiple_of(FLASH_SECTOR_SIZE));
        SimRp2350 {
            state: Arc::new(Mutex::new(State {
                flash: vec![0xFF; flash_size],
                chip_id,
                phase: Phase::Idle,
                stalled: false,
                status: CmdStatus {
                    token: 0,
                    status: Status::Ok,
                    cmd_id: 0,
                    in_progress: false,
                },
                exclusive: 0,
                reboot: None,
                commands: vec![],
                fail_next: None,
            })),
        }
    }

    /// The USB serial number the boot ROM reports: the chip id, 16 upper case hex digits.
    pub fn serial(&self) -> String {
        format!("{:016X}", self.state.lock().unwrap().chip_id)
    }

    /// Flash contents at `addr` (an address in the XIP window, from [FLASH_BASE]).
    pub fn flash(&self, addr: u32, len: usize) -> Vec<u8> {
        let s = self.state.lock().unwrap();
        let at = (addr - FLASH_BASE) as usize;
        s.flash[at..at + len].to_vec()
    }

    /// Put bytes into flash as if programmed earlier (a firmware already on the device).
    pub fn preload(&self, addr: u32, data: &[u8]) {
        let mut s = self.state.lock().unwrap();
        let at = (addr - FLASH_BASE) as usize;
        s.flash[at..at + data.len()].copy_from_slice(data);
    }

    /// The accepted `REBOOT2`, if any: the device leaves BOOTSEL.
    pub fn reboot_request(&self) -> Option<RebootRequest> {
        self.state.lock().unwrap().reboot
    }

    /// Commands accepted so far, in order.
    pub fn commands(&self) -> Vec<CmdId> {
        self.state.lock().unwrap().commands.clone()
    }

    /// The device reset: the boot ROM starts over (no command in progress, no reboot request), flash is kept.
    pub fn reset(&self) {
        let mut s = self.state.lock().unwrap();
        s.phase = Phase::Idle;
        s.stalled = false;
        s.status.in_progress = false;
        s.exclusive = 0;
        s.reboot = None;
        s.fail_next = None;
    }

    /// Refuse the next `cmd` with `status`.
    pub fn fail_next(&self, cmd: CmdId, status: Status) {
        self.state.lock().unwrap().fail_next = Some((cmd, status));
    }
}

impl State {
    fn stall(&mut self, cmd_id: u8, status: Status) -> IoError {
        self.stalled = true;
        self.phase = Phase::Idle;
        self.status.cmd_id = cmd_id;
        self.status.status = status;
        self.status.in_progress = false;
        IoError::Stall
    }

    fn flash_range(
        &self,
        addr: u32,
        len: u32,
        align: usize,
    ) -> Result<std::ops::Range<usize>, Status> {
        let end = addr as u64 + len as u64;
        if addr < FLASH_BASE || end > FLASH_BASE as u64 + self.flash.len() as u64 {
            return Err(Status::InvalidAddress);
        }
        if !(addr as usize).is_multiple_of(align) || !(len as usize).is_multiple_of(align) {
            return Err(Status::BadAlignment);
        }
        let at = (addr - FLASH_BASE) as usize;
        Ok(at..at + len as usize)
    }

    /// Check a command and run what can run before the data phase. Returns the data for IN commands.
    fn start(&mut self, cmd: &Cmd) -> Result<Option<Vec<u8>>, Status> {
        if let Some((id, status)) = self.fail_next
            && id == cmd.id
        {
            self.fail_next = None;
            return Err(status);
        }
        if self.reboot.is_some() {
            return Err(Status::Rebooting);
        }
        let (args_len, needs_transfer) = match cmd.id {
            CmdId::ExclusiveAccess => (1, false),
            CmdId::ExitXip | CmdId::EnterCmdXip => (0, false),
            CmdId::FlashErase => (8, false),
            CmdId::Read | CmdId::Write => (8, true),
            CmdId::Reboot2 => (16, false),
            CmdId::GetInfo => (16, true),
            // RP2040 only, or not simulated
            _ => return Err(Status::UnknownCmd),
        };
        if cmd.args_len != args_len {
            return Err(Status::InvalidCmdLength);
        }
        if needs_transfer != (cmd.transfer_len != 0) {
            return Err(Status::InvalidTransferLength);
        }
        match cmd.id {
            CmdId::Read => {
                let (addr, size) = (cmd.arg_u32(0), cmd.arg_u32(1));
                if size != cmd.transfer_len {
                    return Err(Status::InvalidTransferLength);
                }
                let range = self.flash_range(addr, size, 1)?;
                Ok(Some(self.flash[range].to_vec()))
            }
            CmdId::Write => {
                let (addr, size) = (cmd.arg_u32(0), cmd.arg_u32(1));
                if size != cmd.transfer_len {
                    return Err(Status::InvalidTransferLength);
                }
                self.flash_range(addr, size, FLASH_PAGE_SIZE)?;
                Ok(None)
            }
            CmdId::GetInfo => {
                if cmd.args[0] != proto::GET_INFO_SYS {
                    return Err(Status::InvalidArg);
                }
                let flags = cmd.arg_u32(1) & proto::SYS_INFO_CHIP_INFO;
                let mut words = vec![0u32, flags];
                if flags & proto::SYS_INFO_CHIP_INFO != 0 {
                    // package selection, then the chip id
                    words.extend([0, self.chip_id as u32, (self.chip_id >> 32) as u32]);
                }
                words[0] = words.len() as u32 - 1;
                let mut data: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
                data.resize(cmd.transfer_len as usize, 0);
                Ok(Some(data))
            }
            _ => Ok(None),
        }
    }

    /// Run an OUT command once its data (if any) arrived.
    fn finish(&mut self, cmd: &Cmd, data: &[u8]) -> Result<(), Status> {
        match cmd.id {
            CmdId::ExclusiveAccess => {
                if cmd.args[0] > 2 {
                    return Err(Status::InvalidArg);
                }
                self.exclusive = cmd.args[0];
            }
            CmdId::FlashErase => {
                let range = self.flash_range(cmd.arg_u32(0), cmd.arg_u32(1), FLASH_SECTOR_SIZE)?;
                self.flash[range].fill(0xFF);
            }
            CmdId::Write => {
                let range = self.flash_range(cmd.arg_u32(0), cmd.arg_u32(1), FLASH_PAGE_SIZE)?;
                // NOR flash: programming clears bits only
                for (f, d) in self.flash[range].iter_mut().zip(data) {
                    *f &= *d;
                }
            }
            CmdId::Reboot2 => {
                self.reboot = Some(RebootRequest {
                    flags: cmd.arg_u32(0),
                    delay: Duration::from_millis(cmd.arg_u32(1) as u64),
                    param0: cmd.arg_u32(2),
                });
            }
            _ => {}
        }
        Ok(())
    }
}

impl PicobootIo for SimRp2350 {
    async fn bulk_out(&mut self, data: &[u8]) -> Result<(), IoError> {
        let mut s = self.state.lock().unwrap();
        if s.stalled {
            return Err(IoError::Stall);
        }
        match std::mem::replace(&mut s.phase, Phase::Idle) {
            Phase::Idle => {
                if data.len() != CMD_LEN {
                    return Err(s.stall(0, Status::InvalidCmdLength));
                }
                let (token, cmd) = match Cmd::decode(data) {
                    Ok(c) => c,
                    Err(status) => return Err(s.stall(data[8], status)),
                };
                s.status = CmdStatus {
                    token,
                    status: Status::Ok,
                    cmd_id: cmd.id as u8,
                    in_progress: true,
                };
                match s.start(&cmd) {
                    Err(status) => return Err(s.stall(cmd.id as u8, status)),
                    Ok(Some(data)) => s.phase = Phase::DataIn(data),
                    Ok(None) if cmd.transfer_len > 0 => {
                        s.phase = Phase::DataOut {
                            cmd,
                            buf: Vec::new(),
                        }
                    }
                    Ok(None) => {
                        if let Err(status) = s.finish(&cmd, &[]) {
                            return Err(s.stall(cmd.id as u8, status));
                        }
                        s.commands.push(cmd.id);
                        s.phase = if cmd.id.is_in() {
                            Phase::AckOut
                        } else {
                            Phase::AckIn
                        };
                    }
                }
                Ok(())
            }
            Phase::DataOut { cmd, mut buf } => {
                buf.extend_from_slice(data);
                if buf.len() > cmd.transfer_len as usize {
                    return Err(s.stall(cmd.id as u8, Status::InvalidTransferLength));
                }
                if buf.len() < cmd.transfer_len as usize {
                    s.phase = Phase::DataOut { cmd, buf };
                    return Ok(());
                }
                if let Err(status) = s.finish(&cmd, &buf) {
                    return Err(s.stall(cmd.id as u8, status));
                }
                s.commands.push(cmd.id);
                s.phase = Phase::AckIn;
                Ok(())
            }
            Phase::AckOut if data.is_empty() => {
                let id = CmdId::from_u8(s.status.cmd_id).unwrap();
                s.commands.push(id);
                s.status.in_progress = false;
                Ok(())
            }
            Phase::AckOut | Phase::AckIn | Phase::DataIn(_) => {
                let id = s.status.cmd_id;
                Err(s.stall(id, Status::InvalidState))
            }
        }
    }

    async fn bulk_in(&mut self, len: usize) -> Result<Vec<u8>, IoError> {
        let mut s = self.state.lock().unwrap();
        if s.stalled {
            return Err(IoError::Stall);
        }
        match std::mem::replace(&mut s.phase, Phase::Idle) {
            Phase::DataIn(data) => {
                if data.len() != len {
                    let id = s.status.cmd_id;
                    return Err(s.stall(id, Status::InvalidTransferLength));
                }
                s.phase = Phase::AckOut;
                Ok(data)
            }
            Phase::AckIn if len == 0 => {
                s.status.in_progress = false;
                Ok(vec![])
            }
            other => {
                // nothing to send: a real device NAKs until the host gives up
                s.phase = other;
                Err(IoError::Timeout)
            }
        }
    }

    async fn reset_interface(&mut self) -> Result<(), IoError> {
        let mut s = self.state.lock().unwrap();
        s.stalled = false;
        s.phase = Phase::Idle;
        s.status.in_progress = false;
        Ok(())
    }

    async fn cmd_status(&mut self) -> Result<[u8; 16], IoError> {
        Ok(self.state.lock().unwrap().status.encode())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::Reboot2;
    use crate::{Error, Image, Picoboot, Stage};

    const CHIP_ID: u64 = 0x1234_5678_9ABC_DEF0;

    async fn connect() -> (SimRp2350, Picoboot<SimRp2350>) {
        let sim = SimRp2350::new(CHIP_ID, 64 * 1024);
        let pb = Picoboot::new(sim.clone()).await.unwrap();
        (sim, pb)
    }

    #[tokio::test]
    async fn flash_verify_reboot() {
        let (sim, mut pb) = connect().await;
        // an old firmware is there, the new one is shorter: the rest of the touched sector ends up erased
        sim.preload(FLASH_BASE, &[0x55; 8192]);
        let new: Vec<u8> = (0..5000u32).map(|i| i as u8).collect();
        let image = Image::from_bin(FLASH_BASE, new.clone()).unwrap();
        let mut stages = vec![];
        pb.flash(&image, &mut |stage, done, total| {
            if done == total {
                stages.push(stage);
            }
        })
        .await
        .unwrap();
        assert_eq!(stages, [Stage::Erase, Stage::Write, Stage::Verify]);
        assert_eq!(sim.flash(FLASH_BASE, 5000), new);
        assert_eq!(sim.flash(FLASH_BASE + 5000, 3192), vec![0xFF; 3192]);
        assert_eq!(sim.flash(FLASH_BASE + 8192, 4), vec![0xFF; 4]);

        pb.reboot2(Reboot2::Normal, proto::REBOOT_DELAY)
            .await
            .unwrap();
        let reboot = sim.reboot_request().unwrap();
        assert_eq!(
            reboot.flags & proto::REBOOT2_TYPE_MASK,
            proto::REBOOT2_TYPE_NORMAL
        );
        assert_eq!(reboot.delay, proto::REBOOT_DELAY);
        assert_eq!(
            sim.commands()[..2],
            [CmdId::ExclusiveAccess, CmdId::ExitXip]
        );
    }

    #[tokio::test]
    async fn chip_id_matches_serial() {
        let (sim, mut pb) = connect().await;
        let id = pb.chip_id().await.unwrap();
        assert_eq!(id, CHIP_ID);
        assert_eq!(sim.serial(), "123456789ABCDEF0");
    }

    #[tokio::test]
    async fn refused_command_reports_status_and_recovers() {
        let (_sim, mut pb) = connect().await;
        let err = pb.flash_erase(FLASH_BASE + 100, 4096).await.unwrap_err();
        assert!(
            matches!(
                err,
                Error::Command {
                    cmd: "FLASH_ERASE",
                    status: Status::BadAlignment
                }
            ),
            "{err}"
        );
        let err = pb.read(0x2000_0000, 4).await.unwrap_err();
        assert!(
            matches!(
                err,
                Error::Command {
                    status: Status::InvalidAddress,
                    ..
                }
            ),
            "{err}"
        );
        // the interface was reset, the next command works
        assert_eq!(pb.read(FLASH_BASE, 4).await.unwrap(), [0xFF; 4]);
    }

    #[tokio::test]
    async fn failed_write_stops_the_flash() {
        let (sim, mut pb) = connect().await;
        sim.fail_next(CmdId::Write, Status::NotPermitted);
        let image = Image::from_bin(FLASH_BASE, vec![1; 300]).unwrap();
        let err = pb.flash(&image, &mut ()).await.unwrap_err();
        assert!(
            matches!(
                err,
                Error::Command {
                    cmd: "WRITE",
                    status: Status::NotPermitted
                }
            ),
            "{err}"
        );
    }

    #[tokio::test]
    async fn write_without_erase_fails_verify() {
        let (sim, mut pb) = connect().await;
        sim.preload(FLASH_BASE, &[0x0F; 256]);
        pb.write(FLASH_BASE, &[0xF0; 256]).await.unwrap();
        assert_eq!(sim.flash(FLASH_BASE, 1), [0x00]);
    }
}
