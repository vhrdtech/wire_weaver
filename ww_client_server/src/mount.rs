//! What a generated server hands the handler of a mount slot (SRV-76) for a request under the slot's `api`, what
//! the handler answers, and the prefix rewrite of what crosses the slot (SRV-77).
//!
//! The handler `<slot>_request` is the one place a request leaves the generated dispatch for a backend: a mount
//! forwarder takes the slot's path off ([MountRequest::forward_into]), sends the request on with
//! [MountRequest::path] as its absolute path, and answers it later under the request's own seq. A slot with nothing
//! mounted answers [MountAnswer::Unavailable].
//!
//! The rewrite reads and writes the envelope only: arguments, values and stream data are copied as bytes, never
//! decoded with the mounted API's types. Seqs pass through unchanged, since one front connection owns its backend
//! connection. Events going back to the client get the slot's path in front of theirs ([MountPrefix::event_out]).

use wire_weaver::shrink_wrap::{DeserializeShrinkWrap, RefVec, SerializeShrinkWrap};

#[cfg(feature = "std")]
use crate::{Event, EventKind};
use crate::{PathKind, Request, ResourceIndex};

/// A request under a mount slot's `api`: the whole request and the part of its path below the slot.
#[derive(Debug, Clone, Copy)]
pub struct MountRequest<'a, 'i> {
    /// The request as it arrived: its seq, priority, context and kind.
    pub request: &'a Request<'i>,
    /// The path below the slot's `api`: the request's path as the mounted backend sees it, empty for its root (an
    /// `Introspect` there asks for the backend's introspection data).
    pub path: &'a [ResourceIndex],
}

/// What the handler of a mount slot did with a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MountAnswer {
    /// Nothing is mounted, or the backend is gone: the request is answered with
    /// [ErrorKind::Unavailable](crate::ErrorKind::Unavailable) at once.
    Unavailable,
    /// The request was sent on: the backend's reply, events and stream items reach the client later, under the
    /// request's seq.
    Forwarded,
}

impl wire_weaver::handler::SyncReturn for MountAnswer {}

/// Why a message could not be rewritten.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RewriteError {
    /// The message doesn't decode with this build: malformed, or of a newer envelope.
    Unreadable(wire_weaver::shrink_wrap::Error),
    /// The rewritten message doesn't fit the buffer.
    TooBig(wire_weaver::shrink_wrap::Error),
}

impl core::fmt::Display for RewriteError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            RewriteError::Unreadable(e) => write!(f, "message doesn't decode: {e:?}"),
            RewriteError::TooBig(e) => write!(f, "rewritten message doesn't fit its buffer: {e:?}"),
        }
    }
}

impl core::error::Error for RewriteError {}

impl MountRequest<'_, '_> {
    /// The request as the backend gets it, written into `buf`: [MountRequest::path] as its absolute path, everything
    /// else as it arrived.
    ///
    /// # Errors
    ///
    /// [RewriteError::TooBig] when it doesn't fit `buf`.
    pub fn forward_into<'b>(&self, buf: &'b mut [u8]) -> Result<&'b [u8], RewriteError> {
        request_at(self.request, self.path, buf)
    }

    /// Path of the slot's `api`: the request's absolute path without [MountRequest::path]. `None` when the request
    /// has no absolute path, which the generated dispatch never hands a slot.
    #[cfg(feature = "std")]
    pub fn prefix(&self) -> Option<std::vec::Vec<ResourceIndex>> {
        let PathKind::Absolute { path } = &self.request.path_kind else {
            return None;
        };
        let full = path.iter().collect::<Result<std::vec::Vec<_>, _>>().ok()?;
        let len = full.len().checked_sub(self.path.len())?;
        (full[len..] == *self.path).then(|| full[..len].to_vec())
    }
}

