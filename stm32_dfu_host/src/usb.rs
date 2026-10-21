//! The system bootloader's DFU on real USB devices, through nusb.

use std::time::Duration;

use nusb::descriptors::language_id::US_ENGLISH;
use nusb::transfer::{ControlIn, ControlOut, ControlType, Recipient, TransferError};
use nusb::{DeviceInfo, Interface};

use crate::io::{DfuIo, IoError};
use crate::proto;
use crate::{Dfuse, Error};

/// A DFU request that erases a 128 KB sector may take this long to be answered.
const CONTROL_TIMEOUT: Duration = Duration::from_secs(5);

/// Whether `info` is an STM32 system bootloader in DFU mode.
pub fn is_rom_dfu(info: &DeviceInfo) -> bool {
    info.vendor_id() == proto::ST_VID && info.product_id() == proto::ROM_DFU_PID
}

/// STM32 system bootloaders in DFU mode connected now.
pub async fn list() -> Result<Vec<DeviceInfo>, Error> {
    Ok(nusb::list_devices()
        .await
        .map_err(|e| Error::Other(format!("listing USB devices: {e}")))?
        .filter(is_rom_dfu)
        .collect())
}

/// The device with USB serial number `serial` (ignoring case, part of it is enough when only one matches), or the
/// only one connected for `None`.
pub async fn find(serial: Option<&str>) -> Result<DeviceInfo, Error> {
    let devices = list().await?;
    let describe = |ds: &[&DeviceInfo]| {
        ds.iter()
            .map(|d| d.serial_number().unwrap_or("no serial").to_string())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut matching: Vec<_> = devices
        .iter()
        .filter(|d| {
            serial.is_none_or(|s| {
                d.serial_number()
                    .is_some_and(|ds| ds.to_lowercase().contains(&s.to_lowercase()))
            })
        })
        .collect();
    match matching.len() {
        1 => Ok(matching.pop().unwrap().clone()),
        0 => Err(Error::Other(match serial {
            Some(s) => format!(
                "no STM32 DFU device with serial {s} (present: {})",
                describe(&devices.iter().collect::<Vec<_>>())
            ),
            None => "no STM32 DFU device (0483:df11) found".into(),
        })),
        _ => Err(Error::Other(format!(
            "several STM32 DFU devices, choose one by serial: {}",
            describe(&matching)
        ))),
    }
}

/// Open the device with `serial` and its first alternate setting (internal flash).
pub async fn open(serial: Option<&str>) -> Result<Dfuse<NusbIo>, Error> {
    let info = find(serial).await?;
    Dfuse::new(NusbIo::open(&info, 0).await?)
}

/// The alternate settings of the DFU interface and their strings, the memory layouts.
pub async fn alt_settings(info: &DeviceInfo) -> Result<Vec<(u8, String)>, Error> {
    let io = NusbIo::open(info, 0).await?;
    Ok(io.alts.clone())
}

/// One alternate setting of the DFU interface of a device.
pub struct NusbIo {
    interface: Interface,
    layout: String,
    transfer_size: usize,
    alts: Vec<(u8, String)>,
}

fn io_err(e: TransferError) -> IoError {
    match e {
        TransferError::Stall => IoError::Stall,
        TransferError::Disconnected => IoError::Disconnected,
        TransferError::Cancelled => IoError::Timeout,
        e => IoError::Other(e.to_string()),
    }
}

impl NusbIo {
    /// [NusbIo::open] the device with USB serial number `serial`.
    pub async fn open_by_serial(serial: &str, alt: u8) -> Result<Self, Error> {
        Self::open(&find(Some(serial)).await?, alt).await
    }

    /// Open `info`, claim its DFU interface and select alternate setting `alt`.
    pub async fn open(info: &DeviceInfo, alt: u8) -> Result<Self, Error> {
        let other = |what: &str, e: &dyn std::fmt::Display| Error::Other(format!("{what}: {e}"));
        let device = info.open().await.map_err(|e| other("opening device", &e))?;
        let config = device
            .active_configuration()
            .map_err(|e| other("reading configuration", &e))?;
        let mut number = None;
        let mut alts = vec![];
        let mut transfer_size = proto::DEFAULT_TRANSFER_SIZE;
        for desc in config.interface_alt_settings() {
            if desc.class() != proto::DFU_CLASS || desc.subclass() != proto::DFU_SUBCLASS {
                continue;
            }
            number.get_or_insert(desc.interface_number());
            if number != Some(desc.interface_number()) {
                continue;
            }
            let name = match desc.string_index() {
                Some(i) => device
                    .get_string_descriptor(i, US_ENGLISH, CONTROL_TIMEOUT)
                    .await
                    .map_err(|e| other("reading the alt setting string", &e))?,
                None => String::new(),
            };
            alts.push((desc.alternate_setting(), name));
            // DFU functional descriptor: bLength 9, type 0x21, bmAttributes, wDetachTimeOut, wTransferSize, bcdDFU
            for d in desc.descriptors() {
                if d.descriptor_type() == proto::DFU_FUNCTIONAL_DESCRIPTOR && d.len() >= 7 {
                    transfer_size = u16::from_le_bytes([d[5], d[6]]) as usize;
                }
            }
        }
        let number = number
            .ok_or_else(|| Error::Other("no DFU interface (class 0xFE, subclass 1)".into()))?;
        let layout = alts
            .iter()
            .find(|(a, _)| *a == alt)
            .map(|(_, n)| n.clone())
            .ok_or_else(|| Error::Other(format!("no alternate setting {alt}")))?;
        let interface = device
            .detach_and_claim_interface(number)
            .await
            .map_err(|e| other("claiming the DFU interface", &e))?;
        interface
            .set_alt_setting(alt)
            .await
            .map_err(|e| other("selecting the alternate setting", &e))?;
        Ok(NusbIo {
            interface,
            layout,
            transfer_size,
            alts,
        })
    }
}

impl DfuIo for NusbIo {
    async fn control_out(&mut self, request: u8, value: u16, data: &[u8]) -> Result<(), IoError> {
        self.interface
            .control_out(
                ControlOut {
                    control_type: ControlType::Class,
                    recipient: Recipient::Interface,
                    request,
                    value,
                    index: self.interface.interface_number() as u16,
                    data,
                },
                CONTROL_TIMEOUT,
            )
            .await
            .map_err(io_err)
    }

    async fn control_in(
        &mut self,
        request: u8,
        value: u16,
        len: usize,
    ) -> Result<Vec<u8>, IoError> {
        self.interface
            .control_in(
                ControlIn {
                    control_type: ControlType::Class,
                    recipient: Recipient::Interface,
                    request,
                    value,
                    index: self.interface.interface_number() as u16,
                    length: len as u16,
                },
                CONTROL_TIMEOUT,
            )
            .await
            .map_err(io_err)
    }

    fn layout_string(&self) -> &str {
        &self.layout
    }

    fn transfer_size(&self) -> usize {
        self.transfer_size
    }
}
