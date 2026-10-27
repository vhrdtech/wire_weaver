//! What the hub reads of `ww_client_server` requests and events (SRV-29): the seq, the path and the kind, nothing of
//! the user API. Requests and events are copied with another seq in front; the bytes after the seq stay as they are,
//! so a request of a newer envelope version, which this build can't decode, still goes through.

use crate::claims::{Access, TraitId};
use wire_weaver::shrink_wrap::{
    BufReader, DeserializeShrinkWrap, SerializeShrinkWrap, UVlq32, UVlq32Backfill,
};
use ww_client_server::observe::ObserveRequestOwned;
use ww_client_server::{
    Event, EventKind, PathKind, PathKindOwned, Request, RequestKind, RequestKindOwned,
    RequestOwned, StreamSideband,
};

/// The seq at the start of a request or an event, and how many bytes it takes.
pub(crate) fn split_seq(bytes: &[u8]) -> Option<(u32, &[u8])> {
    let mut rd = BufReader::new(bytes);
    let seq = rd.read_uvlq32().ok()?;
    let (len, _) = rd.pos();
    Some((seq, &bytes[len..]))
}

/// `rest` (the bytes after a seq) with `seq` in front, canonical (shortest) form: an event to a client.
pub(crate) fn with_seq(seq: u32, rest: &[u8]) -> Vec<u8> {
    let mut buf = [0u8; UVlq32Backfill::LEN];
    let head = UVlq32(seq).to_ww_bytes(&mut buf).unwrap_or(&[0]);
    let mut out = Vec::with_capacity(head.len() + rest.len());
    out.extend_from_slice(head);
    out.extend_from_slice(rest);
    out
}

/// `rest` with the seq placeholder in front, as `wire_weaver_client`'s `Command::Forward` takes a request.
pub(crate) fn with_placeholder(rest: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; UVlq32Backfill::LEN + rest.len()];
    // the padded form of 0, which the event loop fills in
    let _ = UVlq32Backfill::backfill(&mut out, 0);
    out[UVlq32Backfill::LEN..].copy_from_slice(rest);
    out
}

/// What a request does, as far as the hub cares.
#[derive(Debug)]
pub(crate) enum Op {
    /// Opens a stream (any delivery) at the path
    StreamOpen(Vec<u32>),
    /// Closes the stream at the path
    StreamClose(Vec<u32>),
    /// Another sideband of the stream at the path (`Ack`, `Resend`, `Query`, user sidebands)
    StreamSideband(Vec<u32>),
    /// Observes properties of the trait level at the path
    Observe(Vec<u32>, ObserveRequestOwned),
    Introspect,
    /// Anything else, or a request this build can't decode (a newer envelope): forwarded as it is
    Other,
}

/// Reads the path and kind of a request; absolute paths only, the events the hub fans out carry those.
pub(crate) fn classify(request: &[u8]) -> Op {
    let Ok(req) = Request::from_ww_bytes(request) else {
        return Op::Other;
    };
    if matches!(req.kind, RequestKind::Introspect) {
        return Op::Introspect;
    }
    let PathKind::Absolute { path } = &req.path_kind else {
        return Op::Other;
    };
    let Ok(path) = path
        .iter()
        .map(|n| n.map(|n| n.0))
        .collect::<Result<Vec<u32>, _>>()
    else {
        return Op::Other;
    };
    match &req.kind {
        RequestKind::StreamOpen { .. } => Op::StreamOpen(path),
        RequestKind::StreamSideband {
            sideband: StreamSideband::Close,
        } => Op::StreamClose(path),
        RequestKind::StreamSideband { .. } => Op::StreamSideband(path),
        RequestKind::Observe { cfg } => match cfg.make_owned() {
            Ok(cfg) => Op::Observe(path, cfg),
            Err(_) => Op::Other,
        },
        _ => Op::Other,
    }
}

/// Where a request goes, as it names it (SRV-34).
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Place {
    Absolute(Vec<u32>),
    /// A global path: the trait, then the path from it
    Trait(TraitId, Vec<u32>),
}

