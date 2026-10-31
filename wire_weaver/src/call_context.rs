//! Who asked for a call, and under which terms: [CallContext], the same for a request served by a device or a host
//! service and for a call in the same process (SRV-48).

use exact_units::Millis;

use crate::Priority;

/// Who asked and under which terms: one value per call, `no_std`, `Copy`, no allocator.
///
/// A server handler reads it from its `Context` ([Context::call](crate::Context::call)), a function of an API's local
/// trait (CLT-57) takes it as its first argument (`cx: &CallContext`), so a policy check or a log line reads the same
/// whether the call came over a link or from the same process. A caller in the same process passes
/// [CallContext::local]; a server builds one from the transport and the request.
///
/// Fields are public for reading and for changing a copy (`let mut cx = CallContext::local(); cx.deadline = ..`);
/// new ones may be added, so a value is made with [local](Self::local) or [new](Self::new).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[non_exhaustive]
pub struct CallContext {
    /// Sequence number of the request, 0 if the caller doesn't expect a reply and for calls in the same process.
    pub seq: u32,
    /// How soon the call should be served and answered (CLT-50), [Priority::NORMAL] unless set.
    pub priority: Priority,
    /// Who asked, as the transport saw it: never from the caller's own words.
    pub peer: Peer,
    /// W3C trace context of the caller, if it sent one.
    pub trace: Option<TraceContext>,
    /// What was left of the caller's time budget when the call arrived, None for no limit.
    pub deadline: Option<Millis<u32>>,
    /// The connection the call came on, to tell the callers of one server apart.
    pub connection: ConnectionId,
}

impl CallContext {
    /// A call from the same process: [Peer::Local], no deadline and no trace, [Priority::NORMAL], `seq` 0.
    pub const fn local() -> Self {
        CallContext {
            seq: 0,
            priority: Priority::NORMAL,
            peer: Peer::Local,
            trace: None,
            deadline: None,
            connection: ConnectionId::LOCAL,
        }
    }

    /// A request `seq` from a peer that the transport doesn't identify ([Peer::Unknown]), as on a device:
    /// [Priority::NORMAL], no deadline and no trace.
    pub const fn new(seq: u32) -> Self {
        CallContext {
            seq,
            priority: Priority::NORMAL,
            peer: Peer::Unknown,
            trace: None,
            deadline: None,
            connection: ConnectionId::LOCAL,
        }
    }

    pub const fn with_priority(mut self, priority: Priority) -> Self {
        self.priority = priority;
        self
    }

    pub const fn with_peer(mut self, peer: Peer) -> Self {
        self.peer = peer;
        self
    }

    pub const fn with_trace(mut self, trace: TraceContext) -> Self {
        self.trace = Some(trace);
        self
    }

    pub const fn with_deadline(mut self, deadline: Millis<u32>) -> Self {
        self.deadline = Some(deadline);
        self
    }

    pub const fn with_connection(mut self, connection: ConnectionId) -> Self {
        self.connection = connection;
        self
    }

    /// The caller doesn't expect a reply (`seq` 0); always true for a call in the same process.
    pub const fn is_reply_expected(&self) -> bool {
        self.seq != 0
    }
}

impl Default for CallContext {
    /// [CallContext::local]
    fn default() -> Self {
        Self::local()
    }
}

/// Who asked, as the transport saw it (SRV-46): the kernel's word on a unix socket, a verified key, an address.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
#[non_exhaustive]
pub enum Peer {
    /// A call in the same process ([CallContext::local]).
    Local,
    /// A process on this machine, from the unix socket's peer credentials. `pid` is None where the system doesn't
    /// tell it (`getpeereid` on macOS).
    Unix { uid: Uid, gid: Gid, pid: Option<Pid> },
    /// A peer that proved a key: an iroh node id, a client certificate's SPKI SHA-256.
    Key(PeerKey),
    /// A network peer known by its address only (TCP, UDP): an address is not an identity.
    Addr(core::net::SocketAddr),
    /// The transport says nothing about the peer (USB, RTT, CAN, a device's links).
    Unknown,
}

