/// Receive buffer for [FramedRx](crate::FramedRx) and [blocking::Server](crate::blocking::Server),
/// sized by the two numbers that actually matter: the largest packet and the largest message.
///
/// Reassembly needs one maximum message plus one maximum packet of space (a packet can arrive while
/// a message is almost complete), [assembly_buf](Self::assembly_buf) accounts for that, so that
/// exactly `MAX_MESSAGE_LEN` is reported to the host.
///
/// Two arrays instead of one of `MAX_MESSAGE_LEN + MAX_PACKET_LEN`, as the latter needs
/// `generic_const_exprs`.
#[repr(C)]
pub struct RxBuffer<const MAX_PACKET_LEN: usize, const MAX_MESSAGE_LEN: usize> {
    message: [u8; MAX_MESSAGE_LEN],
    packet: [u8; MAX_PACKET_LEN],
}

impl<const MAX_PACKET_LEN: usize, const MAX_MESSAGE_LEN: usize>
    RxBuffer<MAX_PACKET_LEN, MAX_MESSAGE_LEN>
{
    pub const fn new() -> Self {
        RxBuffer {
            message: [0u8; MAX_MESSAGE_LEN],
            packet: [0u8; MAX_PACKET_LEN],
        }
    }

    /// Assembly buffer for packets of up to `packet_len` bytes (e.g., the actual endpoint size, which
    /// can be smaller than `MAX_PACKET_LEN`), so that the maximum message is exactly `MAX_MESSAGE_LEN`.
    pub fn assembly_buf(&mut self, packet_len: usize) -> &mut [u8] {
        const {
            assert!(
                size_of::<Self>() == MAX_MESSAGE_LEN + MAX_PACKET_LEN,
                "RxBuffer must have no padding"
            )
        };
        let len = MAX_MESSAGE_LEN + packet_len.min(MAX_PACKET_LEN);
        // SAFETY: repr(C) struct of two u8 arrays: alignment 1 and no padding (checked above), so it is
        // MAX_MESSAGE_LEN + MAX_PACKET_LEN contiguous, initialized bytes, borrowed mutably through self.
        let all = unsafe {
            core::slice::from_raw_parts_mut(
                (self as *mut Self).cast::<u8>(),
                MAX_MESSAGE_LEN + MAX_PACKET_LEN,
            )
        };
        &mut all[..len]
    }
}

impl<const MAX_PACKET_LEN: usize, const MAX_MESSAGE_LEN: usize> Default
    for RxBuffer<MAX_PACKET_LEN, MAX_MESSAGE_LEN>
{
    fn default() -> Self {
        Self::new()
    }
}
