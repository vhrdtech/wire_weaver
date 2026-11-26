//! The example messages, each in its own module with its plain and tuned types and every format's schema.

use crate::rows::Message;

mod can;
mod config;
mod firmware;
mod gps;
mod log;
mod rpc;
mod state;
mod telemetry;

/// Every message, in the order the page shows them.
pub fn all() -> Vec<Message> {
    vec![
        rpc::message(),
        telemetry::message(),
        log::message(),
        firmware::message(),
        config::message(),
        gps::message(),
        can::message(),
        state::message(),
    ]
}
