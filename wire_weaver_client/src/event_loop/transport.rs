//! Message-level transport abstraction between the sans-IO cores ([TxCore](super::core::TxCore),
//! [RxCore](super::core::RxCore)) and a medium.
//!
//! Cores speak in [ww_link] messages `(kind, bytes)`. How those get onto the wire is up to the
//! implementation: USB packs them into packets with [ww_framer], WebSocket relies on its own
//! framing and sends one message per ws frame, etc.

use crate::event_loop::DeviceHandle;

/// Message tx half.
pub(crate) trait MessageTx: Send + 'static {
    /// Write a message. Implementations may hold it back until a frame is full or [flush](Self::flush) is called.
    fn write_message(
        &mut self,
        kind: u8,
        message: &[u8],
    ) -> impl Future<Output = Result<(), String>> + Send;

    /// Send whatever is held back, even if the frame is not full. No-op if nothing is pending.
    fn flush(&mut self) -> impl Future<Output = Result<(), String>> + Send;

    /// Release the medium. Returns once it is actually released, so that a new connection can open it again
    /// right away (e.g., claim the same USB interface).
    fn close(self) -> impl Future<Output = ()> + Send
    where
        Self: Sized,
    {
        async {}
    }
}

/// Message rx half.
pub(crate) trait MessageRx: Send + 'static {
    /// Wait for the next message and return it, borrowed from the implementation's buffer.
    /// The message stays valid until the next call.
    ///
    /// Must be cancel-safe: dropping the future must not lose data. This is natural when
    /// any partially received state lives in `self` (e.g., a framer's staging buffer), not in the future.
    fn read_message(&mut self) -> impl Future<Output = Result<(u8, &[u8]), String>> + Send;

    /// Same as [MessageTx::close].
    fn close(self) -> impl Future<Output = ()> + Send
    where
        Self: Sized,
    {
        async {}
    }
}

pub(crate) struct Opened<Tx, Rx> {
    pub tx: Tx,
    pub rx: Rx,
}

/// Opens a connection for a handle from [Command](super::command::Command)`::Connect` and provides
/// message tx and rx halves. Called once per event loop: to re-connect, tear the loop down and start
/// a new one with the residual, possibly with a different transport.
pub(crate) trait Transport {
    type Tx: MessageTx;
    type Rx: MessageRx;
    fn connect(&mut self, handle: DeviceHandle) -> Result<Opened<Self::Tx, Self::Rx>, String>;
}
