//! A UDP listener whose socket the loop watches.

use std::cell::RefCell;
use std::io;
use std::net::{SocketAddr, UdpSocket};

use calloop::LoopHandle;
use calloop::io::Async;
use wire_weaver_server::udp::UdpListener;
use ww_device::udp::{Readiness, StdUdpSocket};

/// When a [StdUdpSocket] is readable, from the loop watching a clone of it, which is never read itself.
pub struct LoopReadiness<'l>(RefCell<Async<'l, UdpSocket>>);

impl Readiness for LoopReadiness<'_> {
    #[expect(
        clippy::await_holding_refcell_ref,
        reason = "the peer table lets one receiver at a time wait on the socket"
    )]
    async fn readable(&self) -> io::Result<()> {
        let mut socket = self.0.try_borrow_mut().map_err(io::Error::other)?;
        // calloop hands the readiness out once: the next wait is for a datagram that comes after it
        socket.readable().await;
        Ok(())
    }
}

/// A UDP socket the loop watches.
pub type LoopUdpSocket<'l> = StdUdpSocket<LoopReadiness<'l>>;

/// A listener of up to `PEERS` hosts on a socket the loop watches.
pub type LoopUdpListener<'l, const PEERS: usize> = UdpListener<LoopUdpSocket<'l>, PEERS>;

/// Listen on a UDP socket bound to `addr`, on the loop of `handle`: a connection per host, up to `PEERS` at once
/// (`wire_weaver_server::udp`). `message_len_max` is the longest message the server takes; longer ones than a
/// datagram are split across several.
///
/// # Errors
///
/// Binding the socket failed, or the loop can't watch it.
pub fn bind_udp<'l, D, const PEERS: usize>(
    handle: &LoopHandle<'l, D>,
    addr: SocketAddr,
    message_len_max: usize,
) -> io::Result<LoopUdpListener<'l, PEERS>> {
    let socket = UdpSocket::bind(addr)?;
    let watched = handle.adapt_io(socket.try_clone()?)?;
    let readiness = LoopReadiness(RefCell::new(watched));
    Ok(UdpListener::new(
        StdUdpSocket::new(socket, readiness)?,
        message_len_max,
    ))
}
