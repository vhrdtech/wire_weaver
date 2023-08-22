use core::fmt::Display;
use std::net::IpAddr;

// #[derive(Copy, Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
// pub struct Address {
//     pub protocol: Protocol,
//     pub node_id: NodeId,
//     // pub wire_format: WireFormat,
// }

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Protocol {
    Tcp { ip_addr: IpAddr, port: u16 },
    Ws { ip_addr: IpAddr, port: u16 },
}

impl Display for Protocol {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Protocol::Tcp { ip_addr, port } => write!(f, "tcp://{ip_addr}:{port}"),
            Protocol::Ws { ip_addr, port } => write!(f, "ws://{ip_addr}:{port}"),
        }
    }
}

// #[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
// pub enum WireFormat {
//     MessagePack,
//     Wfs,
//     Wfd,
// }

// impl Address {
//     pub fn parse<S: AsRef<str>>(_s: S) -> Option<Self> {
//         todo!()
//     }
// }
