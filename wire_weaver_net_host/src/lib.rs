use std::net::IpAddr;

mod event_loop_udp;

pub use event_loop_udp::udp_worker;
pub use wire_weaver_client_server::util;
pub use wire_weaver_client_server::{Command, Error, OnError};

pub struct UdpTarget {
    pub addr: IpAddr,
    pub port: u16,
}

#[derive(thiserror::Error, Debug)]
pub enum UdpError {
    #[error("test")]
    Test,
}
