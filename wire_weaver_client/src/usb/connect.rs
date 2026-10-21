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
use tracing::{trace, warn};

/// USB class and subclass of the WireWeaver interface, see `wire_weaver_usb_embassy`; the protocol tells the framing,
/// see [ww_link::USB_INTERFACE_PROTOCOL].
const USB_CLASS_VENDOR_SPECIFIC: u8 = 0xFF;
const USB_SUBCLASS_NONE: u8 = 0x00;

pub(crate) struct ConnectOk {
    pub(crate) device: Device,
    pub(crate) interface: Interface,
    pub(crate) transfer_type: TransferType,
    pub(crate) max_packet_size: usize,
    pub(crate) ep_out: u8,
    pub(crate) ep_in: u8,
    /// The interface says it uses [ww_link::UsbSeqFraming]
    pub(crate) seq_prefix: bool,
    /// Held while the interface is claimed (CLT-38); `None` only when the registry itself couldn't be written
    /// to (a broken runtime dir), never when another holder has it (that refuses the connection instead).
    pub(crate) claim: Option<crate::holders::Claim>,
}

/// WireWeaver interface and its endpoints, as found in the configuration descriptor
#[derive(Debug, PartialEq, Eq)]
struct WwInterface {
    number: u8,
    transfer_type: TransferType,
    max_packet_size: usize,
    ep_out: u8,
    ep_in: u8,
    seq_prefix: bool,
}

/// A USB device selected by `connect()`, with what the client says about itself and how it holds the device, for the lock
/// file ([ClientConfig::holder_mode](crate::ClientConfig::holder_mode)).
pub(crate) struct UsbTarget {
    pub(crate) info: DeviceInfo,
    pub(crate) meta: Option<Box<crate::client_meta::ClientMetaOwned>>,
    pub(crate) mode: crate::holders::Mode,
}

pub(crate) fn connect(target: &UsbTarget) -> anyhow::Result<ConnectOk> {
    let di = &target.info;
    trace!("connecting to USB device: {di:?}");
    let device = di.open().wait().map_err(with_hint)?;
    let active_configuration = device
        .active_configuration()
        .context("getting active configuration")?;
    let ww = find_ww_interface(&active_configuration)?;
    trace!("using {ww:x?}");

    // Claim our own lock before claiming the interface (CLT-38): a busy error from another WireWeaver holder
    // then names who, instead of nusb's generic one.
    let key = crate::holders::key_for(crate::holders::DeviceRef::Usb {
        vid: di.vendor_id(),
        pid: di.product_id(),
        serial: di.serial_number().unwrap_or_default(),
        iface: Some(ww.number),
    });
    let claim = match crate::holders::claim_with(
        &key,
        target.mode,
        target.meta.as_deref(),
    ) {
        Ok(claim) => Some(claim),
        Err(crate::holders::ClaimError::HeldBy(holder)) => {
            bail!(
                "device is already open by another WireWeaver program or connection; {holder}{}",
                crate::hub::busy_hint(&holder)
            );
        }
        Err(crate::holders::ClaimError::Io(e)) => {
            warn!("could not record holding {key} ({e:#}), continuing without it");
            None
        }
    };

    let interface = device
        .claim_interface(ww.number)
        .wait()
        .map_err(|e| with_claim_hint(e, di))?;
    Ok(ConnectOk {
        device,
        interface,
        transfer_type: ww.transfer_type,
        max_packet_size: ww.max_packet_size,
        ep_out: ww.ep_out,
        ep_in: ww.ep_in,
        seq_prefix: ww.seq_prefix,
        claim,
    })
}

