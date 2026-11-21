//! `wire_weaver_server` on a calloop event loop: the clock, a TCP source and a UDP listener the loop drives.
//!
//! The server itself needs no runtime: [Server::serve](wire_weaver_server::Server::serve) is a future, scheduled on
//! the loop's executor (`calloop::futures::executor`, feature `executor` of calloop). This crate gives it what
//! comes from the loop:
//!
//! - [LoopClock]: timers on the loop's timers;
//! - [TcpIncoming]: TCP clients, for [listen](wire_weaver_server::byte_stream::listen) with WebSocket
//!   ([WsFraming](wire_weaver_server::byte_stream::WsFraming)) or preamble framing;
//! - [bind_udp]: a [UdpListener](wire_weaver_server::udp::UdpListener) whose socket the loop watches.
//!
//! Each listener stops alone while the program runs, to move to another address: [TcpIncoming::stop_handle] closes
//! its socket at once, `UdpListener::stop_handle` ([LoopUdpStop]) takes no new host. Their connections go on.
//!
//! ```ignore
//! let (executor, scheduler) = calloop::futures::executor()?;
//! event_loop.handle().insert_source(executor, |(), _, _| ())?;
//! let handle = event_loop.handle();
//! let clock = LoopClock::new(handle.clone());
//! let ws = listen(TcpIncoming::bind(&handle, "0.0.0.0:7000")?, WsFraming::new(MESSAGE_LEN_MAX), clock.clone());
//! let udp = bind_udp::<_, 8>(&handle, "0.0.0.0:7000".parse()?, MESSAGE_LEN_MAX)?;
//! let server = Server::new(clock);
//! scheduler.schedule(async move {
//!     let _ = join(
//!         server.serve(link(), ws, make, shutdown.clone()),
//!         server.serve(link(), udp, make, shutdown),
//!     )
//!     .await;
//! })?;
//! ```
//!
//! Connections run inside `serve` on the loop's thread, so backends need not be `Send`.

mod clock;
mod tcp;
mod udp;

pub use clock::LoopClock;
pub use tcp::{TcpIncoming, TcpStop};
pub use udp::{LoopReadiness, LoopUdpListener, LoopUdpSocket, LoopUdpStop, bind_udp};
