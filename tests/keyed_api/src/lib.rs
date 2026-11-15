//! Keyed resource arrays (API-67): a packet dissector whose flows a client addresses by their key, not by the index
//! the device happened to give them. `keyed!(flow by FlowKey)` follows the array `flow[]` it keys.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;
use ww_si::Bytes;
use ww_si::origins::DeviceMicros;

#[ww_api_root]
trait Dissector {
    /// Flows in the order the device met them; an index is reused after its flow is removed.
    ww_impl!(flow[]: Flow);
    /// Flows by protocol, endpoints and the time they were first seen.
    keyed!(flow by FlowKey);
    /// Drops every flow.
    fn clear();
}

#[ww_trait]
trait Flow {
    /// Bytes of every packet of the flow so far.
    property!(ro bytes: Bytes<u64>);
    /// Each packet's payload as it is seen.
    stream!(packets: [u8]);
}

/// What identifies a flow across captures: the same traffic gives the same key whatever index it gets.
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone))]
pub struct FlowKey {
    pub proto: Proto,
    pub src: Endpoint,
    pub dst: Endpoint,
    pub first_seen: DeviceMicros<u64>,
}

/// Transport protocol of a flow.
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone, Copy), ww_repr = nib)]
pub enum Proto {
    Tcp,
    Udp,
}

/// One end of a flow.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, PartialEq, Eq, Clone, Copy)
)]
pub struct Endpoint {
    /// IPv4 address bytes as on the wire, a raw protocol field.
    pub addr: [u8; 4],
    pub port: Port,
}

/// A TCP or UDP port number.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, PartialEq, Eq, Clone, Copy)
)]
pub struct Port(pub u16);
