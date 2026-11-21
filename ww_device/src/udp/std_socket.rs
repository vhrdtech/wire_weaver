//! [DatagramSocket] on a standard library UDP socket, for servers on a computer, on any event loop.

use core::future::Future;
use std::io::{self, ErrorKind};
use std::net::{SocketAddr, UdpSocket};

use super::DatagramSocket;
use crate::fmt::warn;

/// Tells when a nonblocking socket has a datagram to read, from the program's event loop.
pub trait Readiness {
    /// Wait until the socket may have a datagram. A wakeup without one is fine: the socket is read again, and this
    /// is awaited again when it would block. Must be cancel-safe.
    fn readable(&self) -> impl Future<Output = io::Result<()>>;
}

/// [DatagramSocket] on a [UdpSocket], for servers on a computer, on any event loop.
///
/// The socket is nonblocking and read until it would block; then the event loop says when it is readable again
/// ([Readiness]): calloop's `Async`, tokio's `readable()`, `async-io`, or anything else that watches a file
/// descriptor. Sends never wait: a datagram the kernel has no room for is dropped, as the network may drop it.
pub struct StdUdpSocket<R> {
    socket: UdpSocket,
    readiness: R,
}

impl<R: Readiness> StdUdpSocket<R> {
    /// Use `socket` (made nonblocking here), with `readiness` from the event loop watching it, e.g. on a clone of
    /// it ([UdpSocket::try_clone]).
    ///
    /// # Errors
    ///
    /// The socket can't be made nonblocking.
    pub fn new(socket: UdpSocket, readiness: R) -> io::Result<Self> {
        socket.set_nonblocking(true)?;
        Ok(StdUdpSocket { socket, readiness })
    }

    pub fn socket(&self) -> &UdpSocket {
        &self.socket
    }
}

impl<R: Readiness> DatagramSocket for StdUdpSocket<R> {
    type Addr = SocketAddr;
    type RecvError = io::Error;
    type SendError = io::Error;

    /// A datagram longer than `buf` is cut to it. A send's ICMP error that the system reports on a later receive
    /// (Windows does, as `ConnectionReset`) is about that send, not this socket, so it is skipped.
    async fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        loop {
            match self.socket.recv_from(buf) {
                Ok(received) => return Ok(received),
                Err(e) if e.kind() == ErrorKind::WouldBlock => self.readiness.readable().await?,
                Err(e)
                    if matches!(
                        e.kind(),
                        ErrorKind::Interrupted | ErrorKind::ConnectionReset
                    ) => {}
                Err(e) => return Err(e),
            }
        }
    }

    /// Never waits: a datagram the socket's buffer has no room for is dropped, as a lost one would be.
    async fn send_to(&self, datagram: &[u8], addr: SocketAddr) -> io::Result<()> {
        match self.socket.send_to(datagram, addr) {
            Ok(_) => Ok(()),
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                warn!("udp: socket buffer full, a datagram dropped");
                Ok(())
            }
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::UdpSocket;
    use std::time::Duration;
    use std::vec;

    use tokio::io::Interest;

    use super::*;

    /// tokio's readiness of a clone of the socket, which is never read itself.
    struct Tokio(tokio::net::UdpSocket);

    impl Readiness for Tokio {
        async fn readable(&self) -> io::Result<()> {
            self.0.readable().await?;
            // the clone is read, not this one: forget the readiness before that read, so that the next wait is for
            // a datagram that comes after it
            _ = self.0.try_io(Interest::READABLE, || {
                Err::<(), _>(ErrorKind::WouldBlock.into())
            });
            Ok(())
        }
    }

    fn socket() -> StdUdpSocket<Tokio> {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let watched = socket.try_clone().unwrap();
        watched.set_nonblocking(true).unwrap();
        let watched = tokio::net::UdpSocket::from_std(watched).unwrap();
        StdUdpSocket::new(socket, Tokio(watched)).unwrap()
    }

    /// A receive waits on the event loop until a datagram comes, a long one is cut to the buffer, and sends reach
    /// their address.
    #[tokio::test]
    async fn receives_when_readable_and_sends() {
        let device = socket();
        let host = UdpSocket::bind("127.0.0.1:0").unwrap();
        host.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let to = device.socket().local_addr().unwrap();

        let mut buf = [0u8; 4];
        let receive = device.recv_from(&mut buf);
        let send = async {
            // after the receive found nothing and waits on readiness
            tokio::task::yield_now().await;
            host.send_to(&[1, 2, 3, 4, 5, 6], to).unwrap();
        };
        let (received, ()) = tokio::join!(receive, send);
        let (len, from) = received.unwrap();
        assert_eq!((len, from), (4, host.local_addr().unwrap()));
        assert_eq!(buf, [1, 2, 3, 4]);

        device
            .send_to(&[9, 8], host.local_addr().unwrap())
            .await
            .unwrap();
        let mut back = vec![0u8; 16];
        let (len, from) = host.recv_from(&mut back).unwrap();
        assert_eq!((&back[..len], from), (&[9u8, 8][..], to));
    }
}
