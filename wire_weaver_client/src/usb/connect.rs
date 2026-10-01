use crate::config::{ConfigPiece, ValidatedConfig};
use crate::event_loop::command::Command;
use crate::event_loop::transport::Selected;
use anyhow::{Context, anyhow, bail};
use nusb::descriptors::{
    ConfigurationDescriptor, EndpointDescriptor, InterfaceDescriptor, TransferType,
};
use nusb::transfer::Direction;
use nusb::{Device, DeviceInfo, Interface, MaybeFuture};
use tokio::sync::mpsc;
use tracing::trace;

/// USB class, subclass and protocol of the WireWeaver interface, see `wire_weaver_usb_embassy`
const USB_CLASS_VENDOR_SPECIFIC: u8 = 0xFF;
const USB_SUBCLASS_NONE: u8 = 0x00;
const USB_PROTOCOL_WIRE_WEAVER: u8 = 0x37;

pub(crate) struct ConnectOk {
    pub(crate) device: Device,
    pub(crate) interface: Interface,
    pub(crate) transfer_type: TransferType,
    pub(crate) max_packet_size: usize,
    pub(crate) ep_out: u8,
    pub(crate) ep_in: u8,
}

/// WireWeaver interface and its endpoints, as found in the configuration descriptor
#[derive(Debug, PartialEq, Eq)]
struct WwInterface {
    number: u8,
    transfer_type: TransferType,
    max_packet_size: usize,
    ep_out: u8,
    ep_in: u8,
}

pub(crate) fn connect(di: &DeviceInfo) -> anyhow::Result<ConnectOk> {
    trace!("connecting to USB device: {di:?}");
    let device = di.open().wait().map_err(with_hint)?;
    let active_configuration = device
        .active_configuration()
        .context("getting active configuration")?;
    let ww = find_ww_interface(&active_configuration)?;
    trace!("using {ww:x?}");
    let interface = device
        .claim_interface(ww.number)
        .wait()
        .map_err(with_hint)?;
    Ok(ConnectOk {
        device,
        interface,
        transfer_type: ww.transfer_type,
        max_packet_size: ww.max_packet_size,
        ep_out: ww.ep_out,
        ep_in: ww.ep_in,
    })
}

/// Finds the interface with WireWeaver class, subclass and protocol, or, for devices not reporting them,
/// the first interface with a bulk or interrupt IN and OUT endpoint pair.
fn find_ww_interface(config: &ConfigurationDescriptor) -> anyhow::Result<WwInterface> {
    let mut first_usable = None;
    for alt in config.interface_alt_settings() {
        let Some(ww) = endpoint_pair(&alt) else {
            continue;
        };
        if alt.class() == USB_CLASS_VENDOR_SPECIFIC
            && alt.subclass() == USB_SUBCLASS_NONE
            && alt.protocol() == USB_PROTOCOL_WIRE_WEAVER
        {
            return Ok(ww);
        }
        first_usable.get_or_insert(ww);
    }
    first_usable.ok_or(anyhow!(
        "No interface with bulk or interrupt IN and OUT endpoints found in active USB configuration"
    ))
}

/// First IN and OUT endpoints of the same bulk or interrupt type, max packet size is the smaller of the two.
fn endpoint_pair(alt: &InterfaceDescriptor) -> Option<WwInterface> {
    let usable = |ep: &EndpointDescriptor, dir: Direction| {
        ep.direction() == dir
            && matches!(
                ep.transfer_type(),
                TransferType::Bulk | TransferType::Interrupt
            )
    };
    let ep_out = alt.endpoints().find(|ep| usable(ep, Direction::Out))?;
    let ep_in = alt
        .endpoints()
        .find(|ep| usable(ep, Direction::In) && ep.transfer_type() == ep_out.transfer_type())?;
    Some(WwInterface {
        number: alt.interface_number(),
        transfer_type: ep_out.transfer_type(),
        max_packet_size: ep_out.max_packet_size().min(ep_in.max_packet_size()),
        ep_out: ep_out.address(),
        ep_in: ep_in.address(),
    })
}

/// Explain the likely cause of the most common errors
fn with_hint(e: nusb::Error) -> anyhow::Error {
    let hint = match e.kind() {
        nusb::ErrorKind::Busy => {
            "device is already open by another program or another connection in this one"
        }
        nusb::ErrorKind::PermissionDenied if cfg!(target_os = "linux") => {
            "no access to the device, run 'ww udev --help' to see how to install a udev rule"
        }
        _ => return e.into(),
    };
    anyhow!("{e}; {hint}")
}

pub(crate) async fn try_connect(
    c: &ValidatedConfig,
    cmd_rx: &mut Option<mpsc::Receiver<Command>>,
) -> Result<Selected, anyhow::Error> {
    let devices = nusb::list_devices().await.context("listing USB devices")?;
    start_event_loop(c, devices, cmd_rx)
}

pub(crate) fn try_connect_blocking(
    c: &ValidatedConfig,
    cmd_rx: &mut Option<mpsc::Receiver<Command>>,
) -> Result<Selected, anyhow::Error> {
    // TODO: figure out if nusb::list_devices() hangs in other scenarios, apart from enumeration problems on Linux, add timeout
    let devices = nusb::list_devices().wait().context("listing USB devices")?;
    start_event_loop(c, devices, cmd_rx)
}

