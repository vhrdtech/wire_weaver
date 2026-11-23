//! Mounted backends on Unix sockets, reached from the loop (SRV-78).

use std::future::{Ready, ready};
use std::io;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use calloop::LoopHandle;
use calloop::io::Async;
use wire_weaver_server::mount::Connect;

/// A backend listening on a Unix socket, for [Mounts](wire_weaver_server::mount::Mounts) on the loop of `handle`:
/// the stream the loop watches. Connecting to a local socket doesn't wait, so it is done at once.
pub struct UnixConnector<'l, D> {
    handle: LoopHandle<'l, D>,
    path: PathBuf,
}

impl<'l, D> UnixConnector<'l, D> {
    /// The socket at `path`, on the loop of `handle`.
    pub fn new(handle: &LoopHandle<'l, D>, path: impl Into<PathBuf>) -> Self {
        UnixConnector {
            handle: handle.clone(),
            path: path.into(),
        }
    }
}

impl<'l, D> Connect for UnixConnector<'l, D> {
    type Stream = Async<'l, UnixStream>;
    type Opening = Ready<io::Result<Self::Stream>>;

    fn connect(&self) -> Self::Opening {
        ready(
            UnixStream::connect(&self.path)
                .and_then(|stream| self.handle.adapt_io(stream).map_err(io::Error::from)),
        )
    }
}
