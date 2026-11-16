//! Shared pieces for commands that read or write a board through its chip's boot ROM, straight through PICOBOOT
//! (RP2040 / RP2350 BOOTSEL) or DfuSe (the STM32 system bootloader): the [RomFlash] adapters over each
//! ([PicobootRom], [DfuseRom], used by `ww dfu rp flash`, `ww dfu stm32 flash`, `ww info` and `ww board rework
//! allow|disallow`), and finding and opening the one boot ROM device that matches a [Selection] (TOOL-30).

use anyhow::{Result, bail};
use wire_weaver_client::follow::Found;
use wire_weaver_client::follow::{Timeouts, open_retrying};
use wire_weaver_client::ww_picoboot::Picoboot;
use wire_weaver_client::ww_picoboot::usb::NusbIo as PicobootNusbIo;
use wire_weaver_client::ww_stm32_dfu::Dfuse;
use wire_weaver_client::ww_stm32_dfu::usb::{self as stm32_usb, NusbIo as DfuseNusbIo};
use ww_report::rework_check::{FlashOp, RomFlash};

use crate::device::Selection;
use crate::untrusted::escape;

/// Most of the flash the check reads looking for the table of the image in flash (RP: QSPI reads past the chip's
/// size wrap around, an erased 64 KiB ends the search sooner).
pub(crate) const RP_SCAN: u32 = 4 * 1024 * 1024;
/// Internal flash of an STM32, where the image and its table are.
pub(crate) const STM32_FLASH: u32 = 0x0800_0000;
/// Most of it the check reads looking for the table of the image in flash.
pub(crate) const STM32_SCAN: u32 = 2 * 1024 * 1024;
/// Flash program unit of the RP2040 / RP2350 boot ROM.
const RP_PAGE: u32 = 256;

/// [RomFlash] over PICOBOOT.
pub(crate) struct PicobootRom<'a>(pub(crate) &'a mut Picoboot<PicobootNusbIo>);

impl RomFlash for PicobootRom<'_> {
    async fn read(&mut self, addr: u32, len: u32) -> Result<Vec<u8>> {
        Ok(self.0.read(addr, len).await?)
    }

    async fn apply(&mut self, ops: &[FlashOp]) -> Result<()> {
        for op in ops {
            match op {
                FlashOp::Erase { from, to } => self.0.flash_erase(*from, to - from).await?,
                // whole 256-byte pages, the bytes around the write sent as 0xFF: NOR programming leaves them as
                // they are
                FlashOp::Write { offset, bytes } => {
                    let start = offset / RP_PAGE * RP_PAGE;
                    let end = (offset + bytes.len() as u32).div_ceil(RP_PAGE) * RP_PAGE;
                    let mut pages = vec![0xff; (end - start) as usize];
                    let at = (offset - start) as usize;
                    pages[at..at + bytes.len()].copy_from_slice(bytes);
                    self.0.write(start, &pages).await?;
                }
            }
        }
        Ok(())
    }
}

/// [RomFlash] over the STM32 ROM's DfuSe.
pub(crate) struct DfuseRom<'a>(pub(crate) &'a mut Dfuse<DfuseNusbIo>);

impl RomFlash for DfuseRom<'_> {
    async fn read(&mut self, addr: u32, len: u32) -> Result<Vec<u8>> {
        // past the end of the flash the ROM answers an error: read what there is
        let end = self
            .0
            .layout()
            .region_at(addr)
            .map_or(addr, |r| r.end().min(addr as u64 + len as u64) as u32);
        if end <= addr {
            return Ok(vec![]);
        }
        Ok(self
            .0
            .read(addr, (end - addr) as usize, &mut |_| {})
            .await?)
    }

    async fn apply(&mut self, ops: &[FlashOp]) -> Result<()> {
        for op in ops {
            match op {
                FlashOp::Erase { from, to } => {
                    let mut at = *from;
                    while at < *to {
                        let (start, size, _) = self.0.layout().sector_at(at).ok_or_else(|| {
                            anyhow::anyhow!("{at:#010x} is outside the bootloader's flash")
                        })?;
                        self.0.erase_sector(start).await?;
                        at = start + size;
                    }
                }
                FlashOp::Write { offset, bytes } => {
                    self.0.write(*offset, bytes, &mut |_| {}).await?;
                }
            }
        }
        Ok(())
    }
}

/// A boot ROM device found matching a [Selection], with enough to open it again (TOOL-30: `ww info`, `ww board
/// rework allow|disallow`).
pub(crate) enum RomTarget {
    Rp { serial: String },
    Stm32 { serial: String },
}

/// The one boot ROM device (BOOTSEL or STM32 DFU) matching `selection`; `None` when none do (a running WireWeaver
/// device, or nothing at all, isn't an error here: the caller falls back to its own path).
pub(crate) async fn find_rom(selection: &Selection) -> Result<Option<RomTarget>> {
    let config = selection.client_config();
    let devices: Vec<Found> = wire_weaver_client::list_all_usb_devices()
        .await
        .unwrap_or_default()
        .into_iter()
        .map(Found::classify)
        .collect();
    let matching: Vec<&Found> = devices
        .iter()
        .filter(|f| matches!(f, Found::RpBootsel(_) | Found::Stm32Dfu(_)))
        .filter(|f| config.matches(f.info()))
        .collect();
    match matching.as_slice() {
        [] => Ok(None),
        [one] => {
            let serial = one.info().serials.first().cloned().unwrap_or_default();
            Ok(Some(match one {
                Found::RpBootsel(_) => RomTarget::Rp { serial },
                Found::Stm32Dfu(_) => RomTarget::Stm32 { serial },
                _ => unreachable!("filtered to RpBootsel/Stm32Dfu above"),
            }))
        }
        many => bail!(
            "{} boot ROM devices match, choose one with --serial: {}",
            many.len(),
            many.iter()
                .map(|f| escape(f.info().serials.first().map_or("?", String::as_str)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// Open PICOBOOT by the exact serial [find_rom] gave.
pub(crate) async fn open_rp(serial: &str) -> Result<Picoboot<PicobootNusbIo>> {
    let devices = wire_weaver_client::ww_picoboot::usb::list().await?;
    let info = devices
        .into_iter()
        .find(|d| {
            d.serial_number()
                .is_some_and(|s| s.eq_ignore_ascii_case(serial))
        })
        .ok_or_else(|| anyhow::anyhow!("{} is no longer in BOOTSEL", escape(serial)))?;
    Ok(Picoboot::new(PicobootNusbIo::open(&info).await?).await?)
}

/// Open the STM32 system bootloader's DFU interface by the exact serial [find_rom] gave, retried briefly: it can be
/// listed before the host's USB stack lets it be opened.
pub(crate) async fn open_stm32(serial: &str) -> Result<Dfuse<DfuseNusbIo>> {
    let info = stm32_usb::find(Some(serial)).await?;
    let io = open_retrying(
        async || DfuseNusbIo::open(&info, 0).await,
        Timeouts::default().open,
    )
    .await?;
    Ok(Dfuse::new(io)?)
}
