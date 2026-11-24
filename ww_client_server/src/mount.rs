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

use wire_weaver::shrink_wrap::{DeserializeShrinkWrap, RefVec, SerializeShrinkWrap, UNib32};

use crate::{Event, EventKind, PathKind, Request, ResourceIndex, StreamSideband};

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
    /// The path doesn't fit the [PathStore] of a [MountPrefix]: a [PathArray] holds `len_max` ids.
    PathTooLong {
        /// How many ids the store holds.
        len_max: usize,
    },
}

impl core::fmt::Display for RewriteError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            RewriteError::Unreadable(e) => write!(f, "message doesn't decode: {e:?}"),
            RewriteError::TooBig(e) => write!(f, "rewritten message doesn't fit its buffer: {e:?}"),
            RewriteError::PathTooLong { len_max } => {
                write!(f, "path longer than the {len_max} ids its store holds")
            }
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
    /// has no absolute path, which the generated dispatch never hands a slot, or it doesn't decode.
    #[cfg(feature = "std")]
    pub fn prefix(&self) -> Option<std::vec::Vec<ResourceIndex>> {
        let prefix = self.prefix_in(std::vec::Vec::new()).ok().flatten()?;
        Some(prefix.path().to_vec())
    }

    /// The [MountPrefix] of the slot, its path kept in `store`: the request's absolute path without
    /// [MountRequest::path]. `None` when the request has no absolute path, which the generated dispatch never hands
    /// a slot, or its path doesn't end with [MountRequest::path].
    ///
    /// # Errors
    ///
    /// [RewriteError::Unreadable] when the path doesn't decode, [RewriteError::PathTooLong] when `store` can't
    /// hold the prefix.
    pub fn prefix_in<S: PathStore>(
        &self,
        mut store: S,
    ) -> Result<Option<MountPrefix<S>>, RewriteError> {
        let PathKind::Absolute { path } = &self.request.path_kind else {
            return Ok(None);
        };
        let Some(prefix_len) = path.len().checked_sub(self.path.len()) else {
            return Ok(None);
        };
        store.truncate(0);
        for (i, id) in path.iter().enumerate() {
            let id = id.map_err(RewriteError::Unreadable)?;
            match i.checked_sub(prefix_len) {
                None => store.push(id)?,
                Some(below) if self.path.get(below) == Some(&id) => {}
                Some(_) => return Ok(None),
            }
        }
        Ok(Some(MountPrefix { store, prefix_len }))
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

/// Where a [MountPrefix] keeps its path: a [PathArray] of fixed capacity in firmware, a `Vec` on a host.
pub trait PathStore {
    /// The ids stored, in order.
    fn as_slice(&self) -> &[ResourceIndex];
    /// Drop the ids past the first `len`.
    fn truncate(&mut self, len: usize);
    /// Append `id`.
    ///
    /// # Errors
    ///
    /// [RewriteError::PathTooLong] when the store is full.
    fn push(&mut self, id: ResourceIndex) -> Result<(), RewriteError>;
}

/// A path of at most `N` ids, without a heap: the [PathStore] of a firmware's [MountPrefix].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathArray<const N: usize> {
    ids: [ResourceIndex; N],
    len: usize,
}

impl<const N: usize> PathArray<N> {
    const UNUSED: ResourceIndex = ResourceIndex::new(UNib32(0));

    /// An empty path.
    pub const fn new() -> Self {
        Self {
            ids: [Self::UNUSED; N],
            len: 0,
        }
    }
}

impl<const N: usize> Default for PathArray<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> PathStore for PathArray<N> {
    fn as_slice(&self) -> &[ResourceIndex] {
        &self.ids[..self.len]
    }

    fn truncate(&mut self, len: usize) {
        self.len = self.len.min(len);
    }

    fn push(&mut self, id: ResourceIndex) -> Result<(), RewriteError> {
        let slot = self
            .ids
            .get_mut(self.len)
            .ok_or(RewriteError::PathTooLong { len_max: N })?;
        *slot = id;
        self.len += 1;
        Ok(())
    }
}

#[cfg(feature = "std")]
impl PathStore for std::vec::Vec<ResourceIndex> {
    fn as_slice(&self) -> &[ResourceIndex] {
        self
    }

    fn truncate(&mut self, len: usize) {
        std::vec::Vec::truncate(self, len);
    }

    fn push(&mut self, id: ResourceIndex) -> Result<(), RewriteError> {
        std::vec::Vec::push(self, id);
        Ok(())
    }
}

