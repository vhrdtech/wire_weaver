//! W3C trace context (`traceparent`, version 00) as WireWeaver puts it on the wire: the trace and span a request
//! belongs to, carried in `ww_client_server::RequestContext` (SRV-47) and read by handlers from
//! `wire_weaver::CallContext`. Types only, `no_std`, no allocator.
#![no_std]

use shrink_wrap::prelude::*;

/// W3C trace context (`traceparent`, version 00): which trace and span a call belongs to. On the wire it is its 25
/// bytes as they are: the ids, then the flags.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, Eq, PartialEq, Hash),
    sized
)]
pub struct TraceContext {
    pub trace_id: TraceId,
    /// The caller's span: the parent of the spans the call makes.
    pub parent_id: SpanId,
    pub flags: TraceFlags,
}

/// A trace's id, 16 bytes; all zeros is not valid.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, Eq, PartialEq, Hash),
    sized
)]
pub struct TraceId(pub [u8; 16]);

/// A span's id, 8 bytes; all zeros is not valid.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, Eq, PartialEq, Hash),
    sized
)]
pub struct SpanId(pub [u8; 8]);

/// `trace-flags` of a trace context: bit 0 is `sampled`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default),
    sized
)]
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

    #[test]
    fn wire_bytes_are_the_ids_then_the_flags() {
        let trace = TraceContext {
            trace_id: TraceId([0x4b; 16]),
            parent_id: SpanId([0x0f; 8]),
            flags: TraceFlags::SAMPLED,
        };
        let mut buf = [0u8; 32];
        let bytes = trace.to_ww_bytes(&mut buf).expect("25 bytes fit in 32");
        assert_eq!(bytes.len(), 25);
        assert_eq!(&bytes[..16], &[0x4b; 16]);
        assert_eq!(&bytes[16..24], &[0x0f; 8]);
        assert_eq!(bytes[24], 1);
    }
}