/// Finds the interface with WireWeaver class, subclass and protocol, or, for devices not reporting them,
/// the first interface with a bulk or interrupt IN and OUT endpoint pair.
fn find_ww_interface(config: &ConfigurationDescriptor) -> anyhow::Result<WwInterface> {
    let mut first_usable = None;
    for alt in config.interface_alt_settings() {
        let Some(mut ww) = endpoint_pair(&alt) else {
            continue;
        };
        if alt.class() == USB_CLASS_VENDOR_SPECIFIC && alt.subclass() == USB_SUBCLASS_NONE {
            match alt.protocol() {
                ww_link::USB_INTERFACE_PROTOCOL => return Ok(ww),
                ww_link::USB_INTERFACE_PROTOCOL_SEQ => {
                    ww.seq_prefix = true;
                    return Ok(ww);
                }
                _ => {}
            }
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
        seq_prefix: false,
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

/// Same as [with_hint], but a busy claim already passed our own lock (CLT-38: nothing else speaking WireWeaver
/// holds this device), so the holder is a program that never wrote a lock file. On Linux, named by scanning
/// `/proc` for whoever has the device node open; elsewhere, just "another program".
fn with_claim_hint(e: nusb::Error, di: &DeviceInfo) -> anyhow::Error {
    let _ = di; // only used on Linux, see below
    if e.kind() != nusb::ErrorKind::Busy {
        return with_hint(e);
    }
    #[cfg(target_os = "linux")]
    {
        let node = crate::holders::usb_device_node(di.busnum(), di.device_address());
        if let Some(who) = crate::holders::describe_holder_of_node(&node) {
            return anyhow!("{e}; held by {who}");
        }
    }
    anyhow!("{e}; device is already open by another program")
}

pub(crate) async fn try_connect(
    c: &ValidatedConfig,
    cmd_rx: &mut Option<mpsc::Receiver<Command>>,
) -> Result<Selected, anyhow::Error> {
    let devices = nusb::list_devices().await.context("listing USB devices")?;
    let (nusb_info, info) = match select_matching(c, devices)? {
        Ok(matched) => matched,
        Err(unmatched) => return Ok(Selected::NotFound { unmatched }),
    };
    #[cfg(feature = "hub")]
    let route = crate::hub::route(c, &hub_key(&nusb_info)).await?;
    #[cfg(not(feature = "hub"))]
    let route = no_hub(c)?;
    start_event_loop(c, nusb_info, info, route, cmd_rx)
}

pub(crate) fn try_connect_blocking(
    c: &ValidatedConfig,
    cmd_rx: &mut Option<mpsc::Receiver<Command>>,
) -> Result<Selected, anyhow::Error> {
    // TODO: figure out if nusb::list_devices() hangs in other scenarios, apart from enumeration problems on Linux, add timeout
    let devices = nusb::list_devices().wait().context("listing USB devices")?;
    let (nusb_info, info) = match select_matching(c, devices)? {
        Ok(matched) => matched,
        Err(unmatched) => return Ok(Selected::NotFound { unmatched }),
    };
    #[cfg(feature = "hub")]
    let route = {
        // the tunnel's socket belongs to the runtime the event loop runs on, which a multi-thread one drives
        // while this thread waits; a current-thread runtime only runs inside its own block_on
        let handle = tokio::runtime::Handle::current();
        if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread {
            handle.block_on(crate::hub::route(c, &hub_key(&nusb_info)))?
        } else if c.hub_mode == crate::hub::HubMode::Required {
            bail!("connect_blocking() reaches the ww hub on a multi-thread tokio runtime only");
        } else {
            crate::hub::Route::Direct {
                note: Some("connect_blocking() on a current-thread runtime: directly".into()),
            }
        }
    };
    #[cfg(not(feature = "hub"))]
    let route = no_hub(c)?;
    start_event_loop(c, nusb_info, info, route, cmd_rx)
}

/// The device's key in the hub's registry: `usb:<vid>:<pid>:<serial>`, without the interface number.
#[cfg(feature = "hub")]
fn hub_key(di: &DeviceInfo) -> String {
    crate::holders::key_for(crate::holders::DeviceRef::Usb {
        vid: di.vendor_id(),
        pid: di.product_id(),
        serial: di.serial_number().unwrap_or_default(),
        iface: None,
    })
}

/// Without the `hub` feature every connection is direct; one that requires the hub fails.
#[cfg(not(feature = "hub"))]
fn no_hub(c: &ValidatedConfig) -> anyhow::Result<()> {
    if c.hub_mode == crate::hub::HubMode::Required {
        bail!(
            "the ww hub is required (WW_HUB=required or HubMode::Required), but wire_weaver_client is built \
             without the `hub` feature"
        );
    }
    Ok(())
}

#[cfg(feature = "hub")]
type Route = crate::hub::Route;
#[cfg(not(feature = "hub"))]
type Route = ();

fn start_event_loop(
    c: &ValidatedConfig,
    nusb_info: DeviceInfo,
    info: crate::DeviceInfo,
    route: Route,
    cmd_rx: &mut Option<mpsc::Receiver<Command>>,
) -> Result<Selected, anyhow::Error> {
    let Some(cmd_rx) = cmd_rx.take() else {
        bail!("Internal error: cmd_rx is None");
    };
    #[cfg(feature = "hub")]
    if let crate::hub::Route::Hub(tunnel) = route {
        tokio::spawn(crate::local_socket::worker(cmd_rx));
        let mut info = info;
        info.location = format!("{} through the ww hub", info.location);
        return Ok(Selected::Device {
            handle: tunnel,
            info: Box::new(info),
        });
    }
    #[cfg(not(feature = "hub"))]
    let () = route;
    tokio::spawn(async move {
        super::event_loop::usb_worker(cmd_rx).await;
    });
    Ok(Selected::Device {
        handle: Box::new(UsbTarget {
            info: nusb_info,
            meta: c.client_meta.clone(),
            mode: c.holder_mode,
        }),
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
                seq_prefix: false,
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
                seq_prefix: false,
            }
        );
    }

    #[test]
    fn interface_protocol_tells_the_framing() {
        let mut ww = WW;
        ww[7] = ww_link::USB_INTERFACE_PROTOCOL_SEQ;
        let buf = config(&[&CDC_DATA, &ww]);
        let config = ConfigurationDescriptor::new(&buf).unwrap();
        let found = find_ww_interface(&config).unwrap();
        assert_eq!((found.number, found.seq_prefix), (2, true));
    }

    #[test]
    fn no_endpoint_pair() {
        let buf = config(&[&CDC_CONTROL]);
        let config = ConfigurationDescriptor::new(&buf).unwrap();
        assert!(find_ww_interface(&config).is_err());
    }
}
