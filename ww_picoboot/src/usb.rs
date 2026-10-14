//! PICOBOOT on real USB devices, through nusb.

use std::time::Duration;

use nusb::descriptors::TransferType;
use nusb::transfer::{
    Buffer, Bulk, ControlIn, ControlOut, ControlType, Direction, In, Out, Recipient, TransferError,
};
use nusb::{DeviceInfo, Endpoint, Interface};

use crate::io::{IoError, PicobootIo};
use crate::proto;
use crate::{Error, Picoboot};

/// Bulk transfers: long enough for a sector erase or a slow flash.
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(10);
const CONTROL_TIMEOUT: Duration = Duration::from_secs(1);

/// Whether `info` is an RP2040 or RP2350 boot ROM in BOOTSEL mode.
pub fn is_bootsel(info: &DeviceInfo) -> bool {
    info.vendor_id() == proto::RPI_VID
        && matches!(
            info.product_id(),
            proto::RP2040_BOOTSEL_PID | proto::RP2350_BOOTSEL_PID
        )
}

/// RP2040 / RP2350 boot ROMs in BOOTSEL mode connected now.
pub async fn list() -> Result<Vec<DeviceInfo>, Error> {
    Ok(nusb::list_devices()
        .await
        .map_err(|e| Error::Other(format!("listing USB devices: {e}")))?
        .filter(is_bootsel)
        .collect())
}

/// Open the BOOTSEL device with USB serial number `serial` (ignoring case), or the only one connected for `None`.
pub async fn open(serial: Option<&str>) -> Result<Picoboot<NusbIo>, Error> {
    let info = find(serial).await?;
    Picoboot::new(NusbIo::open(&info).await?).await
}

/// The BOOTSEL device with USB serial number `serial` (ignoring case), or the only one connected for `None`.
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
                    .is_some_and(|ds| ds.eq_ignore_ascii_case(s))
            })
        })
        .collect();
    match matching.len() {
        1 => Ok(matching.pop().unwrap().clone()),
        0 => Err(Error::Other(match serial {
            Some(s) => format!(
                "no BOOTSEL device with serial {s} (present: {})",
                describe(&devices.iter().collect::<Vec<_>>())
            ),
            None => "no BOOTSEL device found".into(),
        })),
        _ => Err(Error::Other(format!(
            "several BOOTSEL devices, choose one by serial: {}",
            describe(&matching)
        ))),
    }
}

/// The PICOBOOT interface of a BOOTSEL device.
pub struct NusbIo {
    interface: Interface,
    ep_out: Endpoint<Bulk, Out>,
    ep_in: Endpoint<Bulk, In>,
}

impl NusbIo {
    /// [NusbIo::open] the BOOTSEL device with USB serial number `serial` (ignoring case).
    pub async fn open_by_serial(serial: &str) -> Result<Self, Error> {
        Self::open(&find(Some(serial)).await?).await
    }

    /// Open `info` and claim its PICOBOOT interface (vendor class, two bulk endpoints).
    pub async fn open(info: &DeviceInfo) -> Result<Self, Error> {
        let other = |what: &str, e: nusb::Error| Error::Other(format!("{what}: {e}"));
        let device = info.open().await.map_err(|e| other("opening device", e))?;
        let config = device
            .active_configuration()
            .map_err(|e| Error::Other(format!("reading configuration: {e}")))?;
        let mut found = None;
        for alt in config.interface_alt_settings() {
            if alt.class() != proto::INTERFACE_CLASS {
                continue;
            }
            let ep = |dir: Direction| {
                alt.endpoints()
                    .find(|e| e.direction() == dir && e.transfer_type() == TransferType::Bulk)
                    .map(|e| e.address())
            };
            if let (Some(out), Some(inn)) = (ep(Direction::Out), ep(Direction::In)) {
                found = Some((alt.interface_number(), out, inn));
                break;
            }
        }
        let Some((number, out, inn)) = found else {
            return Err(Error::Other(
                "no PICOBOOT interface (vendor class with two bulk endpoints); disabled by the reboot flags or OTP?".into(),
            ));
        };
        let interface = device
            .claim_interface(number)
            .await
            .map_err(|e| other("claiming PICOBOOT interface", e))?;
        let ep_out = interface
            .endpoint::<Bulk, Out>(out)
            .map_err(|e| other("opening OUT endpoint", e))?;
        let ep_in = interface
            .endpoint::<Bulk, In>(inn)
            .map_err(|e| other("opening IN endpoint", e))?;
        Ok(NusbIo {
            interface,
            ep_out,
            ep_in,
        })
    }
}

