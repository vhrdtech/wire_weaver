//! The peer table: which host each slot serves, and who receives from the shared socket.
//!
//! One socket serves up to `PEERS` hosts, one slot each. A slot is served by one [UdpSource] / [UdpSink] pair,
//! either for good (a firmware's fixed set of servers, [UdpSource::new]) or for one connection
//! ([UdpPeers::accept], then [UdpSource::new]; the slot is freed when the source is dropped).
//!
//! Routing, for each datagram received:
//! - from a host a slot serves: that slot's;
//! - from another host, starting link setup: the host gets a free slot, or the one heard from least recently. Its
//!   setup datagram waits in the table until that slot's source adopts it. A slot served for good is taken over at
//!   once, as if a USB device was opened by another process; a connection is ended instead, and the host's next
//!   hello takes the slot once the connection is dropped;
//! - anything else is dropped.
//!
//! With one peer ([UdpConnection], [Direct]) the slot's source receives straight into its framer. With several
//! ([Shared]), whoever waits receives into the table's handover buffer, one datagram at a time, and the slot it is
//! for copies it out. Nobody receives while a datagram or a setup datagram waits for its slot: a slow slot holds up
//! the others for as long as its server does not wait, and no datagram is lost to it.
//!
//! [UdpSource]: super::UdpSource
//! [UdpSink]: super::UdpSink
//! [UdpSource::new]: super::UdpSource::new

use core::cell::{Cell, RefCell};
use core::future::{pending, poll_fn};
use core::task::{Poll, Waker};

use ww_link::UDP_MAX_DATAGRAM_LEN;

use super::{DatagramSocket, SETUP_DATAGRAM_MAX_LEN, SetupDatagram, UdpError};
use crate::fmt::{debug, warn};

/// One socket shared by the servers of up to `PEERS` hosts, see the [module docs](self).
///
/// `H` is how the slots share the socket: [Shared] (the default) for several, [Direct] for one ([UdpConnection]).
/// Takes an address, a setup datagram (27 bytes) and a few bytes per slot, plus [Shared]'s datagram buffer and about
/// 20 bytes per slot, in addition to the socket.
pub struct UdpPeers<S: DatagramSocket, const PEERS: usize, H = Shared<PEERS>> {
    socket: S,
    slots: [Slot<S::Addr>; PEERS],
    /// A host given a slot, waiting for that slot's source to adopt it
    takeover: Cell<Option<Takeover<S::Addr>>>,
    sharing: H,
}

/// One host on a socket: the slot's source receives straight into its framer, no handover buffer.
pub type UdpConnection<S> = UdpPeers<S, 1, Direct>;

/// One slot, which receives from the socket itself.
pub struct Direct;

/// Several slots: one datagram is received at a time, into a buffer of [UDP_MAX_DATAGRAM_LEN], and handed over to
/// the slot it is for.
pub struct Shared<const PEERS: usize> {
    buf: RefCell<[u8; UDP_MAX_DATAGRAM_LEN]>,
    state: Cell<Handover>,
    /// [UdpPeers::accept] was called: free slots without a source can be given to new hosts
    listening: Cell<bool>,
    accept_waker: Cell<Option<Waker>>,
    /// Datagrams routed so far, wrapping: when each slot was last heard from
    tick: Cell<u32>,
    slots: [SharedSlot; PEERS],
}

impl<const PEERS: usize> Shared<PEERS> {
    const fn new() -> Self {
        Shared {
            buf: RefCell::new([0u8; UDP_MAX_DATAGRAM_LEN]),
            state: Cell::new(Handover::Idle),
            listening: Cell::new(false),
            accept_waker: Cell::new(None),
            tick: Cell::new(0),
            slots: [const { SharedSlot::new() }; PEERS],
        }
    }
}

struct Slot<A> {
    peer: Cell<Option<A>>,
    /// Setup datagram of the adopted host, returned by the source's next read
    replay: Cell<Option<SetupDatagram<A>>>,
    claim: Cell<Claim>,
}

impl<A> Slot<A> {
    const fn new() -> Self {
        Slot {
            peer: Cell::new(None),
            replay: Cell::new(None),
            claim: Cell::new(Claim::None),
        }
    }
}

struct SharedSlot {
    heard_at: Cell<u32>,
    waker: Cell<Option<Waker>>,
    /// Ended for a new host ([Claim::Accepted] only): the source fails from now on
    evicted: Cell<bool>,
}