/// `request` with the absolute path `path` instead of its own, written into `buf`; its seq, priority, context and
/// kind as they are. A forwarder sends a request into a slot with it, and a global path it resolved into a mount
/// (SRV-77) as the absolute path the generated dispatch routes into the slot.
///
/// # Errors
///
/// [RewriteError::TooBig] when it doesn't fit `buf`.
pub fn request_at<'b>(
    request: &Request<'_>,
    path: &[ResourceIndex],
    buf: &'b mut [u8],
) -> Result<&'b [u8], RewriteError> {
    let mut at = request.clone();
    at.path_kind = PathKind::Absolute {
        path: RefVec::Slice { slice: path },
    };
    let len = at.to_ww_bytes(buf).map_err(RewriteError::TooBig)?.len();
    let bytes = Request::set_seq(&mut buf[..len], request.seq.0).map_err(RewriteError::TooBig)?;
    Ok(bytes)
}

/// [request_at] of the request in `bytes`.
///
/// # Errors
///
/// [RewriteError::Unreadable] when `bytes` isn't a request, [RewriteError::TooBig] when it doesn't fit `buf`.
pub fn request_bytes_at<'b>(
    bytes: &[u8],
    path: &[ResourceIndex],
    buf: &'b mut [u8],
) -> Result<&'b [u8], RewriteError> {
    let request = Request::from_ww_bytes(bytes).map_err(RewriteError::Unreadable)?;
    request_at(&request, path, buf)
}

/// The path of a mount slot's `api` on the front, and what puts it in front of the paths the backend sends back.
#[cfg(feature = "std")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountPrefix {
    prefix: std::vec::Vec<ResourceIndex>,
    /// Reused for the joined path of each event.
    joined: std::vec::Vec<ResourceIndex>,
}

#[cfg(feature = "std")]
impl MountPrefix {
    /// The prefix of the slot whose `api` is at `prefix`: `[hub, 1]` for `mount!(hub)`, `[ext, index, 1]` for an
    /// element of `mounts!(ext)`.
    pub fn new(prefix: std::vec::Vec<ResourceIndex>) -> Self {
        Self {
            prefix,
            joined: std::vec::Vec::new(),
        }
    }

    /// The path of the slot's `api`.
    pub fn path(&self) -> &[ResourceIndex] {
        &self.prefix
    }

    /// The front's absolute path of the backend's path `below`.
    pub fn join(&self, below: &[ResourceIndex]) -> std::vec::Vec<ResourceIndex> {
        let mut path = self.prefix.clone();
        path.extend_from_slice(below);
        path
    }