/// What a request does where, for claims (SRV-34): the access, the place, and whether it is a multi request (it
/// reaches the resources under its path). `Ok(None)` for introspection, `Err` for a request this build can't decode
/// (a newer envelope).
pub(crate) fn access(request: &[u8]) -> Result<Option<(Access, Place, bool)>, ()> {
    let req = Request::from_ww_bytes(request).map_err(|_| ())?;
    let (access, multi) = match &req.kind {
        RequestKind::Call { .. } => (Access::Call, false),
        RequestKind::MultiCall { .. } => (Access::Call, true),
        RequestKind::Write { .. }
        | RequestKind::SinkWrite { .. }
        | RequestKind::SinkWriteUnits { .. } => (Access::Write, false),
        RequestKind::MultiWrite { .. } => (Access::Write, true),
        RequestKind::Read => (Access::Read, false),
        RequestKind::MultiRead { .. } => (Access::Read, true),
        RequestKind::StreamOpen { .. } | RequestKind::StreamSideband { .. } => {
            (Access::Stream, false)
        }
        RequestKind::Observe { .. } => (Access::Observe, false),
        RequestKind::Introspect => return Ok(None),
    };
    let ids = |p: &wire_weaver::shrink_wrap::RefVec<'_, wire_weaver::shrink_wrap::UNib32>| {
        p.iter()
            .map(|n| n.map(|n| n.0))
            .collect::<Result<Vec<u32>, _>>()
            .ok()
    };
    let place = match &req.path_kind {
        PathKind::Absolute { path } => Place::Absolute(ids(path).ok_or(())?),
        PathKind::GlobalCompact {
            gid,
            path_from_trait,
        } => Place::Trait(
            TraitId::Compact(gid.gid.id.0),
            ids(path_from_trait).ok_or(())?,
        ),
        PathKind::GlobalFull {
            gid,
            trait_name,
            path_from_trait,
        } => Place::Trait(
            TraitId::Full {
                crate_id: gid.crate_id.to_string(),
                trait_name: trait_name.to_string(),
            },
            ids(path_from_trait).ok_or(())?,
        ),
    };
    Ok(Some((access, place, multi)))
}

/// A request the hub sends itself: closing a stream, an observe merged from several clients. Seq placeholder in front.
pub(crate) fn request(path: &[u32], kind: RequestKindOwned) -> Option<Vec<u8>> {
    use wire_weaver::shrink_wrap::{SerializeShrinkWrapOwned, UNib32};
    RequestOwned {
        seq: UVlq32Backfill(0),
        priority: ww_client_server::Priority::NORMAL.nibble(),
        path_kind: PathKindOwned::Absolute {
            path: path.iter().map(|n| UNib32(*n)).collect(),
        },
        kind,
    }
    .to_ww_bytes_owned()
    .ok()
}

/// `Written` with `seq`: what a device answers a sideband it took without a reply.
pub(crate) fn written(seq: u32) -> Vec<u8> {
    let event = Event {
        seq: UVlq32(seq),
        result: Ok(EventKind::Written),
    };
    let mut buf = [0u8; 16];
    event
        .to_ww_bytes(&mut buf)
        .map(<[u8]>::to_vec)
        .unwrap_or_default()
}

/// What an event with seq 0 is to the hub.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Unsolicited {
    /// Stream data or a stream item at the path: dropped when a client's queue is full
    Data(Vec<u32>),
    /// A sideband of the stream at the path, `Close` or not: never dropped
    Sideband { path: Vec<u32>, close: bool },
    /// Property updates of the trait level at the path
    PropertyUpdate(Vec<u32>),
    /// Something this build doesn't know: to every client, never dropped
    Other,
}

