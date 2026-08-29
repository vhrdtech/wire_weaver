pub enum TraceEvent {
    /// Request being sent to a remote device
    Request {
        bytes: Vec<u8>,
    },

    /// Connected to a remote device
    Connected {
        info: Box<ConnectionInfo>,
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
// const _: () = {
//     assert!(size_of::<TraceEvent>() < 36);
// };

pub struct ConnectionInfo {
    // /// API model version, e.g., ww_client_server
    // pub remote_api_model: FullVersionOwned,
    // pub local_api_model: FullVersionOwned,
    // /// Link in use, e.g., wire_weaver_usb_link
    // pub link: FullVersionOwned,
    // /// User API version
    // pub user_api: FullVersionOwned,
    // pub max_remote_message_size: usize,
    // pub max_local_message_size: usize,
}
