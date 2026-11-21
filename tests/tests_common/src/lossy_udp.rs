//! A lossy UDP socket for the simulated UDP device (`in_process::udp_device`): every datagram from a host is
//! delivered, dropped or held back behind later ones, as a test decides ([Impair]), before `ww_device::udp` reads it.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use wire_weaver_client::in_process::{Disconnected, UdpAddr, UdpPort};
use ww_device::udp::DatagramSocket;

use crate::locked::Locked;

/// What happens to a datagram from the host on its way to the device.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Fate {
    /// Delivered in order.
    Deliver,
    /// Lost.
    Drop,
    /// Delivered after this many later datagrams were: reordered (at least 1).
    Delay(usize),
}

/// Decides the [Fate] of every datagram from the host, by its bytes.
pub type Impair = Box<dyn FnMut(&[u8]) -> Fate + Send>;

/// [UdpPort] with the datagrams from hosts going through an [Impair].
pub(crate) struct LossyPort {
    port: UdpPort,
    impair: Arc<Mutex<Impair>>,
    queue: RefCell<Queue>,
}

#[derive(Default)]
struct Queue {
    /// Datagrams due, in the order the device reads them.
    ready: VecDeque<(UdpAddr, Vec<u8>)>,
    /// Datagrams held back, with how many more datagrams go before each.
    held: Vec<(usize, UdpAddr, Vec<u8>)>,
}

impl LossyPort {
    pub(crate) fn new(port: UdpPort, impair: Arc<Mutex<Impair>>) -> Self {
        Self {
            port,
            impair,
            queue: RefCell::default(),
        }
    }
}

impl Queue {
    /// `datagram` goes now; the held ones whose turn came go right after it.
    fn deliver(&mut self, from: UdpAddr, datagram: Vec<u8>) {
        self.ready.push_back((from, datagram));
        let mut i = 0;
        while i < self.held.len() {
            self.held[i].0 -= 1;
            if self.held[i].0 == 0 {
                let (_, from, datagram) = self.held.remove(i);
                self.ready.push_back((from, datagram));
            } else {
                i += 1;
            }
        }
    }
}

impl DatagramSocket for LossyPort {
    type Addr = UdpAddr;
    type RecvError = Disconnected;
    type SendError = Disconnected;

    /// Cancel-safe: the port's own receive is, and nothing after it waits.
    async fn recv_from(&self, buf: &mut [u8]) -> Result<(usize, UdpAddr), Disconnected> {
        loop {
            let due = self.queue.borrow_mut().ready.pop_front();
            if let Some((from, datagram)) = due {
                let len = datagram.len().min(buf.len());
                buf[..len].copy_from_slice(&datagram[..len]);
                return Ok((len, from));
            }
            let mut datagram = vec![0u8; ww_device::ww_link::UDP_MAX_DATAGRAM_LEN];
            let (len, from) = self.port.recv_from(&mut datagram).await?;
            datagram.truncate(len);
            let fate = (self.impair.locked())(&datagram);
            let mut queue = self.queue.borrow_mut();
            match fate {
                Fate::Deliver => queue.deliver(from, datagram),
                Fate::Drop => {}
                Fate::Delay(later) => {
                    assert!(later > 0, "a delay of at least one datagram");
                    queue.held.push((later, from, datagram));
                }
            }
        }
    }

    async fn send_to(&self, datagram: &[u8], addr: UdpAddr) -> Result<(), Disconnected> {
        self.port.send_to(datagram, addr).await
    }
}
