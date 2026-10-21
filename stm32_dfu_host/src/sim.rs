//! A simulated STM32 system bootloader in DFU mode, at the level of the control requests: the DFU state machine
//! (DNBUSY polling, error states, manifestation), DfuSe set address / erase, block addressing, and flash that behaves
//! as NOR flash: erase sets bytes to `0xFF`, programming only clears bits and takes whole flash words, so a write
//! without an erase or an unaligned one shows up as an error status like on the ROM.
//!
//! [SimStm32Dfu] is a cheap handle: clone it to keep looking at the flash and the leave request while [crate::Dfuse]
//! owns another clone.

use std::sync::{Arc, Mutex};

use crate::io::{DfuIo, IoError};
use crate::layout::MemoryLayout;
use crate::proto::*;

/// Poll timeout reported while a command runs; short to keep tests fast.
const BUSY_MS: u32 = 1;

#[derive(Clone)]
pub struct SimStm32Dfu {
    state: Arc<Mutex<Sim>>,
    layout: String,
    transfer_size: usize,
}

struct Sim {
    layout: MemoryLayout,
    /// One buffer per layout region.
    mem: Vec<Vec<u8>>,
    uid: [u32; 3],
    state: State,
    status: DfuStatus,
    /// Result of the command just received, reported when its busy time is over.
    pending: Option<DfuStatus>,
    addr: u32,
    word: usize,
    transfer_size: usize,
    erased: Vec<u32>,
    left: Option<u32>,
    gone: bool,
    corrupt: Option<u32>,
}

impl SimStm32Dfu {
    /// An STM32H72x-like device: 1 MB of erased flash in eight 128 KB sectors at `0x08000000`, 32-byte flash words,
    /// 1024-byte transfers.
    pub fn h72x(uid: [u32; 3]) -> Self {
        Self::with_layout("@Internal Flash  /0x08000000/08*128Kg", uid, 1024, 32)
    }

    pub fn with_layout(layout: &str, uid: [u32; 3], transfer_size: usize, word: usize) -> Self {
        let parsed = MemoryLayout::parse(layout).expect("layout of the simulated device");
        let mem = parsed
            .regions
            .iter()
            .map(|r| vec![0xFF; (r.sector_size * r.sectors) as usize])
            .collect();
        SimStm32Dfu {
            layout: layout.to_string(),
            transfer_size,
            state: Arc::new(Mutex::new(Sim {
                layout: parsed,
                mem,
                uid,
                state: State::DfuIdle,
                status: DfuStatus::Ok,
                pending: None,
                addr: 0,
                word,
                transfer_size,
                erased: vec![],
                left: None,
                gone: false,
                corrupt: None,
            })),
        }
    }

    /// The serial number the device shows, see [crate::rom_serial].
    pub fn serial(&self) -> String {
        crate::rom_serial(self.state.lock().unwrap().uid)
    }

    /// Put bytes into memory as if they had been there (a bootloader already in flash).
    pub fn preload(&self, addr: u32, data: &[u8]) {
        let mut s = self.state.lock().unwrap();
        for (i, b) in data.iter().enumerate() {
            let at = addr + i as u32;
            *s.byte(at).expect("preload inside memory") = *b;
        }
    }

    /// `len` bytes of memory at `addr`.
    pub fn memory(&self, addr: u32, len: usize) -> Vec<u8> {
        let mut s = self.state.lock().unwrap();
        (0..len as u32).map(|i| *s.byte(addr + i).expect("inside memory")).collect()
    }

    /// Sectors erased so far, in order.
    pub fn erased(&self) -> Vec<u32> {
        self.state.lock().unwrap().erased.clone()
    }

    /// The address the host asked to jump to when it left DFU mode, `None` while the device is still in DFU.
    pub fn left(&self) -> Option<u32> {
        self.state.lock().unwrap().left
    }

    /// Flip the lowest bit of the byte at `addr` as soon as it is programmed, so a read-back differs.
    pub fn corrupt_on_write(&self, addr: u32) {
        self.state.lock().unwrap().corrupt = Some(addr);
    }
}

impl Sim {
    fn byte(&mut self, addr: u32) -> Option<&mut u8> {
        let i = self.layout.regions.iter().position(|r| r.contains(addr))?;
        let off = (addr - self.layout.regions[i].start) as usize;
        self.mem[i].get_mut(off)
    }

    fn fail(&mut self, status: DfuStatus) {
        self.pending = Some(status);
    }

