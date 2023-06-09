use crate::node::async_std::NodeError;
use std::net::SocketAddr;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RemoteNodeAddr {
    Tcp(SocketAddr),
    Ws(SocketAddr),
    // Can,
    // Usb,
    // Ipc,
}

impl RemoteNodeAddr {
    pub fn parse(addr: &str) -> Result<Self, NodeError> {
        return if addr.starts_with("tcp://") {
            let ip_addr = addr
                .strip_prefix("tcp://")
                .ok_or(NodeError::InvalidNodeAddr)?;
            Ok(RemoteNodeAddr::Tcp(ip_addr.parse()?))
        } else {
            let ip_addr = addr
                .strip_prefix("ws://")
                .ok_or(NodeError::InvalidNodeAddr)?;
            Ok(RemoteNodeAddr::Ws(ip_addr.parse()?))
        };
    }
}