fn start_event_loop(
    c: &ValidatedConfig,
    devices: impl Iterator<Item = nusb::DeviceInfo>,
    cmd_rx: &mut Option<mpsc::Receiver<Command>>,
) -> Result<Selected, anyhow::Error> {
    let (nusb_info, info) = match select_matching(c, devices)? {
        Ok(matched) => matched,
        Err(unmatched) => return Ok(Selected::NotFound { unmatched }),
    };
    let Some(cmd_rx) = cmd_rx.take() else {
        bail!("Internal error: cmd_rx is None");
    };
    tokio::spawn(async move {
        super::event_loop::usb_worker(cmd_rx).await;
    });
    Ok(Selected::Device {
        handle: Box::new(nusb_info),
        info: Box::new(info),
    })
}

/// Returns the only matching device, or WireWeaver devices that did not match if none did.
#[allow(clippy::type_complexity)]
fn select_matching(
    c: &ValidatedConfig,
    devices: impl Iterator<Item = nusb::DeviceInfo>,
) -> Result<Result<(nusb::DeviceInfo, crate::DeviceInfo), Vec<crate::DeviceInfo>>, crate::Error> {
    // without an explicit VID:PID or path, only consider devices reporting WireWeaver API id
    let by_location = c.pieces.iter().any(|p| {
        matches!(
            p,
            ConfigPiece::UsbVidPid { .. } | ConfigPiece::UsbPath { .. }
        )
    });
    let mut matching = vec![];
    let mut unmatched = vec![];
    for nusb_info in devices.into_iter() {
        let info = crate::DeviceInfo::from(&nusb_info);
        if (by_location || info.api.is_some()) && info.is_matching(&c.pieces) {
            matching.push((nusb_info, info));
        } else if info.api.is_some() {
            unmatched.push(info);
        }
    }
    if let Some(matched) = matching.pop() {
        if matching.is_empty() {
            Ok(Ok(matched))
        } else {
            let mut devices = vec![matched.1];
            devices.extend(matching.drain(..).map(|(_, info)| info));
            Err(crate::Error::AmbiguousDeviceChoice(devices))
        }
    } else {
        Ok(Err(unmatched))
    }
}

/// List connected USB devices that report WireWeaver API id, without opening them.
pub async fn list_devices() -> Result<Vec<crate::DeviceInfo>, anyhow::Error> {
    let mut devices = list_all_devices().await?;
    devices.retain(|d| d.api.is_some());
    Ok(devices)
}

/// List all connected USB devices, including the ones not reporting WireWeaver API id, without opening them.
pub async fn list_all_devices() -> Result<Vec<crate::DeviceInfo>, anyhow::Error> {
    Ok(nusb::list_devices()
        .await?
        .map(|d| crate::DeviceInfo::from(&d))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG_HEAD: [u8; 9] = [9, 2, 0, 0, 3, 1, 0, 0x80, 50];
    /// CDC ACM control interface, interrupt IN only
    const CDC_CONTROL: [u8; 16] = [
        9, 4, 0, 0, 1, 0x02, 0x02, 0x01, 0, //
        7, 5, 0x81, 0x03, 8, 0, 16,
    ];
    /// CDC data interface, bulk IN and OUT
    const CDC_DATA: [u8; 23] = [
        9, 4, 1, 0, 2, 0x0A, 0, 0, 0, //
        7, 5, 0x02, 0x02, 64, 0, 0, //
        7, 5, 0x82, 0x02, 64, 0, 0,
    ];
    /// WireWeaver interface, interrupt IN and OUT, max packet size 1024 / 512
    const WW: [u8; 23] = [
        9, 4, 2, 0, 2, 0xFF, 0, 0x37, 0, //
        7, 5, 0x83, 0x03, 0x00, 0x04, 1, //
        7, 5, 0x03, 0x03, 0x00, 0x02, 1,
    ];

    fn config(interfaces: &[&[u8]]) -> Vec<u8> {
        let mut buf = CONFIG_HEAD.to_vec();
        for i in interfaces {
            buf.extend_from_slice(i);
        }
        let len = buf.len() as u16;
        buf[2..4].copy_from_slice(&len.to_le_bytes());
        buf[4] = interfaces.len() as u8;
        buf
    }

    #[test]
    fn finds_ww_interface_in_composite_device() {
        let buf = config(&[&CDC_CONTROL, &CDC_DATA, &WW]);
        let config = ConfigurationDescriptor::new(&buf).unwrap();
        assert_eq!(
            find_ww_interface(&config).unwrap(),
            WwInterface {
                number: 2,
                transfer_type: TransferType::Interrupt,
                max_packet_size: 512,
                ep_out: 0x03,
                ep_in: 0x83,
            }
        );
    }

    #[test]
    fn falls_back_to_first_endpoint_pair() {
        let buf = config(&[&CDC_CONTROL, &CDC_DATA]);
        let config = ConfigurationDescriptor::new(&buf).unwrap();
        assert_eq!(
            find_ww_interface(&config).unwrap(),
            WwInterface {
                number: 1,
                transfer_type: TransferType::Bulk,
                max_packet_size: 64,
                ep_out: 0x02,
                ep_in: 0x82,
            }
        );
    }

    #[test]
    fn no_endpoint_pair() {
        let buf = config(&[&CDC_CONTROL]);
        let config = ConfigurationDescriptor::new(&buf).unwrap();
        assert!(find_ww_interface(&config).is_err());
    }
}
