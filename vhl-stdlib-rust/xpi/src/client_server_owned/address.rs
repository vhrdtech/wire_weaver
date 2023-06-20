use super::NodeId;
use std::net::IpAddr;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Address {
    pub protocol: Protocol,
    pub node_id: NodeId,
    pub wire_format: WireFormat,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Protocol {
    Tcp { addr: IpAddr, port: u16 },
    Ws { addr: IpAddr, port: u16 },
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum WireFormat {
    MessagePack,
    Wfs,
    Wfd,
}
