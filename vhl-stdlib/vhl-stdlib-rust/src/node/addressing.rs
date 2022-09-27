use std::net::SocketAddr;
use crate::node::async_std::NodeError;

#[derive(Debug)]
pub enum RemoteNodeAddr {
    Tcp(SocketAddr),
    // Can,
    // Usb,
    // Ipc,
}

impl RemoteNodeAddr {
    pub fn parse(addr: &str) -> Result<Self, NodeError> {
        let ip_addr = addr.strip_prefix("tcp://").ok_or(NodeError::InvalidNodeAddr)?;
        Ok(RemoteNodeAddr::Tcp(ip_addr.parse()?))
    }
}