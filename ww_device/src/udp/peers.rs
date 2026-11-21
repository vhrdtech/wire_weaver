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
//! With one peer ([UdpConnection], [Direct]) the slot's source receives straight into its framer. With several, a
//! source that waits receives into its own framer too, one datagram at a time; a datagram for another slot is copied
//! into the table's handover buffer, and that slot copies it out. Nobody receives while a datagram or a setup
//! datagram waits for its slot: a slow slot holds up the others for as long as its server does not wait, and no
//! datagram is lost to it.
//!
//! The table of several slots is [Shared] in firmware (no lock, one executor) and `Locked` on a computer (feature
//! `std`, `LockedUdpPeers`): `Sync`, so that connections run on any thread of a multi-threaded runtime. Either way
//! the table is only touched between awaits, never while a receive waits.
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
use sealed::{Access, State};

/// One socket shared by the servers of up to `PEERS` hosts, see the [module docs](self).
///
/// `H` is how the slots share the socket: [Shared] (the default) for several in firmware, `Locked` for several on
/// a computer (`LockedUdpPeers`, feature `std`), [Direct] for one ([UdpConnection]). [Shared] and `Locked` take a datagram buffer
/// of [UDP_MAX_DATAGRAM_LEN], a setup datagram (27 bytes) and about 60 bytes per slot, in addition to the socket.
pub struct UdpPeers<
    S: DatagramSocket,
    const PEERS: usize,
    H = Shared<<S as DatagramSocket>::Addr, PEERS>,
> {
    socket: S,
    sharing: H,
}

/// One host on a socket: the slot's source receives straight into its framer, no handover buffer.
pub type UdpConnection<S> = UdpPeers<S, 1, Direct<<S as DatagramSocket>::Addr>>;

/// Up to `PEERS` hosts on a socket, for servers on a computer: `Sync`, so connections run on any thread.
#[cfg(feature = "std")]
pub type LockedUdpPeers<S, const PEERS: usize> =
    UdpPeers<S, PEERS, Locked<<S as DatagramSocket>::Addr, PEERS>>;

/// One slot, which receives from the socket itself.
pub struct Direct<A> {
    peer: Cell<Option<A>>,
    /// Setup datagram of the adopted host, returned by the source's next read
    replay: Cell<Option<SetupDatagram<A>>>,
    /// A host that started link setup, waiting for the source to adopt it
    takeover: Cell<Option<SetupDatagram<A>>>,
}

/// Several slots in firmware: the table in a [RefCell], for sources on one executor.
pub struct Shared<A, const PEERS: usize>(RefCell<State<A, PEERS>>);

/// Several slots on a computer: the table behind a mutex, held only between awaits, for sources on any thread.
#[cfg(feature = "std")]
pub struct Locked<A, const PEERS: usize>(std::sync::Mutex<State<A, PEERS>>);

impl<A: Copy + PartialEq, const PEERS: usize> Access<A, PEERS> for Shared<A, PEERS> {
    fn with<R>(&self, f: impl FnOnce(&mut State<A, PEERS>) -> R) -> R {
        // never borrowed across an await or a call out (wakers are woken after it is released)
        f(&mut self.0.borrow_mut())
    }
}

#[cfg(feature = "std")]
impl<A: Copy + PartialEq, const PEERS: usize> Access<A, PEERS> for Locked<A, PEERS> {
    fn with<R>(&self, f: impl FnOnce(&mut State<A, PEERS>) -> R) -> R {
        // a panic while routing can leave the table half changed: the program restarts the server
        match self.0.lock() {
            Ok(mut state) => f(&mut state),
            // spirit-level: allow(C0005) a poisoned table is not served on, see above
            Err(_) => panic!("udp: a thread panicked while holding the peer table"),
        }
    }
}

struct Slot<A> {
    peer: Option<A>,
    /// Setup datagram of the adopted host, returned by the source's next read
    replay: Option<SetupDatagram<A>>,
    claim: Claim,
    heard_at: u32,
    waker: Option<Waker>,
    /// Ended for a new host ([Claim::Accepted] only): the source fails from now on
    evicted: bool,
}

