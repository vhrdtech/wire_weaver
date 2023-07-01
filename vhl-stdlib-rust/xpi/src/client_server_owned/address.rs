use std::net::IpAddr;

// #[derive(Copy, Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
// pub struct Address {
//     pub protocol: Protocol,
//     pub node_id: NodeId,
//     // pub wire_format: WireFormat,
// }

#[derive(Copy, Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Protocol {
    Tcp { ip_addr: IpAddr, port: u16 },
    Ws { ip_addr: IpAddr, port: u16 },
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