impl SharedSlot {
    const fn new() -> Self {
        SharedSlot {
            heard_at: Cell::new(0),
            waker: Cell::new(None),
            evicted: Cell::new(false),
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum Claim {
    /// No source
    None,
    /// A source for good: it adopts every host given to its slot
    Fixed,
    /// An accepted connection: it serves one host
    Accepted,
}

#[derive(Copy, Clone)]
struct Takeover<A> {
    slot: usize,
    setup: SetupDatagram<A>,
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum Handover {
    Idle,
    /// Someone receives into the handover buffer
    Receiving,
    /// The buffer holds a datagram of `len` bytes for `slot`
    For {
        slot: usize,
        len: usize,
    },
}

/// Where a received datagram goes.
#[derive(Copy, Clone, PartialEq, Eq)]
enum Route {
    Slot(usize),
    /// Its sender was given this slot, the datagram waits in [UdpPeers::takeover]
    Takeover(usize),
    Drop,
}

/// What a waiting slot may do next.
enum Turn {
    Evicted,
    Takeover,
    Handover(usize),
    Receive,
}

impl<S: DatagramSocket, const PEERS: usize, H> UdpPeers<S, PEERS, H> {
    const fn with_sharing(socket: S, sharing: H) -> Self {
        UdpPeers {
            socket,
            slots: [const { Slot::new() }; PEERS],
            takeover: Cell::new(None),
            sharing,
        }
    }

    /// The host `slot` serves, if any.
    ///
    /// # Panics
    ///
    /// `slot` is not below `PEERS`.
    pub fn peer(&self, slot: usize) -> Option<S::Addr> {
        self.slot(slot).peer.get()
    }

    pub fn socket(&self) -> &S {
        &self.socket
    }

    fn slot(&self, slot: usize) -> &Slot<S::Addr> {
        match self.slots.get(slot) {
            Some(s) => s,
            // spirit-level: allow(C0005) a slot index past PEERS is a bug in the caller, documented under Panics
            None => panic!("udp: slot out of range"),
        }
    }

    /// A source serves `slot` from now on.
    fn claim(&self, slot: usize) {
        let s = self.slot(slot);
        match s.claim.get() {
            Claim::None => s.claim.set(Claim::Fixed),
            Claim::Accepted => {}
            // spirit-level: allow(C0005) two sources on one slot is a bug in the caller, documented under Panics
            Claim::Fixed => panic!("udp: a slot has one source"),
        }
    }

    /// The source of `slot` is gone: the slot is free, and a host waiting for it is dropped.
    fn release_slot(&self, slot: usize) {
        let s = self.slot(slot);
        s.peer.set(None);
        s.replay.set(None);
        s.claim.set(Claim::None);
        if self.takeover.get().is_some_and(|t| t.slot == slot) {
            self.takeover.set(None);
        }
    }

    /// The setup datagram waiting for `slot`, taken out.
    fn take_takeover(&self, slot: usize) -> Option<SetupDatagram<S::Addr>> {
        let t = self.takeover.get().filter(|t| t.slot == slot)?;
        self.takeover.set(None);
        Some(t.setup)
    }
}

impl<S: DatagramSocket> UdpPeers<S, 1, Direct> {
    pub const fn new(socket: S) -> Self {
        Self::with_sharing(socket, Direct)
    }

    async fn read_direct(&self, buf: &mut [u8]) -> Result<usize, UdpError<S::RecvError>> {
        if let Some(d) = self.slots[0].replay.take() {
            return Ok(copy_into(buf, d.bytes()));
        }
        loop {
            let (len, from) = self.socket.recv_from(buf).await.map_err(UdpError::Socket)?;
            if self.slots[0].peer.get() == Some(from) {
                return Ok(len);
            }
            if let Some(setup) = SetupDatagram::new(buf.get(..len).unwrap_or(buf), from) {
                debug!("udp: another host started link setup");
                self.takeover.set(Some(Takeover { slot: 0, setup }));
                return Err(UdpError::PeerChanged);
            }
            // e.g., a host from a previous session that did not notice the takeover yet
            warn!("udp: dropping datagram from another host");
        }
    }

    async fn wait_connected_direct(&self) {
        loop {
            if let Some(setup) = self.take_takeover(0) {
                self.slots[0].peer.set(Some(setup.from));
                self.slots[0].replay.set(Some(setup));
                debug!("udp: peer adopted");
                return;
            }
            // one more byte, to tell a truncated datagram from one that fits exactly
            let mut buf = [0u8; SETUP_DATAGRAM_MAX_LEN + 1];
            // an error (e.g., too long for buf) is not a datagram that starts link setup either; link setup from the
            // host already served starts over, from anyone else it takes over
            if let Ok((len, from)) = self.socket.recv_from(&mut buf).await
                && let Some(setup) = SetupDatagram::received(&buf, len, from)
            {
                self.takeover.set(Some(Takeover { slot: 0, setup }));
            }
        }
    }
}

impl<S: DatagramSocket, const PEERS: usize> UdpPeers<S, PEERS, Shared<PEERS>> {
    /// # Panics
    ///
    /// At compile time: no slot.
    pub const fn new(socket: S) -> Self {
        const { assert!(PEERS > 0, "UdpPeers needs at least one slot") };
        Self::with_sharing(socket, Shared::new())
    }

    fn shared(&self, slot: usize) -> &SharedSlot {
        match self.sharing.slots.get(slot) {
            Some(s) => s,
            // spirit-level: allow(C0005) a slot index past PEERS is a bug in the caller, documented under Panics
            None => panic!("udp: slot out of range"),
        }
    }

    fn release(&self, slot: usize) {
        self.release_slot(slot);
        self.shared(slot).evicted.set(false);
        if matches!(self.sharing.state.get(), Handover::For { slot: s, .. } if s == slot) {
            self.sharing.state.set(Handover::Idle);
        }
        self.wake_all();
    }

    /// Where `slot` sends to.
    fn send_to(&self, slot: usize) -> Result<S::Addr, UdpError<S::SendError>> {
        if self.shared(slot).evicted.get() {
            return Err(UdpError::PeerChanged);
        }
        self.slot(slot).peer.get().ok_or(UdpError::NoPeer)
    }

    /// `slot` serves the host of `setup` from now on, its next read returns `setup`.
    fn adopt(&self, slot: usize, setup: SetupDatagram<S::Addr>) {
        let s = self.slot(slot);
        s.peer.set(Some(setup.from));
        s.replay.set(Some(setup));
        self.shared(slot).heard_at.set(self.sharing.tick.get());
        debug!("udp: peer adopted in slot {}", slot);
        self.wake_all();
    }

    fn wake_all(&self) {
        for s in &self.sharing.slots {
            if let Some(w) = s.waker.take() {
                w.wake();
            }
        }
        if let Some(w) = self.sharing.accept_waker.take() {
            w.wake();
        }
    }

    /// The next host to serve: a slot to make a [UdpSource] / [UdpSink] pair for, whose source adopts the host in
    /// its first `wait_connected()`. Make them before calling `accept()` again: until the source adopts the host,
    /// nothing else is received. The slot is free again when that source is dropped, so a connection is served
    /// once. Cancel-safe.
    ///
    /// When every slot is taken, a new host ends the connection heard from least recently and is served once that
    /// connection is dropped, from its next hello.
    ///
    /// # Errors
    ///
    /// Receiving from the socket failed.
    ///
    /// [UdpSource]: super::UdpSource
    /// [UdpSink]: super::UdpSink
    pub async fn accept(&self) -> Result<usize, S::RecvError> {
        self.sharing.listening.set(true);
        loop {
            let accepted = poll_fn(|cx| {
                if let Some(t) = self.takeover.get()
                    && self.slot(t.slot).claim.get() == Claim::None
                {
                    self.slot(t.slot).claim.set(Claim::Accepted);
                    return Poll::Ready(Some(t.slot));
                }
                if self.may_receive() {
                    return Poll::Ready(None);
                }
                self.sharing.accept_waker.set(Some(cx.waker().clone()));
                Poll::Pending
            })
            .await;
            if let Some(slot) = accepted {
                debug!("udp: accepted a host in slot {}", slot);
                return Ok(slot);
            }
            self.receive(None).await?;
        }
    }

    async fn read_shared(
        &self,
        slot: usize,
        buf: &mut [u8],
    ) -> Result<usize, UdpError<S::RecvError>> {
        if let Some(d) = self.slot(slot).replay.take() {
            return Ok(copy_into(buf, d.bytes()));
        }
        loop {
            match self.turn(slot).await {
                Turn::Evicted | Turn::Takeover => return Err(UdpError::PeerChanged),
                Turn::Handover(len) => {
                    let len = copy_into(buf, &self.sharing.buf.borrow()[..len]);
                    self.sharing.state.set(Handover::Idle);
                    self.wake_all();
                    return Ok(len);
                }
                Turn::Receive => self.receive(Some(slot)).await.map_err(UdpError::Socket)?,
            }
        }
    }

    async fn wait_connected_shared(&self, slot: usize) {
        loop {
            match self.turn(slot).await {
                // a connection serves one host
                Turn::Evicted => pending::<()>().await,
                Turn::Takeover => {
                    if let Some(setup) = self.take_takeover(slot) {
                        self.adopt(slot, setup);
                        return;
                    }
                }
                Turn::Handover(len) => {
                    // link setup from the host already served starts over
                    let setup = self.peer(slot).and_then(|from| {
                        SetupDatagram::new(&self.sharing.buf.borrow()[..len], from)
                    });
                    self.sharing.state.set(Handover::Idle);
                    self.wake_all();
                    if let Some(setup) = setup {
                        self.adopt(slot, setup);
                        return;
                    }
                }
                // an error is not a host to adopt either: the next turn receives again
                Turn::Receive => _ = self.receive(Some(slot)).await,
            }
        }
    }

    /// Where a datagram from `from` goes; a takeover is kept in [Self::takeover]. `me` is the slot of the source
    /// that received it, preferred when free.
    fn route(&self, from: S::Addr, datagram: &[u8], me: Option<usize>) -> Route {
        let tick = self.sharing.tick.get().wrapping_add(1);
        self.sharing.tick.set(tick);
        if let Some(s) = self.slots.iter().position(|s| s.peer.get() == Some(from)) {
            self.shared(s).heard_at.set(tick);
            return Route::Slot(s);
        }
        let Some(setup) = SetupDatagram::new(datagram, from) else {
            return Route::Drop;
        };
        let slot = match self.free_slot(me) {
            Some(slot) => slot,
            None => {
                let Some(slot) = self.least_recent() else {
                    return Route::Drop;
                };
                let s = self.slot(slot);
                if s.claim.get() == Claim::Accepted {
                    debug!(
                        "udp: every slot is taken, ending the connection in slot {}",
                        slot
                    );
                    self.shared(slot).evicted.set(true);
                    s.peer.set(None);
                    self.wake_all();
                    return Route::Drop;
                }
                slot
            }
        };
        debug!("udp: another host started link setup, slot {}", slot);
        self.takeover.set(Some(Takeover { slot, setup }));
        self.wake_all();
        Route::Takeover(slot)
    }

    /// A slot without a host that a source serves or that [Self::accept] can hand out, `me` first.
    fn free_slot(&self, me: Option<usize>) -> Option<usize> {
        let listening = self.sharing.listening.get();
        let free = |i: usize| {
            let s = self.slot(i);
            s.peer.get().is_none()
                && !self.shared(i).evicted.get()
                && match s.claim.get() {
                    Claim::Fixed => true,
                    Claim::None => listening,
                    Claim::Accepted => false,
                }
        };
        if let Some(me) = me
            && free(me)
        {
            return Some(me);
        }
        (0..PEERS).find(|&i| free(i))
    }

    /// The slot with a host heard from least recently.
    fn least_recent(&self) -> Option<usize> {
        let tick = self.sharing.tick.get();
        (0..PEERS)
            .filter(|&i| self.slot(i).peer.get().is_some())
            .max_by_key(|&i| tick.wrapping_sub(self.shared(i).heard_at.get()))
    }

    /// Nobody receives, and nothing waits for its slot.
    fn may_receive(&self) -> bool {
        self.sharing.state.get() == Handover::Idle && self.takeover.get().is_none()
    }

    /// Wait until `slot` has something, or may receive.
    async fn turn(&self, slot: usize) -> Turn {
        poll_fn(|cx| {
            let s = self.shared(slot);
            if s.evicted.get() {
                return Poll::Ready(Turn::Evicted);
            }
            if self.takeover.get().is_some_and(|t| t.slot == slot) {
                return Poll::Ready(Turn::Takeover);
            }
            if let Handover::For { slot: to, len } = self.sharing.state.get()
                && to == slot
            {
                return Poll::Ready(Turn::Handover(len));
            }
            if self.may_receive() {
                return Poll::Ready(Turn::Receive);
            }
            s.waker.set(Some(cx.waker().clone()));
            Poll::Pending
        })
        .await
    }

    /// Receive one datagram into the handover buffer and route it. `me` is the receiving source's slot, None for
    /// [Self::accept].
    #[expect(
        clippy::await_holding_refcell_ref,
        reason = "the buffer is borrowed only in Handover::Receiving, which this receive holds, and in Handover::For"
    )]
    async fn receive(&self, me: Option<usize>) -> Result<(), S::RecvError> {
        let receiving = Receiving(self);
        self.sharing.state.set(Handover::Receiving);
        let mut buf = self.sharing.buf.borrow_mut();
        // spirit-level: allow(T0001) the buffer is only borrowed in Handover::Receiving (held here) and Handover::For
        let (len, from) = self.socket.recv_from(&mut buf[..]).await?;
        let len = len.min(UDP_MAX_DATAGRAM_LEN);
        match self.route(from, &buf[..len], me) {
            Route::Slot(slot) => self.sharing.state.set(Handover::For { slot, len }),
            Route::Takeover(_) => {}
            // e.g., a host from a previous session that did not notice the takeover yet
            Route::Drop => warn!("udp: dropping datagram from another host"),
        }
        drop(buf);
        drop(receiving);
        Ok(())
    }
}

/// Holds the receiving role: ends it when dropped, also when the receive is cancelled, and wakes whoever waits.
struct Receiving<'p, S: DatagramSocket, const PEERS: usize>(&'p UdpPeers<S, PEERS, Shared<PEERS>>);

impl<S: DatagramSocket, const PEERS: usize> Drop for Receiving<'_, S, PEERS> {
    fn drop(&mut self) {
        if self.0.sharing.state.get() == Handover::Receiving {
            self.0.sharing.state.set(Handover::Idle);
        }
        self.0.wake_all();
    }
}

