use std::any::Any;

use wire_weaver_client_common::Error;
use wire_weaver_usb_host::nusb::{self, MaybeFuture};

use crate::{
    device_info::DeviceInfo,
    options::{OptionPiece, Options},
};

pub(crate) fn connect_blocking(f: &Options) -> Result<OptionPiece<()>, Error> {
    // TODO: figure out if nusb::list_devices() hangs in other scenarios, apart from enumeration problems on Linux, add timeout
    let devices = nusb::list_devices()
        .wait()
        .map_err(|e| Error::Other(e.to_string()))?;
    let mut matching = vec![];
    for nusb_info in devices {
        let info = DeviceInfo::from(&nusb_info);
        let vid_pid_match = vid_pid_match(&f.pieces, &nusb_info);
        let chain_match = port_chain_match(&f.pieces, &nusb_info);
        if vid_pid_match || chain_match || info.is_matching(f) {
            matching.push((nusb_info, info));
        }
    }
    if let Some((nusb_info, info)) = matching.pop() {
        if matching.is_empty() {
            // let any: Box<dyn Any> = Box::new(nusb_info);
            let dev = nusb_info.open().wait().unwrap();
            let any: Box<dyn Any> = Box::new(dev);
            Ok(Some(()))
        } else {
            Err(Error::Other(
                "More than one device matched the provided filter".into(),
            ))
        }
    } else {
        Ok(None)
    }
}

fn vid_pid_match(pieces: &[OptionPiece], info: &nusb::DeviceInfo) -> bool {
    pieces.iter().any(|p| {
        if let OptionPiece::UsbVidPid { vid, pid } = p {
            info.vendor_id() == *vid && info.product_id() == *pid
        } else {
            false
        }
    })
}

fn port_chain_match(pieces: &[OptionPiece], info: &nusb::DeviceInfo) -> bool {
    pieces.iter().any(|p| {
        if let OptionPiece::UsbPath { bus_id, port_chain } = p {
            info.bus_id() == bus_id && info.port_chain() == port_chain
        } else {
            false
        }
    })
}

impl From<&nusb::DeviceInfo> for DeviceInfo {
    fn from(info: &nusb::DeviceInfo) -> Self {
        let manufacturer = info.manufacturer_string().unwrap_or_default().to_string();
        let product = info.product_string().unwrap_or_default().to_string();
        let raw_serial = info.serial_number().unwrap_or_default().to_string();
        // TODO: parse serials, versions, labels, etc.
        DeviceInfo {
            manufacturer,
            product,
            serials: vec![raw_serial],
            user_label: "".into(),
            api: None,
        }
    }
}
