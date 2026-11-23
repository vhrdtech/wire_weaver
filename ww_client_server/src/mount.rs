//! What a generated server hands the handler of a mount slot (SRV-76) for a request under the slot's `api`, and what
//! the handler answers.
//!
//! The handler `<slot>_request` is the one place a request leaves the generated dispatch for a backend: a mount
//! forwarder takes the slot's path off, sends the request on with [MountRequest::path] as its absolute path, and
//! answers it later under the request's own seq. A slot with nothing mounted answers [MountAnswer::Unavailable].

use crate::{Request, ResourceIndex};

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