/// Copy `datagram` to the start of `buf`, cut to fit (as a socket truncates); returns the length copied.
fn copy_into(buf: &mut [u8], datagram: &[u8]) -> usize {
    let len = datagram.len().min(buf.len());
    if let (Some(to), Some(from)) = (buf.get_mut(..len), datagram.get(..len)) {
        to.copy_from_slice(from);
    }
    len
}

/// What [UdpSource](super::UdpSource) and [UdpSink](super::UdpSink) need of a [UdpPeers], so that they take it by
/// any reference: `&UdpPeers` in firmware, `Rc<UdpPeers>` for connections on tasks of their own.
pub trait PeerTable: sealed::Table {}

impl<S: DatagramSocket, const PEERS: usize, H> PeerTable for UdpPeers<S, PEERS, H> where
    Self: sealed::Table
{
}

pub(super) mod sealed {
    use core::future::Future;

    use super::super::{DatagramSocket, UdpError};
    use super::{Direct, Shared, UdpPeers};

    pub trait Table {
        type Socket: DatagramSocket;

        fn claim(&self, slot: usize);
        fn release(&self, slot: usize);
        fn socket(&self) -> &Self::Socket;
        /// The host `slot` sends to.
        fn send_to(
            &self,
            slot: usize,
        ) -> Result<
            <Self::Socket as DatagramSocket>::Addr,
            UdpError<<Self::Socket as DatagramSocket>::SendError>,
        >;
        fn read(
            &self,
            slot: usize,
            buf: &mut [u8],
        ) -> impl Future<Output = Result<usize, UdpError<<Self::Socket as DatagramSocket>::RecvError>>>;
        fn wait_connected(&self, slot: usize) -> impl Future<Output = ()>;
    }