fn io_err(e: TransferError) -> IoError {
    match e {
        TransferError::Stall => IoError::Stall,
        TransferError::Disconnected => IoError::Disconnected,
        TransferError::Cancelled => IoError::Timeout,
        e => IoError::Other(e.to_string()),
    }
}

/// Wait for the transfer submitted on `ep`, cancel it after [TRANSFER_TIMEOUT].
async fn complete<D: nusb::transfer::EndpointDirection>(
    ep: &mut Endpoint<Bulk, D>,
) -> Result<(Buffer, usize), IoError> {
    let completion = match tokio::time::timeout(TRANSFER_TIMEOUT, ep.next_complete()).await {
        Ok(c) => c,
        Err(_) => {
            ep.cancel_all();
            _ = ep.next_complete().await;
            return Err(IoError::Timeout);
        }
    };
    completion.status.map_err(io_err)?;
    Ok((completion.buffer, completion.actual_len))
}

impl PicobootIo for NusbIo {
    async fn bulk_out(&mut self, data: &[u8]) -> Result<(), IoError> {
        let mut buf = self.ep_out.allocate(data.len());
        buf.extend_from_slice(data);
        self.ep_out.submit(buf);
        complete(&mut self.ep_out).await.map(drop)
    }

    async fn bulk_in(&mut self, len: usize) -> Result<Vec<u8>, IoError> {
        let mps = self.ep_in.max_packet_size();
        let requested = len.max(1).div_ceil(mps) * mps;
        self.ep_in.submit(self.ep_in.allocate(requested));
        let (buf, actual) = complete(&mut self.ep_in).await?;
        if actual != len {
            return Err(IoError::Other(format!(
                "received {actual} bytes instead of {len}"
            )));
        }
        let mut data = buf.into_vec();
        data.truncate(actual);
        Ok(data)
    }

    async fn reset_interface(&mut self) -> Result<(), IoError> {
        // as picotool: clear the host side of a stall (CLEAR_FEATURE ENDPOINT_HALT), then reset the interface
        self.ep_out
            .clear_halt()
            .await
            .map_err(|e| IoError::Other(e.to_string()))?;
        self.ep_in
            .clear_halt()
            .await
            .map_err(|e| IoError::Other(e.to_string()))?;
        self.interface
            .control_out(
                ControlOut {
                    control_type: ControlType::Vendor,
                    recipient: Recipient::Interface,
                    request: proto::IF_RESET,
                    value: 0,
                    index: self.interface.interface_number() as u16,
                    data: &[],
                },
                CONTROL_TIMEOUT,
            )
            .await
            .map_err(io_err)
    }

    async fn cmd_status(&mut self) -> Result<[u8; 16], IoError> {
        let data = self
            .interface
            .control_in(
                ControlIn {
                    control_type: ControlType::Vendor,
                    recipient: Recipient::Interface,
                    request: proto::IF_CMD_STATUS,
                    value: 0,
                    index: self.interface.interface_number() as u16,
                    length: 16,
                },
                CONTROL_TIMEOUT,
            )
            .await
            .map_err(io_err)?;
        data.try_into()
            .map_err(|d: Vec<u8>| IoError::Other(format!("status of {} bytes", d.len())))
    }
}