/// The path of a mount slot's `api` on the front, and what puts it in front of the paths the backend sends back.
///
/// `S` holds the prefix followed by the joined path of the last [join](Self::join) or
/// [event_out](Self::event_out): `MountPrefix<PathArray<N>>` in firmware, where a joined path longer than `N` is
/// [RewriteError::PathTooLong], `MountPrefix<Vec<ResourceIndex>>` on a host, with no limit of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountPrefix<S> {
    store: S,
    prefix_len: usize,
}

#[cfg(feature = "std")]
impl MountPrefix<std::vec::Vec<ResourceIndex>> {
    /// The prefix of the slot whose `api` is at `prefix`: `[hub, 1]` for `mount!(hub)`, `[ext, index, 1]` for an
    /// element of `mounts!(ext)`.
    pub fn from_vec(prefix: std::vec::Vec<ResourceIndex>) -> Self {
        Self {
            prefix_len: prefix.len(),
            store: prefix,
        }
    }
}

impl<S: PathStore> MountPrefix<S> {
    /// The prefix `prefix`, kept in `store`; whatever `store` held is dropped.
    ///
    /// # Errors
    ///
    /// [RewriteError::PathTooLong] when `store` can't hold `prefix`.
    pub fn new(mut store: S, prefix: &[ResourceIndex]) -> Result<Self, RewriteError> {
        store.truncate(0);
        for id in prefix {
            store.push(*id)?;
        }
        Ok(Self {
            store,
            prefix_len: prefix.len(),
        })
    }

    /// The path of the slot's `api`.
    pub fn path(&self) -> &[ResourceIndex] {
        &self.store.as_slice()[..self.prefix_len]
    }

    /// The front's absolute path of the backend's path `below`.
    ///
    /// # Errors
    ///
    /// [RewriteError::PathTooLong] when the store can't hold the joined path.
    pub fn join(&mut self, below: &[ResourceIndex]) -> Result<&[ResourceIndex], RewriteError> {
        self.join_ids(below.iter().copied().map(Ok))
    }

    fn join_ids(
        &mut self,
        below: impl Iterator<Item = Result<ResourceIndex, RewriteError>>,
    ) -> Result<&[ResourceIndex], RewriteError> {
        self.store.truncate(self.prefix_len);
        for id in below {
            self.store.push(id?)?;
        }
        Ok(self.store.as_slice())
    }

    /// The backend's event in `event` as the front client gets it, written into `buf`: stream data, sideband, stream
    /// items and property updates with the prefix in front of their path; replies, writes and errors carry no path
    /// and are copied as they are.
    ///
    /// # Errors
    ///
    /// [RewriteError::Unreadable] when `event` isn't an event, [RewriteError::TooBig] when it doesn't fit `buf`,
    /// [RewriteError::PathTooLong] when the joined path doesn't fit the store.
    pub fn event_out<'b>(
        &mut self,
        event: &[u8],
        buf: &'b mut [u8],
    ) -> Result<&'b [u8], RewriteError> {
        Ok(self.rewrite(event, buf)?.bytes)
    }

    /// [event_out](Self::event_out), with what a forwarder checks before it sends the event on: its seq, whether it
    /// is the last answer to its request, whether it closes a stream. After a rewrite of an event with a path,
    /// [joined](Self::joined) is that path on the front.
    ///
    /// # Errors
    ///
    /// As [event_out](Self::event_out).
    pub fn rewrite<'b>(
        &mut self,
        event: &[u8],
        buf: &'b mut [u8],
    ) -> Result<Rewritten<'b>, RewriteError> {
        // the path is replaced in place, the event is never moved into a second one
        let mut decoded = Event::from_ww_bytes(event).map_err(RewriteError::Unreadable)?;
        let seq = decoded.seq.0;
        // stream data under a request's seq (introspection) comes before its answer
        let last = !matches!(
            decoded.result,
            Ok(EventKind::StreamData { .. } | EventKind::PropertyUpdate { .. })
        );
        #[cfg(feature = "streams")]
        let last = last && !matches!(decoded.result, Ok(EventKind::StreamItem { .. }));
        let closes = matches!(
            decoded.result,
            Ok(EventKind::StreamSideband {
                sideband: StreamSideband::Close,
                ..
            })
        );
        let path = match &mut decoded.result {
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
                return Ok(Rewritten {
                    bytes: out,
                    seq,
                    last,
                    path: false,
                    closes: false,
                });
            }
        };
        let below = *path;
        let joined = self.join_ids(below.iter().map(|id| id.map_err(RewriteError::Unreadable)))?;
        *path = RefVec::Slice { slice: joined };
        let bytes = decoded.to_ww_bytes(buf).map_err(RewriteError::TooBig)?;
        Ok(Rewritten {
            bytes,
            seq,
            last,
            path: true,
            closes,
        })
    }

    /// The front's path of the last event [rewrite](Self::rewrite) or [join](Self::join) put the prefix in front of.
    pub fn joined(&self) -> &[ResourceIndex] {
        self.store.as_slice()
    }
}

