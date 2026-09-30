// use crate::{
//     DeviceInfo,
//     device_info::{ConnectionInfo, DeviceApiInfo},
// };

pub enum TraceEvent {
    /// Request being sent to a remote device
    Request {
        bytes: Vec<u8>,
    },

    /// Connected to a remote device
    Connected {
        // info: Box<DeviceInfo>,
        // api: Box<DeviceApiInfo>,
    },
    /// Event from a remote device
    Event {
        bytes: Vec<u8>,
    },
    /// Device disconnected
    Disconnected {
        reason: String,
        keep_streams: bool,
    },

    Error {
        reason: String,
    },
}

// Ensure the event is not too big as there can be a lot of them
const _: () = {
    assert!(size_of::<TraceEvent>() < 36);
};