// by hand: defmt has no Format for core::net addresses
#[cfg(feature = "defmt")]
impl defmt::Format for Peer {
    fn format(&self, f: defmt::Formatter<'_>) {
        use core::net::SocketAddr;
        match self {
            Peer::Local => defmt::write!(f, "Local"),
            Peer::Unix { uid, gid, pid } => {
                defmt::write!(f, "Unix {{ uid: {}, gid: {}, pid: {} }}", uid, gid, pid)
            }
            Peer::Key(key) => defmt::write!(f, "Key({})", key),
            Peer::Addr(SocketAddr::V4(a)) => {
                defmt::write!(f, "Addr({}:{})", a.ip().octets(), a.port())
            }
            Peer::Addr(SocketAddr::V6(a)) => {
                defmt::write!(f, "Addr([{}]:{})", a.ip().segments(), a.port())
            }
            Peer::Unknown => defmt::write!(f, "Unknown"),
        }
    }
}

/// A user id on this machine (`uid_t`).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Uid(pub u32);

/// A group id on this machine (`gid_t`).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Gid(pub u32);

/// A process id on this machine.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Pid(pub u32);

/// 32 bytes that name a peer that proved it holds a key: a public key (an iroh node id, Ed25519) or a certificate
/// fingerprint (SPKI SHA-256).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct PeerKey(pub [u8; 32]);

/// Tells the connections of one server apart (SRV-12): a server numbers them as it accepts them. Calls in the same
/// process and on a device with one link are on [ConnectionId::LOCAL].
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct ConnectionId(pub u32);

impl ConnectionId {
    /// Calls in the same process, and the one link of a device.
    pub const LOCAL: ConnectionId = ConnectionId(0);
}

/// W3C trace context (`traceparent`, version 00): which trace and span a call belongs to.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct TraceContext {
    pub trace_id: TraceId,
    /// The caller's span: the parent of the spans the call makes.
    pub parent_id: SpanId,
    pub flags: TraceFlags,
}

/// A trace's id, 16 bytes; all zeros is not valid.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct TraceId(pub [u8; 16]);

/// A span's id, 8 bytes; all zeros is not valid.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct SpanId(pub [u8; 8]);

/// `trace-flags` of a trace context: bit 0 is `sampled`.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct TraceFlags(pub u8);

impl TraceFlags {
    pub const SAMPLED: TraceFlags = TraceFlags(1);

    pub const fn is_sampled(self) -> bool {
        self.0 & 1 != 0
    }
}

impl TraceContext {
    /// Both ids are valid (not all zeros), as W3C asks of a `traceparent` to be used.
    pub fn is_valid(&self) -> bool {
        self.trace_id.0 != [0; 16] && self.parent_id.0 != [0; 8]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_and_request() {
        let cx = CallContext::local();
        assert_eq!(cx.peer, Peer::Local);
        assert!(!cx.is_reply_expected());
        assert_eq!(cx, CallContext::default());

        let cx = CallContext::new(7)
            .with_priority(Priority::URGENT)
            .with_deadline(Millis::new(250))
            .with_connection(ConnectionId(3));
        assert_eq!(cx.peer, Peer::Unknown);
        assert!(cx.is_reply_expected());
        assert_eq!(cx.deadline.map(|d| d.get()), Some(250));
        assert_eq!(cx.priority, Priority::URGENT);
    }

    #[test]
    fn trace_validity() {
        let mut trace = TraceContext {
            trace_id: TraceId([1; 16]),
            parent_id: SpanId([2; 8]),
            flags: TraceFlags::SAMPLED,
        };
        assert!(trace.is_valid() && trace.flags.is_sampled());
        trace.trace_id = TraceId([0; 16]);
        assert!(!trace.is_valid());
    }

    /// It sits in every server `Context`: a change of size is a change of the firmware's stack (SRV-48 measured the
    /// dispatch frames with it; `Peer` 36 B, the trace 26 B).
    #[test]
    fn size() {
        assert_eq!(size_of::<CallContext>(), 80);
    }
}
