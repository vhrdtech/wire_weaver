//! One client on the process's own stdin and stdout: a server run as an ssh forced command (`ssh host svc --stdio`,
//! `ClientConfig::command` on the client) or spawned by its client.

use std::convert::Infallible;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures_util::io::{AsyncRead, AsyncWrite};
use futures_util::stream::{self, Stream};
use tokio_util::compat::{Compat, TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use wire_weaver::Peer;

/// The process's stdin and stdout as one stream. Nothing else may read stdin or write stdout while it runs: logs go
/// to stderr.
pub struct Stdio {
    stdin: Compat<tokio::io::Stdin>,
    stdout: Compat<tokio::io::Stdout>,
}

/// A source of one stream, the process's stdio, whose peer is the process's own user: whoever could start it. A
/// server on it ends when its client leaves.
pub fn stdio() -> impl Stream<Item = Result<(Stdio, Peer), Infallible>> + Unpin {
    let stdio = Stdio {
        stdin: tokio::io::stdin().compat(),
        stdout: tokio::io::stdout().compat_write(),
    };
    stream::iter([Ok((stdio, own_user()))])
}

/// This process's user.
#[cfg(unix)]
fn own_user() -> Peer {
    use wire_weaver::{Gid, Uid};
    Peer::Unix {
        uid: Uid(rustix::process::getuid().as_raw()),
        gid: Gid(rustix::process::getgid().as_raw()),
        pid: None,
    }
}

/// This process's user, which [Peer] has no word for on Windows yet (SRV-46).
#[cfg(not(unix))]
fn own_user() -> Peer {
    Peer::Unknown
}

impl AsyncRead for Stdio {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.stdin).poll_read(cx, buf)
    }
}

impl AsyncWrite for Stdio {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.stdout).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stdout).poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stdout).poll_close(cx)
    }
}
