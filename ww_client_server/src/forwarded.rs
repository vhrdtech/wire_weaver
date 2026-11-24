//! Who a forwarder's client is (SRV-79): a mount forwarder (SRV-78) sends the peer its front's transport saw with
//! every request it passes on, in [RequestContext::forwarded_for](crate::RequestContext::forwarded_for). A backend
//! takes it only from a connection it trusts to forward (`wire_weaver_server::Server::trust_forwarders`); from any
//! other it is ignored, and the request is the connection's own.

use wire_weaver::prelude::*;
use wire_weaver::{Gid, Peer, PeerKey, Pid};

/// [wire_weaver::Peer] on the wire, as the front's transport saw it; [Peer::Local] has no form here, since a call
/// from the forwarder's own process is the forwarder's.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, Clone, Copy, PartialEq, Eq),
    ww_repr = nib,
    future_version = Newer
)]
pub enum ForwardedPeer {
    /// The front's transport says nothing about its client (USB, RTT, a device's link): [Peer::Unknown].
    Unknown,
    /// A process on the front's machine, from the unix socket's peer credentials: [Peer::Unix].
    Unix {
        uid: UnixUid,
        gid: UnixGid,
        pid: Option<UnixPid>,
    },
    /// A peer that proved a key: [Peer::Key].
    Key(KeyBytes),
    /// A network peer known by its address only: [Peer::Addr].
    Addr { ip: IpBytes, port: Port },
}

/// A user id on the front's machine.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, Clone, Copy, PartialEq, Eq)
)]
pub struct UnixUid(pub UNib32);

/// A group id on the front's machine.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, Clone, Copy, PartialEq, Eq)
)]
pub struct UnixGid(pub UNib32);

/// A process id on the front's machine.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, Clone, Copy, PartialEq, Eq)
)]
pub struct UnixPid(pub UNib32);

/// The 32 bytes of a [PeerKey].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, Clone, Copy, PartialEq, Eq),
    sized
)]
pub struct KeyBytes(pub [u8; 32]);

/// An IP address.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, Clone, Copy, PartialEq, Eq),
    ww_repr = u1,
    no_future_version
)]
pub enum IpBytes {
    V4([u8; 4]),
    V6([u8; 16]),
}

/// A TCP or UDP port.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, Clone, Copy, PartialEq, Eq),
    sized
)]
pub struct Port(pub u16);

/// The context a forwarder sends `request` on with: the client's trace and deadline, and `peer`, the client as the
/// forwarder's transport saw it, in place of whatever the client put in `forwarded_for`; `None` when there is
/// nothing to say. A firmware reads no context: there [Request::context](crate::Request::context) is a flag alone.
#[cfg(feature = "std")]
pub fn forwarded_context(
    peer: Peer,
    request: &crate::Request<'_>,
) -> Option<crate::RequestContext> {
    // one that doesn't read loses its trace and deadline; who the client is never comes from it
    let terms = request
        .context
        .as_ref()
        .and_then(|context| context.read().ok());
    let context = crate::RequestContext {
        trace: terms.and_then(|terms| terms.trace),
        deadline: terms.and_then(|terms| terms.deadline),
        forwarded_for: ForwardedPeer::from_peer(peer),
    };
    let empty =
        context.trace.is_none() && context.deadline.is_none() && context.forwarded_for.is_none();
    (!empty).then_some(context)
}

impl ForwardedPeer {
    /// `peer` as a forwarder sends it on; `None` for [Peer::Local]: the forwarder's own process, which its backend
    /// sees as the connection's peer.
    pub fn from_peer(peer: Peer) -> Option<Self> {
        use core::net::IpAddr;
        Some(match peer {
            Peer::Local => return None,
            Peer::Unix { uid, gid, pid } => ForwardedPeer::Unix {
                uid: UnixUid(UNib32(uid.0)),
                gid: UnixGid(UNib32(gid.0)),
                pid: pid.map(|pid| UnixPid(UNib32(pid.0))),
            },
            Peer::Key(key) => ForwardedPeer::Key(KeyBytes(key.0)),
            Peer::Addr(addr) => ForwardedPeer::Addr {
                ip: match addr.ip() {
                    IpAddr::V4(ip) => IpBytes::V4(ip.octets()),
                    IpAddr::V6(ip) => IpBytes::V6(ip.octets()),
                },
                port: Port(addr.port()),
            },
            // a kind added to `Peer` later: nothing more is known of it here
            _ => ForwardedPeer::Unknown,
        })
    }

    /// The peer this names, as a backend's handlers read it from `cx.call()`.
    pub fn to_peer(self) -> Peer {
        use core::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
        match self {
            // a kind of a newer forwarder: nothing more is known of it here
            ForwardedPeer::Unknown | ForwardedPeer::Newer(_) => Peer::Unknown,
            ForwardedPeer::Unix { uid, gid, pid } => Peer::Unix {
                uid: wire_weaver::Uid(uid.0.0),
                gid: Gid(gid.0.0),
                pid: pid.map(|pid| Pid(pid.0.0)),
            },
            ForwardedPeer::Key(key) => Peer::Key(PeerKey(key.0)),
            ForwardedPeer::Addr { ip, port } => {
                let ip = match ip {
                    IpBytes::V4(ip) => IpAddr::V4(Ipv4Addr::from(ip)),
                    IpBytes::V6(ip) => IpAddr::V6(Ipv6Addr::from(ip)),
                };
                Peer::Addr(SocketAddr::new(ip, port.0))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::net::SocketAddr;
    use wire_weaver::Uid;

    #[test]
    fn every_peer_but_local_round_trips() {
        let peers = [
            Peer::Unknown,
            Peer::Unix {
                uid: Uid(1000),
                gid: Gid(100),
                pid: Some(Pid(4242)),
            },
            Peer::Unix {
                uid: Uid(0),
                gid: Gid(0),
                pid: None,
            },
            Peer::Key(PeerKey([7; 32])),
            Peer::Addr(SocketAddr::from(([192, 168, 1, 20], 7590))),
            Peer::Addr("[fe80::1]:7590".parse().expect("an address")),
        ];
        for peer in peers {
            let forwarded = ForwardedPeer::from_peer(peer).expect("not local");
            let mut buf = [0u8; 64];
            let bytes = forwarded.to_ww_bytes(&mut buf).expect("fits");
            let back = ForwardedPeer::from_ww_bytes(bytes).expect("reads back");
            assert_eq!(back.to_peer(), peer);
        }
        assert_eq!(ForwardedPeer::from_peer(Peer::Local), None);
    }
}