pub(crate) fn classify_event(event: &[u8]) -> Unsolicited {
    let Ok(event) = Event::from_ww_bytes(event) else {
        return Unsolicited::Other;
    };
    let path = |p: &wire_weaver::shrink_wrap::RefVec<'_, wire_weaver::shrink_wrap::UNib32>| {
        p.iter()
            .map(|n| n.map(|n| n.0))
            .collect::<Result<Vec<u32>, _>>()
            .ok()
    };
    match &event.result {
        Ok(EventKind::StreamData { path: p, .. } | EventKind::StreamItem { path: p, .. }) => {
            path(p).map_or(Unsolicited::Other, Unsolicited::Data)
        }
        Ok(EventKind::StreamSideband { path: p, sideband }) => match path(p) {
            Some(path) => Unsolicited::Sideband {
                path,
                close: matches!(sideband, StreamSideband::Close),
            },
            None => Unsolicited::Other,
        },
        Ok(EventKind::PropertyUpdate { path: p, .. }) => {
            path(p).map_or(Unsolicited::Other, Unsolicited::PropertyUpdate)
        }
        _ => Unsolicited::Other,
    }
}

/// The introspection data in the events of an introspection answer (each after its seq): the stream data at the
/// root path, in order.
pub(crate) fn introspection_bytes(events: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = vec![];
    for e in events {
        if let Ok(event) = Event::from_ww_bytes(&with_seq(0, e))
            && let Ok(EventKind::StreamData { path, data }) = event.result
            && path.is_empty()
        {
            bytes.extend_from_slice(data.0);
        }
    }
    bytes
}

/// Whether an answer says the request worked: a value, `Written` or a sideband, not an error.
pub(crate) fn is_ok(event: &[u8]) -> bool {
    Event::from_ww_bytes(event).is_ok_and(|e| e.result.is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wire_weaver::shrink_wrap::{RefVec, SerializeShrinkWrapOwned, UNib32};

    #[test]
    fn seq_replaced_rest_kept() {
        for seq in [0, 1, 127, 128, 70_000, u32::MAX] {
            let bytes = with_seq(seq, &[0xAB, 0xCD]);
            assert_eq!(split_seq(&bytes), Some((seq, &[0xAB, 0xCD][..])));
        }
        // a request as a client serializes it: padded seq placeholder
        let req = RequestOwned {
            seq: UVlq32Backfill(0),
            priority: ww_client_server::Priority::NORMAL.nibble(),
            path_kind: PathKindOwned::Absolute {
                path: vec![UNib32(2), UNib32(5)],
            },
            kind: RequestKindOwned::Read,
        }
        .to_ww_bytes_owned()
        .unwrap();
        let (seq, rest) = split_seq(&req).unwrap();
        assert_eq!(seq, 0);
        assert_eq!(with_placeholder(rest), req);
        // the same request with a real seq, as on the wire
        let mut sent = req.clone();
        let trimmed = Request::set_seq(&mut sent, 300).unwrap().to_vec();
        assert_eq!(split_seq(&trimmed), Some((300, rest)));
        assert!(matches!(classify(&trimmed), Op::Other));
    }

    #[test]
    fn requests_and_events_classified() {
        let open = request(&[3, 1], RequestKindOwned::Introspect).unwrap();
        assert!(matches!(classify(&open), Op::Introspect));
        let close = request(
            &[3, 1],
            RequestKindOwned::StreamSideband {
                sideband: StreamSideband::Close,
            },
        )
        .unwrap();
        assert!(matches!(classify(&close), Op::StreamClose(p) if p == [3, 1]));
        assert!(matches!(classify(&[0xFF, 0xFF]), Op::Other));

        let event = Event {
            seq: UVlq32(0),
            result: Ok(EventKind::StreamSideband {
                path: RefVec::Slice {
                    slice: &[UNib32(4)],
                },
                sideband: StreamSideband::Close,
            }),
        };
        let mut buf = [0u8; 32];
        let bytes = event.to_ww_bytes(&mut buf).unwrap();
        assert_eq!(
            classify_event(bytes),
            Unsolicited::Sideband {
                path: vec![4],
                close: true
            }
        );
        assert!(is_ok(&written(9)));
        assert_eq!(split_seq(&written(9)).unwrap().0, 9);
    }
}