    fn command(&mut self, data: &[u8]) {
        let addr = data.get(1..5).map(|a| u32::from_le_bytes(a.try_into().unwrap()));
        match (data[0], addr) {
            (CMD_SET_ADDRESS, Some(addr)) => {
                if self.layout.region_at(addr).is_some() {
                    self.addr = addr;
                } else {
                    self.fail(DfuStatus::Address);
                }
            }
            (CMD_ERASE, Some(addr)) => match self.layout.sector_at(addr) {
                Some((base, size, r)) if r.erasable => {
                    for i in 0..size {
                        *self.byte(base + i).unwrap() = 0xFF;
                    }
                    self.erased.push(base);
                }
                Some(_) => self.fail(DfuStatus::Erase),
                None => self.fail(DfuStatus::Address),
            },
            (CMD_GET_COMMANDS, _) => {}
            _ => self.fail(DfuStatus::Unknown),
        }
    }

    fn program(&mut self, block: u16, data: &[u8]) {
        let addr = self.addr + (block - FIRST_DATA_BLOCK) as u32 * self.transfer_size as u32;
        if !self.layout.writable(addr, data.len()) {
            return self.fail(DfuStatus::Address);
        }
        if !(addr as usize).is_multiple_of(self.word) || !data.len().is_multiple_of(self.word) {
            return self.fail(DfuStatus::Prog);
        }
        for (i, b) in data.iter().enumerate() {
            let cell = self.byte(addr + i as u32).unwrap();
            if *cell & *b != *b {
                // a bit that is 0 can't be programmed to 1: not erased
                return self.fail(DfuStatus::Prog);
            }
            *cell &= *b;
        }
        if let Some(c) = self.corrupt
            && (addr..addr + data.len() as u32).contains(&c)
        {
            *self.byte(c).unwrap() ^= 1;
        }
    }
}

impl DfuIo for SimStm32Dfu {
    async fn control_out(&mut self, request: u8, value: u16, data: &[u8]) -> Result<(), IoError> {
        let mut s = self.state.lock().unwrap();
        if s.gone {
            return Err(IoError::Disconnected);
        }
        match request {
            DFU_DNLOAD => {
                if !matches!(s.state, State::DfuIdle | State::DnloadIdle) {
                    return Err(IoError::Stall);
                }
                s.pending = None;
                if value == 0 && data.is_empty() {
                    s.state = State::ManifestSync;
                } else if value == 0 {
                    s.command(data);
                    s.state = State::DnloadSync;
                } else if value >= FIRST_DATA_BLOCK {
                    s.program(value, data);
                    s.state = State::DnloadSync;
                } else {
                    return Err(IoError::Stall);
                }
                Ok(())
            }
            DFU_CLRSTATUS => {
                if s.state != State::Error {
                    return Err(IoError::Stall);
                }
                s.status = DfuStatus::Ok;
                s.state = State::DfuIdle;
                Ok(())
            }
            DFU_ABORT => {
                if matches!(s.state, State::Error | State::Manifest | State::ManifestSync) {
                    return Err(IoError::Stall);
                }
                s.state = State::DfuIdle;
                Ok(())
            }
            _ => Err(IoError::Stall),
        }
    }

    async fn control_in(&mut self, request: u8, value: u16, len: usize) -> Result<Vec<u8>, IoError> {
        let mut s = self.state.lock().unwrap();
        if s.gone {
            return Err(IoError::Disconnected);
        }
        match request {
            DFU_GETSTATUS => {
                let reply = match s.state {
                    State::DnloadSync => {
                        // busy for BUSY_MS, the answer to the next request is the result
                        s.state = match s.pending.take() {
                            Some(e) => {
                                s.status = e;
                                State::Error
                            }
                            None => State::DnloadIdle,
                        };
                        Status {
                            status: DfuStatus::Ok,
                            poll_timeout_ms: BUSY_MS,
                            state: State::DnBusy,
                        }
                    }
                    State::ManifestSync => {
                        s.state = State::ManifestWaitReset;
                        s.left = Some(s.addr);
                        s.gone = true;
                        Status {
                            status: DfuStatus::Ok,
                            poll_timeout_ms: BUSY_MS,
                            state: State::Manifest,
                        }
                    }
                    state => Status {
                        status: s.status,
                        poll_timeout_ms: 0,
                        state,
                    },
                };
                Ok(reply.to_bytes()[..len.min(Status::LEN)].to_vec())
            }
            DFU_GETSTATE => Ok(vec![s.state.into()]),
            DFU_UPLOAD => {
                if !matches!(s.state, State::DfuIdle | State::UploadIdle) || value < FIRST_DATA_BLOCK {
                    return Err(IoError::Stall);
                }
                let addr = s.addr + (value - FIRST_DATA_BLOCK) as u32 * s.transfer_size as u32;
                let n = len.min(s.transfer_size);
                if !s.layout.readable(addr, n) {
                    s.state = State::Error;
                    s.status = DfuStatus::Address;
                    return Err(IoError::Stall);
                }
                s.state = State::UploadIdle;
                Ok((0..n as u32).map(|i| *s.byte(addr + i).unwrap()).collect())
            }
            _ => Err(IoError::Stall),
        }
    }

    fn layout_string(&self) -> &str {
        &self.layout
    }

    fn transfer_size(&self) -> usize {
        self.transfer_size
    }
}