impl<A> Slot<A> {
    const fn new() -> Self {
        Slot {
            peer: None,
            replay: None,
            claim: Claim::None,
            heard_at: 0,
            waker: None,
            evicted: false,
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
    /// Someone receives from the socket
    Receiving,
    /// The handover buffer holds a datagram of `len` bytes for `slot`
    For {
        slot: usize,
        len: usize,
    },
}

/// Where a received datagram goes.
#[derive(Copy, Clone, PartialEq, Eq)]
enum Route {
    Slot(usize),
    /// Its sender was given a slot, the datagram waits in [State::takeover]
    Takeover,
    Drop,
}

/// What a waiting slot may do next.
enum Turn {
    Evicted,
    Takeover,
    Handover(usize),
    /// Receive from the socket: the receiving role is taken, a [Receiving] gives it back
    Receive,
}

/// The wakers of every slot and of [UdpPeers::accept], taken out of the table to be woken after it is released.
struct Wakers<const PEERS: usize>([Option<Waker>; PEERS], Option<Waker>);

impl<const PEERS: usize> Wakers<PEERS> {
    fn wake(self) {
        for w in self.0.into_iter().flatten() {
            w.wake();
        }
        if let Some(w) = self.1 {
            w.wake();
        }
    }
}

impl<A: Copy + PartialEq, const PEERS: usize> State<A, PEERS> {
    const fn new() -> Self {
        const { assert!(PEERS > 0, "UdpPeers needs at least one slot") };
        State {
            slots: [const { Slot::new() }; PEERS],
            takeover: None,
            buf: [0u8; UDP_MAX_DATAGRAM_LEN],
            handover: Handover::Idle,
            listening: false,
            accept_waker: None,
            tick: 0,
        }
    }

    fn slot(&mut self, slot: usize) -> &mut Slot<A> {
        match self.slots.get_mut(slot) {
            Some(s) => s,
            // spirit-level: allow(C0005) a slot index past PEERS is a bug in the caller, documented under Panics
            None => panic!("udp: slot out of range"),
        }
    }

    fn take_wakers(&mut self) -> Wakers<PEERS> {
        Wakers(
            core::array::from_fn(|i| self.slots[i].waker.take()),
            self.accept_waker.take(),
        )
    }

    /// A source serves `slot` from now on.
    fn claim(&mut self, slot: usize) {
        let s = self.slot(slot);
        match s.claim {
            Claim::None => s.claim = Claim::Fixed,
            Claim::Accepted => {}
            // spirit-level: allow(C0005) two sources on one slot is a bug in the caller, documented under Panics
            Claim::Fixed => panic!("udp: a slot has one source"),
        }
    }

    /// The source of `slot` is gone: the slot is free, and a host or a datagram waiting for it is dropped.
    fn release(&mut self, slot: usize) {
        let s = self.slot(slot);
        s.peer = None;
        s.replay = None;
        s.claim = Claim::None;
        s.evicted = false;
        if self.takeover.is_some_and(|t| t.slot == slot) {
            self.takeover = None;
        }
        if matches!(self.handover, Handover::For { slot: s, .. } if s == slot) {
            self.handover = Handover::Idle;
        }
    }

    /// `slot` serves the host of `setup` from now on.
    fn adopt(&mut self, slot: usize, setup: SetupDatagram<A>) {
        let tick = self.tick;
        let s = self.slot(slot);
        s.peer = Some(setup.from);
        s.heard_at = tick;
        debug!("udp: peer adopted in slot {}", slot);
    }

    /// The setup datagram waiting for `slot`, taken out.
    fn take_takeover(&mut self, slot: usize) -> Option<SetupDatagram<A>> {
        let t = self.takeover.filter(|t| t.slot == slot)?;
        self.takeover = None;
        Some(t.setup)
    }

    /// Nobody receives, and nothing waits for its slot.
    fn may_receive(&self) -> bool {
        self.handover == Handover::Idle && self.takeover.is_none()
    }

    /// What `slot` may do next, `None` to wait (its waker is kept).
    fn turn(&mut self, slot: usize, waker: &Waker) -> Option<Turn> {
        if self.slot(slot).evicted {
            return Some(Turn::Evicted);
        }
        if self.takeover.is_some_and(|t| t.slot == slot) {
            return Some(Turn::Takeover);
        }
        if let Handover::For { slot: to, len } = self.handover
            && to == slot
        {
            return Some(Turn::Handover(len));
        }
        if self.may_receive() {
            self.handover = Handover::Receiving;
            return Some(Turn::Receive);
        }
        self.slot(slot).waker = Some(waker.clone());
        None
    }

    /// Route a datagram the receiver of `me` (None for [UdpPeers::accept]) received: true when it is for `me`,
    /// otherwise it is handed over or kept as a takeover, or dropped. Ends the receiving role.
    fn deliver(&mut self, from: A, datagram: &[u8], me: Option<usize>) -> bool {
        self.handover = Handover::Idle;
        match self.route(from, datagram, me) {
            Route::Slot(slot) if Some(slot) == me => return true,
            Route::Slot(slot) => {
                let len = copy_into(&mut self.buf, datagram);
                self.handover = Handover::For { slot, len };
            }
            Route::Takeover => {}
            // e.g., a host from a previous session that did not notice the takeover yet
            Route::Drop => warn!("udp: dropping datagram from another host"),
        }
        false
    }

    /// Where a datagram from `from` goes; a takeover is kept in [State::takeover]. `me` is the slot of the source
    /// that received it, preferred when free.
    fn route(&mut self, from: A, datagram: &[u8], me: Option<usize>) -> Route {
        let tick = self.tick.wrapping_add(1);
        self.tick = tick;
        if let Some(s) = self.slots.iter_mut().position(|s| s.peer == Some(from)) {
            self.slot(s).heard_at = tick;
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
                if s.claim == Claim::Accepted {
                    debug!(
                        "udp: every slot is taken, ending the connection in slot {}",
                        slot
                    );
                    s.evicted = true;
                    s.peer = None;
                    return Route::Drop;
                }
                slot
            }
        };
        debug!("udp: another host started link setup, slot {}", slot);
        self.takeover = Some(Takeover { slot, setup });
        Route::Takeover
    }

    /// A slot without a host that a source serves or that [UdpPeers::accept] can hand out, `me` first.
    fn free_slot(&self, me: Option<usize>) -> Option<usize> {
        let free = |i: usize| {
            self.slots.get(i).is_some_and(|s| {
                s.peer.is_none()
                    && !s.evicted
                    && match s.claim {
                        Claim::Fixed => true,
                        Claim::None => self.listening,
                        Claim::Accepted => false,
                    }
            })
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
        let tick = self.tick;
        (0..PEERS)
            .filter(|&i| self.slots[i].peer.is_some())
            .max_by_key(|&i| tick.wrapping_sub(self.slots[i].heard_at))
    }
}

impl<S: DatagramSocket, const PEERS: usize, H> UdpPeers<S, PEERS, H> {
    pub fn socket(&self) -> &S {
        &self.socket
    }
}

impl<S: DatagramSocket> UdpConnection<S> {
    pub const fn new(socket: S) -> Self {
        UdpPeers {
            socket,
            sharing: Direct {
                peer: Cell::new(None),
                replay: Cell::new(None),
                takeover: Cell::new(None),
            },
        }
    }

    /// The host the one slot serves, if any.
    ///
    /// # Panics
    ///
    /// `slot` is not 0.
    pub fn peer(&self, slot: usize) -> Option<S::Addr> {
        assert!(slot == 0, "udp: one slot");
        self.sharing.peer.get()
    }

    async fn read_direct(&self, buf: &mut [u8]) -> Result<usize, UdpError<S::RecvError>> {
        if let Some(d) = self.sharing.replay.take() {
            return Ok(copy_into(buf, d.bytes()));
        }
        loop {
            let (len, from) = self.socket.recv_from(buf).await.map_err(UdpError::Socket)?;
            if self.sharing.peer.get() == Some(from) {
                return Ok(len);
            }
            if let Some(setup) = SetupDatagram::new(buf.get(..len).unwrap_or(buf), from) {
                debug!("udp: another host started link setup");
                self.sharing.takeover.set(Some(setup));
                return Err(UdpError::PeerChanged);
            }
            // e.g., a host from a previous session that did not notice the takeover yet
            warn!("udp: dropping datagram from another host");
        }
    }

    async fn wait_connected_direct(&self) {
        loop {
            if let Some(setup) = self.sharing.takeover.take() {
                self.sharing.peer.set(Some(setup.from));
                self.sharing.replay.set(Some(setup));
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
                self.sharing.takeover.set(Some(setup));
            }
        }
    }

    /// The source is gone: the slot is free, and a host waiting for it is dropped.
    fn release_direct(&self) {
        self.sharing.peer.set(None);
        self.sharing.replay.set(None);
        self.sharing.takeover.set(None);
    }
}

impl<S: DatagramSocket, const PEERS: usize> UdpPeers<S, PEERS, Shared<S::Addr, PEERS>> {
    /// # Panics
    ///
    /// At compile time: no slot.
    pub const fn new(socket: S) -> Self {
        UdpPeers {
            socket,
            sharing: Shared(RefCell::new(State::new())),
        }
    }
}

#[cfg(feature = "std")]
impl<S: DatagramSocket, const PEERS: usize> LockedUdpPeers<S, PEERS> {
    /// # Panics
    ///
    /// At compile time: no slot.
    pub const fn new(socket: S) -> Self {
        UdpPeers {
            socket,
            sharing: Locked(std::sync::Mutex::new(State::new())),
        }
    }
}

impl<S: DatagramSocket, const PEERS: usize, H: Access<S::Addr, PEERS>> UdpPeers<S, PEERS, H> {
    fn peer_of(&self, slot: usize) -> Option<S::Addr> {
        self.sharing.with(|t| t.slot(slot).peer)
    }

    fn wake_all(&self) {
        self.sharing.with(State::take_wakers).wake();
    }

    fn release(&self, slot: usize) {
        self.sharing.with(|t| t.release(slot));
        self.wake_all();
    }

    /// Where `slot` sends to.
    fn send_to(&self, slot: usize) -> Result<S::Addr, UdpError<S::SendError>> {
        self.sharing.with(|t| {
            let s = t.slot(slot);
            if s.evicted {
                return Err(UdpError::PeerChanged);
            }
            s.peer.ok_or(UdpError::NoPeer)
        })
    }

    /// The next host to serve: a slot to make a [UdpSource] / [UdpSink] pair for, whose source adopts the host in
    /// its first `wait_connected()`. Make them before calling `accept()` again: until the source adopts the host,
    /// nothing else is received. The slot is free again when that source is dropped, so a connection is served
    /// once. Receives into `buf` while no source does. Cancel-safe.
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
    pub async fn accept(
        &self,
        buf: &mut [u8; UDP_MAX_DATAGRAM_LEN],
    ) -> Result<usize, S::RecvError> {
        self.sharing.with(|t| t.listening = true);
        loop {
            let accepted = poll_fn(|cx| {
                self.sharing.with(|t| {
                    if let Some(to) = t.takeover
                        && t.slot(to.slot).claim == Claim::None
                    {
                        t.slot(to.slot).claim = Claim::Accepted;
                        return Poll::Ready(Some(to.slot));
                    }
                    if t.may_receive() {
                        t.handover = Handover::Receiving;
                        return Poll::Ready(None);
                    }
                    t.accept_waker = Some(cx.waker().clone());
                    Poll::Pending
                })
            })
            .await;
            if let Some(slot) = accepted {
                debug!("udp: accepted a host in slot {}", slot);
                return Ok(slot);
            }
            let receiving = Receiving(self);
            let (len, from) = self.socket.recv_from(buf).await?;
            let datagram = buf.get(..len).unwrap_or(buf);
            self.sharing.with(|t| t.deliver(from, datagram, None));
            drop(receiving);
        }
    }

    /// Wait until `slot` has something, or may receive.
    async fn turn(&self, slot: usize) -> Turn {
        poll_fn(|cx| match self.sharing.with(|t| t.turn(slot, cx.waker())) {
            Some(turn) => Poll::Ready(turn),
            None => Poll::Pending,
        })
        .await
    }

    async fn read_shared(
        &self,
        slot: usize,
        buf: &mut [u8],
    ) -> Result<usize, UdpError<S::RecvError>> {
        if let Some(d) = self.sharing.with(|t| t.slot(slot).replay.take()) {
            return Ok(copy_into(buf, d.bytes()));
        }
        loop {
            match self.turn(slot).await {
                Turn::Evicted => return Err(UdpError::PeerChanged),
                Turn::Takeover => {
                    // a slot without a host takes its first one at once, one serving a host goes down first
                    let first = self.sharing.with(|t| {
                        if t.slot(slot).peer.is_some() {
                            return None;
                        }
                        let setup = t.take_takeover(slot)?;
                        t.adopt(slot, setup);
                        Some(setup)
                    });
                    let Some(setup) = first else {
                        return Err(UdpError::PeerChanged);
                    };
                    self.wake_all();
                    return Ok(copy_into(buf, setup.bytes()));
                }
                Turn::Handover(len) => {
                    let len = self.sharing.with(|t| {
                        t.handover = Handover::Idle;
                        copy_into(buf, t.buf.get(..len).unwrap_or(&t.buf))
                    });
                    self.wake_all();
                    return Ok(len);
                }
                Turn::Receive => {
                    let receiving = Receiving(self);
                    let (len, from) = self.socket.recv_from(buf).await.map_err(UdpError::Socket)?;
                    let len = len.min(buf.len());
                    let datagram = buf.get(..len).unwrap_or(buf);
                    let mine = self.sharing.with(|t| t.deliver(from, datagram, Some(slot)));
                    drop(receiving);
                    if mine {
                        return Ok(len);
                    }
                }
            }
        }
    }

    /// Up once `slot` may read: a host given to it is adopted. A slot served for good is up without a host, like a
    /// medium without connections: its reads adopt the first host. A connection whose host was taken from it never
    /// comes up again.
    async fn wait_connected_shared(&self, slot: usize) {
        let up = poll_fn(|cx| {
            self.sharing.with(|t| {
                if t.slot(slot).evicted {
                    return Poll::Ready(false);
                }
                if let Some(setup) = t.take_takeover(slot) {
                    t.adopt(slot, setup);
                    t.slot(slot).replay = Some(setup);
                    return Poll::Ready(true);
                }
                let s = t.slot(slot);
                if s.claim == Claim::Fixed || s.peer.is_some() {
                    return Poll::Ready(true);
                }
                s.waker = Some(cx.waker().clone());
                Poll::Pending
            })
        })
        .await;
        self.wake_all();
        if !up {
            // a connection serves one host
            pending::<()>().await;
        }
    }
}

/// Holds the receiving role: ends it when dropped, also when the receive is cancelled, and wakes whoever waits.
struct Receiving<'p, S: DatagramSocket, const PEERS: usize, H: Access<S::Addr, PEERS>>(
    &'p UdpPeers<S, PEERS, H>,
);

impl<S: DatagramSocket, const PEERS: usize, H: Access<S::Addr, PEERS>> Drop
    for Receiving<'_, S, PEERS, H>
{
    fn drop(&mut self) {
        self.0.sharing.with(|t| {
            if t.handover == Handover::Receiving {
                t.handover = Handover::Idle;
            }
        });
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
/// any reference: `&UdpPeers` in firmware, `Arc<LockedUdpPeers>` for connections on tasks of their own.
pub trait PeerTable: sealed::Table {}

impl<S: DatagramSocket, const PEERS: usize, H> PeerTable for UdpPeers<S, PEERS, H> where
    Self: sealed::Table
{
}

pub(super) mod sealed {
    use core::future::Future;
    use core::task::Waker;

    use ww_link::UDP_MAX_DATAGRAM_LEN;

    use super::super::{DatagramSocket, UdpError};
    #[cfg(feature = "std")]
    use super::Locked;
    use super::{Handover, Shared, Slot, Takeover, UdpConnection, UdpPeers};

    /// The table of a [UdpPeers] of several slots, changed only through [Access::with].
    pub struct State<A, const PEERS: usize> {
        pub(super) slots: [Slot<A>; PEERS],
        /// A host given a slot, waiting for that slot's source to adopt it
        pub(super) takeover: Option<Takeover<A>>,
        /// The handover buffer: a datagram received by one slot's source for another
        pub(super) buf: [u8; UDP_MAX_DATAGRAM_LEN],
        pub(super) handover: Handover,
        /// [UdpPeers::accept] was called: free slots without a source can be given to new hosts
        pub(super) listening: bool,
        pub(super) accept_waker: Option<Waker>,
        /// Datagrams routed so far, wrapping: when each slot was last heard from
        pub(super) tick: u32,
    }

    /// How the sources of several slots reach their [State]: run `f` on it, alone.
    pub trait Access<A, const PEERS: usize> {
        fn with<R>(&self, f: impl FnOnce(&mut State<A, PEERS>) -> R) -> R;
    }

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

    impl<S: DatagramSocket> Table for UdpConnection<S> {
        type Socket = S;

        /// The one slot is 0. Several sources on it are not told apart, as before slots existed.
        fn claim(&self, slot: usize) {
            assert!(slot == 0, "udp: one slot");
        }

        fn release(&self, _slot: usize) {
            self.release_direct();
        }

        fn socket(&self) -> &S {
            &self.socket
        }

        fn send_to(&self, _slot: usize) -> Result<S::Addr, UdpError<S::SendError>> {
            self.sharing.peer.get().ok_or(UdpError::NoPeer)
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

    /// [Table] and `peer` of a table of several slots, `$sharing` the [Access] it is behind. One impl per sharing
    /// type: a generic one over [Access] would overlap [UdpConnection]'s.
    macro_rules! several_slots {
        ($sharing:ident) => {
            impl<S: DatagramSocket, const PEERS: usize>
                UdpPeers<S, PEERS, $sharing<S::Addr, PEERS>>
            {
                /// The host `slot` serves, if any.
                ///
                /// # Panics
                ///
                /// `slot` is not below `PEERS`.
                pub fn peer(&self, slot: usize) -> Option<S::Addr> {
                    self.peer_of(slot)
                }
            }

            impl<S: DatagramSocket, const PEERS: usize> Table
                for UdpPeers<S, PEERS, $sharing<S::Addr, PEERS>>
            {
                type Socket = S;

                fn claim(&self, slot: usize) {
                    self.sharing.with(|t| t.claim(slot));
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
        };
    }

    several_slots!(Shared);
    #[cfg(feature = "std")]
    several_slots!(Locked);
}