    /// The backend's event in `event` as the front client gets it, written into `buf`: stream data, sideband, stream
    /// items and property updates with the prefix in front of their path; replies, writes and errors carry no path
    /// and are copied as they are.
    ///
    /// # Errors
    ///
    /// [RewriteError::Unreadable] when `event` isn't an event, [RewriteError::TooBig] when it doesn't fit `buf`.
    pub fn event_out<'b>(
        &mut self,
        event: &[u8],
        buf: &'b mut [u8],
    ) -> Result<&'b [u8], RewriteError> {
        let Event { seq, result } =
            Event::from_ww_bytes(event).map_err(RewriteError::Unreadable)?;
        let path = match &result {
            Ok(EventKind::StreamData { path, .. })
            | Ok(EventKind::StreamSideband { path, .. })
            | Ok(EventKind::PropertyUpdate { path, .. }) => path,
            #[cfg(feature = "streams")]
            Ok(EventKind::StreamItem { path, .. }) => path,
            _ => {
                let out = buf.get_mut(..event.len()).ok_or(RewriteError::TooBig(
                    wire_weaver::shrink_wrap::Error::OutOfBoundsWriteRawSlice,
                ))?;
                out.copy_from_slice(event);
                return Ok(out);
            }
        };
        self.joined.clear();
        self.joined.extend_from_slice(&self.prefix);
        for id in path.iter() {
            self.joined.push(id.map_err(RewriteError::Unreadable)?);
        }
        let path = RefVec::Slice {
            slice: self.joined.as_slice(),
        };
        let result = match result {
            Ok(EventKind::StreamData { data, .. }) => Ok(EventKind::StreamData { path, data }),
            Ok(EventKind::StreamSideband { sideband, .. }) => {
                Ok(EventKind::StreamSideband { path, sideband })
            }
            #[cfg(feature = "streams")]
            Ok(EventKind::StreamItem { head, data, .. }) => {
                Ok(EventKind::StreamItem { path, head, data })
            }
            Ok(EventKind::PropertyUpdate {
                t,
                missed,
                sample_times,
                data,
                ..
            }) => Ok(EventKind::PropertyUpdate {
                path,
                t,
                missed,
                sample_times,
                data,
            }),
            _ => unreachable!("only events with a path get here, matched above"),
        };
        Event { seq, result }
            .to_ww_bytes(buf)
            .map_err(RewriteError::TooBig)
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    use crate::stream::StreamHead;
    use crate::{
        Error, ErrorKind, Priority, RequestContext, RequestKind, StreamSideband, resource_path,
    };
    use wire_weaver::shrink_wrap::tail_bytes::TailBytes;
    use wire_weaver::shrink_wrap::{RefBox, UNib32, UVlq32, UVlq32Backfill};

    fn request<'i>(path: &'i [ResourceIndex], kind: RequestKind<'i>) -> Request<'i> {
        Request {
            seq: UVlq32Backfill(300),
            priority: Priority::NORMAL.to_wire(),
            context: None,
            path_kind: PathKind::absolute(path),
            kind,
        }
    }

    fn sent<'b>(request: &Request<'_>, buf: &'b mut [u8]) -> &'b [u8] {
        let len = request.to_ww_bytes(buf).unwrap().len();
        Request::set_seq(&mut buf[..len], request.seq.0).unwrap()
    }

    fn ids(path: &RefVec<'_, ResourceIndex>) -> std::vec::Vec<u32> {
        path.iter().map(|id| id.unwrap().get().0).collect()
    }

    #[test]
    fn forwarded_request_has_the_path_below_the_slot() {
        let front = resource_path([3, 1, 2, 5]);
        let req = request(
            &front,
            RequestKind::Call {
                args: TailBytes(&[7, 8]),
            },
        );
        let mount = MountRequest {
            request: &req,
            path: &front[2..],
        };
        let mut buf = [0u8; 64];
        let bytes = mount.forward_into(&mut buf).unwrap();
        let back = Request::from_ww_bytes(bytes).unwrap();
        assert_eq!(back.seq.0, 300);
        assert_eq!(back.priority, Priority::NORMAL.to_wire());
        let PathKind::Absolute { path } = &back.path_kind else {
            panic!("an absolute path");
        };
        assert_eq!(ids(path), [2, 5]);
        let RequestKind::Call { args } = back.kind else {
            panic!("a call");
        };
        assert_eq!(args.as_slice(), [7, 8]);
        assert_eq!(mount.prefix(), Some(resource_path([3, 1]).to_vec()));
    }

    /// A request at its own path comes out byte for byte as it was sent: the rewrite touches the path only.
    #[test]
    fn same_path_gives_the_same_bytes() {
        let path = resource_path([3, 1, 2]);
        let context = RequestContext {
            trace: None,
            deadline: Some(ww_si::Millis::new(UNib32(250))),
        };
        let kinds = [
            RequestKind::Read,
            RequestKind::Introspect,
            RequestKind::Write {
                data: TailBytes(&[1, 2, 3]),
            },
            RequestKind::StreamSideband {
                sideband: StreamSideband::Open,
            },
        ];
        for kind in kinds {
            for with_context in [false, true] {
                let mut req = request(&path, kind.clone());
                if with_context {
                    req.context = Some(RefBox::new(&context));
                }
                let (mut a, mut b) = ([0u8; 64], [0u8; 64]);
                let original = sent(&req, &mut a);
                assert_eq!(
                    request_at(&req, &path, &mut b).unwrap(),
                    original,
                    "{kind:?}"
                );
                let mut c = [0u8; 64];
                assert_eq!(request_bytes_at(original, &path, &mut c).unwrap(), original);
            }
        }
    }

    #[test]
    fn request_too_big_for_the_buffer() {
        let path = resource_path([3, 1, 2]);
        let req = request(
            &path,
            RequestKind::Write {
                data: TailBytes(&[0; 32]),
            },
        );
        let mut buf = [0u8; 8];
        assert!(matches!(
            request_at(&req, &path, &mut buf),
            Err(RewriteError::TooBig(_))
        ));
        assert!(matches!(
            request_bytes_at(&[0xff; 2], &path, &mut buf),
            Err(RewriteError::Unreadable(_))
        ));
    }

    fn event_bytes(event: Event<'_>) -> std::vec::Vec<u8> {
        let mut buf = [0u8; 128];
        event.to_ww_bytes(&mut buf).unwrap().to_vec()
    }

    fn out(prefix: &mut MountPrefix, event: &[u8]) -> std::vec::Vec<u8> {
        let mut buf = [0u8; 128];
        prefix.event_out(event, &mut buf).unwrap().to_vec()
    }

    #[test]
    fn events_with_a_path_get_the_prefix() {
        let below = resource_path([0, 4]);
        let joined = resource_path([3, 1, 0, 4]);
        let mut prefix = MountPrefix::new(resource_path([3, 1]).to_vec());
        assert_eq!(prefix.join(&below), joined);
        let data = |path| EventKind::StreamData {
            path,
            data: TailBytes(&[9, 9]),
        };
        let sideband = |path| EventKind::StreamSideband {
            path,
            sideband: StreamSideband::Close,
        };
        let item = |path| EventKind::StreamItem {
            path,
            head: StreamHead::at(17),
            data: TailBytes(&[5]),
        };
        let update = |path| EventKind::PropertyUpdate {
            path,
            t: None,
            missed: None,
            sample_times: None,
            data: TailBytes(&[1, 0]),
        };
        let kinds: [&dyn Fn(RefVec<'static, ResourceIndex>) -> EventKind<'static>; 4] =
            [&data, &sideband, &item, &update];
        for kind in kinds {
            let at = |path: &'static [ResourceIndex]| {
                event_bytes(Event {
                    seq: UVlq32(0),
                    result: Ok(kind(RefVec::Slice { slice: path })),
                })
            };
            static BELOW: [ResourceIndex; 2] = resource_path([0, 4]);
            static JOINED: [ResourceIndex; 4] = resource_path([3, 1, 0, 4]);
            assert_eq!(out(&mut prefix, &at(&BELOW)), at(&JOINED));
            // an empty prefix gives the bytes back: the rewrite touches the path only
            assert_eq!(out(&mut MountPrefix::new(vec![]), &at(&BELOW)), at(&BELOW));
        }
    }

    #[test]
    fn replies_pass_unchanged() {
        let mut prefix = MountPrefix::new(resource_path([3, 1]).to_vec());
        let value = event_bytes(Event {
            seq: UVlq32(300),
            result: Ok(EventKind::Value {
                data: TailBytes(&[1, 2, 3]),
            }),
        });
        let error = event_bytes(Event {
            seq: UVlq32(7),
            result: Err(Error::new(4, ErrorKind::BadPath)),
        });
        assert_eq!(out(&mut prefix, &value), value);
        assert_eq!(out(&mut prefix, &error), error);
        let mut small = [0u8; 2];
        assert!(matches!(
            prefix.event_out(&value, &mut small),
            Err(RewriteError::TooBig(_))
        ));
        assert!(matches!(
            prefix.event_out(&[], &mut small),
            Err(RewriteError::Unreadable(_))
        ));
    }
}