/// An event as [MountPrefix::rewrite] wrote it, and what a forwarder checks before sending it on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rewritten<'b> {
    /// The event as the front client gets it.
    pub bytes: &'b [u8],
    /// Its seq: 0 for an event of no request.
    pub seq: u32,
    /// It is the last event of its request: not stream data, a stream item or a property update, which come before
    /// a request's answer (introspection data) or under seq 0.
    pub last: bool,
    /// It carries a path, now [MountPrefix::joined].
    pub path: bool,
    /// It is a stream's `Close` sideband.
    pub closes: bool,
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
            forwarded_for: None,
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

    fn out<S: PathStore>(prefix: &mut MountPrefix<S>, event: &[u8]) -> std::vec::Vec<u8> {
        let mut buf = [0u8; 128];
        prefix.event_out(event, &mut buf).unwrap().to_vec()
    }

    #[test]
    fn events_with_a_path_get_the_prefix() {
        let below = resource_path([0, 4]);
        let joined = resource_path([3, 1, 0, 4]);
        let mut prefix = MountPrefix::from_vec(resource_path([3, 1]).to_vec());
        assert_eq!(prefix.join(&below).unwrap(), joined);
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
            assert_eq!(
                out(&mut MountPrefix::from_vec(vec![]), &at(&BELOW)),
                at(&BELOW)
            );
        }
    }

    #[test]
    fn replies_pass_unchanged() {
        let mut prefix = MountPrefix::from_vec(resource_path([3, 1]).to_vec());
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

    /// What a forwarder checks comes with the rewritten bytes: the seq, whether the event ends its request, whether
    /// it closes a stream, and the front's path of an event with one.
    #[test]
    fn rewrite_tells_seq_and_kind() {
        let mut prefix = MountPrefix::from_vec(resource_path([3, 1]).to_vec());
        static BELOW: [ResourceIndex; 1] = resource_path([4]);
        let close = event_bytes(Event {
            seq: UVlq32(0),
            result: Ok(EventKind::StreamSideband {
                path: RefVec::Slice { slice: &BELOW },
                sideband: StreamSideband::Close,
            }),
        });
        let chunk = event_bytes(Event {
            seq: UVlq32(9),
            result: Ok(EventKind::StreamData {
                path: RefVec::Slice { slice: &[] },
                data: TailBytes(&[1]),
            }),
        });
        let value = event_bytes(Event {
            seq: UVlq32(9),
            result: Ok(EventKind::Value {
                data: TailBytes(&[1]),
            }),
        });
        let mut buf = [0u8; 64];
        let r = prefix.rewrite(&close, &mut buf).unwrap();
        assert_eq!((r.seq, r.last, r.path, r.closes), (0, true, true, true));
        assert_eq!(prefix.joined(), resource_path([3, 1, 4]));
        let r = prefix.rewrite(&chunk, &mut buf).unwrap();
        assert_eq!((r.seq, r.last, r.path, r.closes), (9, false, true, false));
        let r = prefix.rewrite(&value, &mut buf).unwrap();
        assert_eq!((r.seq, r.last, r.path, r.closes), (9, true, false, false));
        assert_eq!(r.bytes, value);
    }

    /// Events with paths `[0]`, `[0, 4]`, ... below the slot, one of each kind with a path.
    fn events_below(len_max: u32) -> std::vec::Vec<std::vec::Vec<u8>> {
        let mut events = std::vec::Vec::new();
        for len in 0..=len_max {
            let ids: std::vec::Vec<ResourceIndex> = (0..len)
                .map(|i| ResourceIndex::new(UNib32(i * 4)))
                .collect();
            let path = || RefVec::Slice { slice: &ids };
            let kinds = [
                EventKind::StreamData {
                    path: path(),
                    data: TailBytes(&[9, 9]),
                },
                EventKind::StreamSideband {
                    path: path(),
                    sideband: StreamSideband::Close,
                },
                EventKind::StreamItem {
                    path: path(),
                    head: StreamHead::at(17),
                    data: TailBytes(&[5]),
                },
                EventKind::PropertyUpdate {
                    path: path(),
                    t: None,
                    missed: None,
                    sample_times: None,
                    data: TailBytes(&[1, 0]),
                },
            ];
            for kind in kinds {
                events.push(event_bytes(Event {
                    seq: UVlq32(len),
                    result: Ok(kind),
                }));
            }
        }
        events
    }

    /// The fixed-capacity form gives the bytes of the `Vec` form up to its capacity, and `PathTooLong` past it, after
    /// which it rewrites the next event as before.
    #[test]
    fn fixed_and_vec_forms_give_the_same_bytes() {
        let front = resource_path([3, 1]);
        let mut host = MountPrefix::from_vec(front.to_vec());
        // the prefix and at most 4 ids below it
        let mut fixed = MountPrefix::new(PathArray::<6>::new(), &front).unwrap();
        let reply = event_bytes(Event {
            seq: UVlq32(300),
            result: Ok(EventKind::Value {
                data: TailBytes(&[1, 2, 3]),
            }),
        });
        for event in events_below(4).iter().chain([&reply]) {
            assert_eq!(out(&mut fixed, event), out(&mut host, event));
        }
        for event in events_below(5).iter().skip(5 * 4) {
            let host_bytes = out(&mut host, event);
            assert!(host_bytes.len() > event.len(), "the Vec form takes it");
            let mut buf = [0u8; 128];
            assert_eq!(
                fixed.event_out(event, &mut buf),
                Err(RewriteError::PathTooLong { len_max: 6 })
            );
            assert_eq!(fixed.path(), front, "the prefix survives an overflow");
        }
        let last_fitting = &events_below(4)[4 * 4];
        assert_eq!(out(&mut fixed, last_fitting), out(&mut host, last_fitting));
        // a buffer too small is TooBig in both, never a truncated message
        for event in events_below(4).iter().chain([&reply]) {
            let (mut a, mut b) = ([0u8; 4], [0u8; 4]);
            let fixed_out = fixed.event_out(event, &mut a);
            assert!(matches!(fixed_out, Err(RewriteError::TooBig(_))));
            assert_eq!(fixed_out, host.event_out(event, &mut b));
        }
    }

    #[test]
    fn fixed_join_and_prefix_up_to_capacity() {
        let front = resource_path([3, 1]);
        let below = resource_path([2, 5]);
        let full = resource_path([3, 1, 2, 5]);
        let mut host = MountPrefix::from_vec(front.to_vec());
        let mut fixed = MountPrefix::new(PathArray::<4>::new(), &front).unwrap();
        assert_eq!(fixed.join(&below).unwrap(), host.join(&below).unwrap());
        assert_eq!(
            fixed.join(&resource_path([2, 5, 6])),
            Err(RewriteError::PathTooLong { len_max: 4 })
        );
        assert_eq!(host.join(&resource_path([2, 5, 6])).unwrap().len(), 5);
        assert_eq!(
            MountPrefix::new(PathArray::<1>::new(), &front),
            Err(RewriteError::PathTooLong { len_max: 1 })
        );

        let req = request(&full, RequestKind::Read);
        let at = |path| MountRequest {
            request: &req,
            path,
        };
        let fitting = at(&below).prefix_in(PathArray::<2>::new()).unwrap();
        assert_eq!(fitting.map(|p| p.path().to_vec()), at(&below).prefix());
        assert_eq!(at(&below).prefix(), Some(front.to_vec()));
        assert_eq!(
            at(&below[1..]).prefix_in(PathArray::<2>::new()),
            Err(RewriteError::PathTooLong { len_max: 2 })
        );
        // a path that doesn't end with the one below the slot has no prefix in either form
        let other = resource_path([2, 6]);
        assert_eq!(at(&other).prefix_in(PathArray::<2>::new()), Ok(None));
        assert_eq!(at(&other).prefix(), None);
        let longer = resource_path([0, 3, 1, 2, 5]);
        assert_eq!(at(&longer).prefix_in(PathArray::<2>::new()), Ok(None));
        assert_eq!(at(&longer).prefix(), None);
    }
}
