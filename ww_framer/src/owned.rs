//! Owned variants of [Tx] and [FramedRx] backed by [Vec], for `std` hosts.
//!
//! Both are thin wrappers: on each call the borrowed framer is re-created from the owned buffer
//! and the saved state ([Tx::from_parts] / [FramedRx::from_parts]), the call is delegated and the
//! state is saved back. No framing logic lives here.

use alloc::vec;
use alloc::vec::Vec;
use core::ops::Range;

use crate::framed_rx::{FramedRx, FramedRxState};
use crate::traits::{Checksum, Head, Tail};
use crate::tx::{Tx, TxState};

/// Owned [Tx], see its docs for the intended use.
pub struct TxOwned<H, C, T> {
    buf: Vec<u8>,
    state: TxState,
    _phantom: core::marker::PhantomData<(H, C, T)>,
}

impl<H: Head, C: Checksum, T: Tail> TxOwned<H, C, T>
where
    H::UserKind: Copy,
{
    /// Create new framer with an assembly buffer of exactly `frame_size` bytes
    /// (maximum frame or DMA size).
    pub fn new(frame_size: usize) -> Self {
        let mut buf = vec![0u8; frame_size];
        let state = Tx::<H, C, T>::new(&mut buf).into_parts();
        TxOwned {
            buf,
            state,
            _phantom: core::marker::PhantomData,
        }
    }

    fn with<R>(&mut self, f: impl FnOnce(&mut Tx<'_, H, C, T>) -> R) -> R {
        let mut tx = Tx::<H, C, T>::from_parts(&mut self.buf, self.state);
        let r = f(&mut tx);
        self.state = tx.into_parts();
        r
    }

    /// See [Tx::write].
    pub fn write(&mut self, user_kind: H::UserKind, message: &[u8]) -> Result<bool, ()> {
        self.with(|tx| tx.write(user_kind, message))
    }

    /// See [Tx::flush].
    pub fn flush(&mut self) -> usize {
        self.with(|tx| tx.flush())
    }

    /// [Tx::flush] and copy the frame out, returns None if the frame is empty.
    pub fn flush_to_vec(&mut self) -> Option<Vec<u8>> {
        let len = self.flush();
        (len > 0).then(|| self.buf[..len].to_vec())
    }

    /// See [Tx::is_empty].
    pub fn is_empty(&self) -> bool {
        self.state.is_empty()
    }

    /// See [Tx::buf].
    pub fn buf(&self) -> &[u8] {
        &self.buf
    }

    /// Maximum frame size this framer was created with.
    pub fn frame_size(&self) -> usize {
        self.buf.len()
    }
}

/// Owned [FramedRx], see its docs for the intended use.
pub struct FramedRxOwned<H: Head, C, T> {
    buf: Vec<u8>,
    state: FramedRxState<H::UserKind>,
    _phantom: core::marker::PhantomData<(C, T)>,
}

impl<H: Head, C: Checksum, T: Tail> FramedRxOwned<H, C, T>
where
    H::UserKind: Copy + PartialEq,
{
    /// Create new framer with an assembly buffer of `assembly_size` bytes.
    /// Must hold at least one maximum re-assembled message + one maximum frame.
    pub fn new(assembly_size: usize) -> Self {
        let mut buf = vec![0u8; assembly_size];
        let state = FramedRx::<H, C, T>::new(&mut buf).into_parts();
        FramedRxOwned {
            buf,
            state,
            _phantom: core::marker::PhantomData,
        }
    }

    fn with<R>(&mut self, f: impl FnOnce(&mut FramedRx<'_, H, C, T>) -> R) -> R {
        let mut rx = FramedRx::<H, C, T>::from_parts(&mut self.buf, self.state);
        let r = f(&mut rx);
        self.state = rx.into_parts();
        r
    }

    /// See [FramedRx::free].
    pub fn free(&self) -> usize {
        self.buf.len() - self.state.staged_end()
    }

    /// See [FramedRx::stage].
    pub fn stage(&mut self, frame: &[u8]) -> Result<(), ()> {
        self.with(|rx| rx.stage(frame))
    }

    /// See [FramedRx::reassemble].
    pub fn reassemble(&mut self) {
        self.with(|rx| rx.reassemble())
    }

    /// See [FramedRx::message].
    pub fn message(&self) -> Option<(H::UserKind, &[u8])> {
        self.message_range()
            .map(|(user_kind, range)| (user_kind, &self.buf[range]))
    }

    /// See [FramedRx::message_range].
    pub fn message_range(&self) -> Option<(H::UserKind, Range<usize>)> {
        self.state.message_range()
    }
}