    impl<S: DatagramSocket> Table for UdpPeers<S, 1, Direct> {
        type Socket = S;

        /// The one slot is 0. Several sources on it are not told apart, as before slots existed.
        fn claim(&self, slot: usize) {
            assert!(slot == 0, "udp: one slot");
        }

        fn release(&self, slot: usize) {
            self.release_slot(slot);
        }

        fn socket(&self) -> &S {
            &self.socket
        }

        fn send_to(&self, _slot: usize) -> Result<S::Addr, UdpError<S::SendError>> {
            self.slots[0].peer.get().ok_or(UdpError::NoPeer)
        }

        /// The one slot is 0, checked by [Table::claim].
        fn read(
            &self,
            _slot: usize,
            buf: &mut [u8],
        ) -> impl Future<Output = Result<usize, UdpError<S::RecvError>>> {
            self.read_direct(buf)
        }

        fn wait_connected(&self, _slot: usize) -> impl Future<Output = ()> {
            self.wait_connected_direct()
        }
    }

    impl<S: DatagramSocket, const PEERS: usize> Table for UdpPeers<S, PEERS, Shared<PEERS>> {
        type Socket = S;

        fn claim(&self, slot: usize) {
            UdpPeers::claim(self, slot);
        }

        fn release(&self, slot: usize) {
            UdpPeers::release(self, slot);
        }

        fn socket(&self) -> &S {
            &self.socket
        }

        fn send_to(&self, slot: usize) -> Result<S::Addr, UdpError<S::SendError>> {
            UdpPeers::send_to(self, slot)
        }

        fn read(
            &self,
            slot: usize,
            buf: &mut [u8],
        ) -> impl Future<Output = Result<usize, UdpError<S::RecvError>>> {
            self.read_shared(slot, buf)
        }

        fn wait_connected(&self, slot: usize) -> impl Future<Output = ()> {
            self.wait_connected_shared(slot)
        }
    }
}
