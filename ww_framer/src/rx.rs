use core::marker::PhantomData;

use crate::traits::{Checksum, Head, Tail};

pub struct Rx<'i, H, C, T> {
    /// Holds up two one maximum Message size + one maximum input packet size (e.g., USB packet)
    ///
    /// For example if assembly_buf is 1024B, and maximum re-assembled message is 512, it could be
    /// that message is almost ready (e.g., 511B) and a whole new packet comes.
    /// Immediately a message is ready and must be returned with the approach used.
    /// But remaining bytes from the packet must be stored somewhere in the mean-time as well.
    assembly_buf: &'i mut [u8],

    /// Length of the message currently being assembled (not yet returned).
    /// Message can be assembled over many rx_packet calls (e.g., if packets are small and message is big).
    /// Held at assembly_buf[..assembling_len].
    /// 0 if no message is being assembled at the moment.
    assembling_len: usize,

    /// Length of the unprocessed data from the last packet.
    /// Held at assembly_buf[assembling_len .. assembling_len + staging_len].
    /// 0 if no remaining packet bytes are staged at the moment.
    staging_idx: usize,

    _phantom_h: PhantomData<H>,
    _phantom_c: PhantomData<C>,
    _phantom_t: PhantomData<T>,
}

enum State {
    /// If assembling_len != 0, then a message is ready
    Gap,
    Assembling,
}

impl<'b, 'i: 'b, H: Head, C: Checksum, T: Tail> Rx<'i, H, C, T>
where
    H::UserKind: Copy,
{
    /// Create new framer from the provided assembly buffer.
    /// Buffer must be exactly the length of the maximum frame (or DMA size).
    /// TODO: min buffer size
    pub fn new(assembly_buf: &'i mut [u8]) -> Self {
        debug_assert!(assembly_buf.len() >= 8);
        Rx {
            assembly_buf,
            assembling_len: 0,
            staging_idx: 0,
            _phantom_h: PhantomData,
            _phantom_c: PhantomData,
            _phantom_t: PhantomData,
        }
    }

    /// Returns the number of bytes that can be staged
    pub fn free(&self) -> usize {
        self.assembly_buf.len() - self.assembling_len
    }

    /// Call with a next received frame or chunk of bytes, then call [Self::reassemble] and
    /// [Self::message] in a loop until getting None.
    ///
    /// Can also be called multiple times before reassembling, if there is enough space ([Self::free]).
    pub fn stage(&mut self, frame: &[u8]) -> Result<(), ()> {
        if frame.len() + self.staging_idx <= self.assembly_buf.len() {
            self.assembly_buf[self.staging_idx..self.staging_idx + frame.len()]
                .copy_from_slice(frame);
        } else {
            return Err(());
        }
        Ok(())
    }

    pub fn reassemble(&mut self) {}

    /// Intented use:
    /// ```
    /// rx.stage(frame)?;
    /// loop {
    ///     rx.reassemble();
    ///     let Some((kind, message)) = self.message() else {
    ///         break;
    ///     }
    /// }
    /// ```
    pub fn message(&self) -> Option<(H::UserKind, &[u8])> {
        todo!()
    }
}
