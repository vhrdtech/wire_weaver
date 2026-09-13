/// [U2Head](crate::framed::U2Head) bytes used across tests, see its docs for the layout.
///
/// Naming: `KIND_LEN[_UKx]`, user kind is 0 unless `_UKx` is present.
/// Continue / End carry the *remaining* length.
#[cfg(test)]
pub(crate) mod common {
    use shrink_wrap::{BufReader, BufWriter};

    use crate::traits::{Checksum, RdError, WrError};

    // `mmuu_0lll` - 1 byte, len ∈ 0..=7
    pub const FULL_0: u8 = 0b0000_0000;
    pub const FULL_1: u8 = 0b0000_0001;
    pub const FULL_4: u8 = 0b0000_0100;
    pub const FULL_4_UK1: u8 = 0b0001_0100;
    pub const START_4: u8 = 0b0100_0100;
    pub const START_5: u8 = 0b0100_0101;
    pub const START_6: u8 = 0b0100_0110;
    pub const START_7: u8 = 0b0100_0111;
    pub const CONT_2: u8 = 0b1000_0010;
    pub const CONT_4: u8 = 0b1000_0100;
    pub const CONT_4_UK1: u8 = 0b1001_0100;
    pub const CONT_5: u8 = 0b1000_0101;
    pub const CONT_6: u8 = 0b1000_0110;
    pub const END_1: u8 = 0b1100_0001;
    pub const END_2: u8 = 0b1100_0010;
    pub const END_4: u8 = 0b1100_0100;

    // `mmuu_10ll llll_llll` - 2 bytes, len < 1KiB
    pub const FULL_11: [u8; 2] = [0b0000_1000, 0b0000_1011];
    pub const START_9: [u8; 2] = [0b0100_1000, 0b0000_1001];
    pub const START_10: [u8; 2] = [0b0100_1000, 0b0000_1010];
    pub const START_11: [u8; 2] = [0b0100_1000, 0b0000_1011];
    pub const END_9: [u8; 2] = [0b1100_1000, 0b0000_1001];

    // `mmuu_110l llll_llll llll_llll` - 3 bytes, len < 128KiB
    pub const FULL_1024: [u8; 3] = [0b0000_1100, 0b0000_0100, 0b0000_0000];

    // `mmuu_1110 llll_llll llll_llll llll_llll` - 4 bytes, len < 16MiB
    pub const FULL_131072: [u8; 4] = [0b0000_1110, 0b0000_0010, 0b0000_0000, 0b0000_0000];

    // `mm11_uuuu uuuu_0lll` - 2 bytes, extended user kind
    pub const FULL_0_UK255: [u8; 2] = [0b0011_1111, 0b1111_0000];
    pub const START_3_UK255: [u8; 2] = [0b0111_1111, 0b1111_0011];
    pub const START_5_UK255: [u8; 2] = [0b0111_1111, 0b1111_0101];
    pub const END_1_UK255: [u8; 2] = [0b1111_1111, 0b1111_0001];
    pub const END_2_UK255: [u8; 2] = [0b1111_1111, 0b1111_0010];

    // `0b1111` length prefix is not a valid form
    pub const FULL_BAD_LEN: u8 = 0b0000_1111;
    pub const FULL_BAD_LEN_UK255: [u8; 2] = [0b0011_1111, 0b1111_1111];

    /// 1 byte XOR of all message bytes, for both Full and split messages.
    pub(crate) struct XorChecksum;

    impl Checksum for XorChecksum {
        const LEN_BYTES_FULL: usize = 1;
        const LEN_BYTES_SPLIT: usize = 1;

        fn write(message: &[u8], _is_split: bool, wr: &mut BufWriter<'_>) -> Result<(), WrError> {
            let xor = message.iter().fold(0u8, |acc, b| acc ^ b);
            wr.write_u8(xor)?;
            Ok(())
        }

        fn read(message: &[u8], _is_split: bool, rd: &mut BufReader<'_>) -> Result<(), RdError> {
            let xor = message.iter().fold(0u8, |acc, b| acc ^ b);
            if rd.read_u8()? == xor {
                Ok(())
            } else {
                Err(RdError::ChecksumMismatch)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use shrink_wrap::{BufReader, BufWriter};

    use super::common::*;
    use crate::Tx;
    use crate::framed::U2Head;
    use crate::traits::{Head, MessageKind, NopChecksum, NopTail, RdError, WrError};

    /// Test head with a fixed size: `FULL` bytes for Full/Start and `CONT` bytes for Continue/End.
    /// Byte 0 is the kind, byte 1 the user kind, byte 2 the length, rest is zero.
    /// Lengths above 255 are reported as too big.
    struct TestHead<const FULL: usize, const CONT: usize>;

    impl<const FULL: usize, const CONT: usize> Head for TestHead<FULL, CONT> {
        type UserKind = u8;
        const MIN_FRAME_SIZE: usize = 1;

        fn write(
            kind: MessageKind,
            user_kind: Self::UserKind,
            len: usize,
            wr: &mut BufWriter<'_>,
        ) -> Result<(), WrError> {
            if len > 255 {
                return Err(WrError::TooBig);
            }
            let n = match kind {
                MessageKind::Full | MessageKind::Start => FULL,
                MessageKind::Continue | MessageKind::End => CONT,
            };
            for i in 0..n {
                let b = match i {
                    0 => kind as u8,
                    1 => user_kind,
                    2 => len as u8,
                    _ => unreachable!(),
                };
                wr.write_u8(b)?;
            }
            Ok(())
        }

        fn read(_rd: &mut BufReader<'_>) -> Result<(MessageKind, Self::UserKind, usize), RdError> {
            unreachable!("TestHead is only used to test Tx")
        }
    }

    #[test]
    fn head_not_fitting_empty_frame_is_an_error() {
        // extended user kind head is 2 bytes, frame is 1 byte: can never be sent
        let mut buf = [0u8; 1];
        let mut tx = Tx::<U2Head, NopChecksum, NopTail>::new(&mut buf);
        assert_eq!(tx.write(255, &[]), Err(()));
        assert_eq!(tx.flush(), 0);
        // tx is still usable afterwards
        assert_eq!(tx.write(0, &[]), Ok(true));
        assert_eq!(tx.flush(), 1);
    }

    #[test]
    fn too_big_message_is_an_error() {
        let mut buf = [0u8; 8];
        let mut tx = Tx::<TestHead<1, 1>, NopChecksum, NopTail>::new(&mut buf);
        let big = [0u8; 256];

        // at frame start
        assert_eq!(tx.write(0, &big), Err(()));
        assert_eq!(tx.flush(), 0);

        // mid frame, already written data is preserved
        assert_eq!(tx.write(0, &[0xA1]), Ok(true));
        assert_eq!(tx.write(0, &big), Err(()));
        let len = tx.flush();
        assert_eq!(&tx.buf()[..len], &[MessageKind::Full as u8, 0xA1]);

        // tx is still usable afterwards
        assert_eq!(tx.write(0, &[0xA2]), Ok(true));
        let len = tx.flush();
        assert_eq!(&tx.buf()[..len], &[MessageKind::Full as u8, 0xA2]);
    }

    #[test]
    fn head_fitting_but_no_room_for_data_mid_frame_uses_next_frame() {
        let mut buf = [0u8; 4];
        let mut tx = Tx::<U2Head, NopChecksum, NopTail>::new(&mut buf);
        // fill 3 bytes of the frame with two empty messages (1 + 2 byte heads)
        assert_eq!(tx.write(0, &[]), Ok(true));
        assert_eq!(tx.write(255, &[]), Ok(true));
        // 1 byte head fits exactly, but no room for data: use next frame, not an error
        assert_eq!(tx.write(0, &[0xA1]), Ok(false));
        let len = tx.flush();
        assert_eq!(
            &tx.buf()[..len],
            &[FULL_0, FULL_0_UK255[0], FULL_0_UK255[1]]
        );
        assert_eq!(tx.write(0, &[0xA1]), Ok(true));
        let len = tx.flush();
        assert_eq!(&tx.buf()[..len], &[FULL_1, 0xA1]);
    }

    #[test]
    fn continue_head_not_fitting_empty_frame_is_an_error() {
        // Full/Start head is 1 byte, Continue/End head is 3 bytes: larger than the frame
        let mut buf = [0u8; 2];
        let mut tx = Tx::<TestHead<1, 3>, NopChecksum, NopTail>::new(&mut buf);
        let msg = [0xA1, 0xA2, 0xA3, 0xA4, 0xA5];

        assert_eq!(tx.write(0, &msg), Ok(false));
        let len = tx.flush();
        assert_eq!(&tx.buf()[..len], &[MessageKind::Start as u8, 0xA1]);

        // Continue head cannot fit even at frame start: message can never progress
        assert_eq!(tx.write(0, &msg), Err(()));
        assert_eq!(tx.flush(), 0);

        // state is reset to Gap, tx is still usable afterwards
        assert_eq!(tx.write(0, &[0xB1]), Ok(true));
        let len = tx.flush();
        assert_eq!(&tx.buf()[..len], &[MessageKind::Full as u8, 0xB1]);
    }

    #[test]
    fn write_without_flush_after_partial_message_uses_next_frame() {
        let mut buf = [0u8; 4];
        let mut tx = Tx::<U2Head, NopChecksum, NopTail>::new(&mut buf);
        let msg = [0xA1, 0xA2, 0xA3, 0xA4, 0xA5];

        // Start with 3 bytes of payload fills the frame completely
        assert_eq!(tx.write(0, &msg), Ok(false));
        // Continue head does not fit into a full frame: not an error, state is preserved
        assert_eq!(tx.write(0, &msg), Ok(false));
        let len = tx.flush();
        assert_eq!(&tx.buf()[..len], &[START_5, 0xA1, 0xA2, 0xA3]);

        // End with remaining = 2
        assert_eq!(tx.write(0, &msg), Ok(true));
        let len = tx.flush();
        assert_eq!(&tx.buf()[..len], &[END_2, 0xA4, 0xA5]);
    }

    #[test]
    fn continue_head_filling_whole_frame_is_an_error() {
        // Full/Start head is 1 byte, Continue/End head is 2 bytes: same as the frame
        let mut buf = [0u8; 2];
        let mut tx = Tx::<TestHead<1, 2>, NopChecksum, NopTail>::new(&mut buf);
        let msg = [0xA1, 0xA2, 0xA3];

        assert_eq!(tx.write(0, &msg), Ok(false));
        let len = tx.flush();
        assert_eq!(&tx.buf()[..len], &[MessageKind::Start as u8, 0xA1]);

        // Continue head fits, but leaves no room for data even at frame start
        assert_eq!(tx.write(0, &msg), Err(()));
        assert_eq!(tx.flush(), 0);

        // state is reset to Gap, tx is still usable afterwards
        assert_eq!(tx.write(0, &[0xB1]), Ok(true));
        let len = tx.flush();
        assert_eq!(&tx.buf()[..len], &[MessageKind::Full as u8, 0xB1]);
    }

    #[test]
    fn continue_head_fitting_but_no_room_for_data_mid_frame_uses_next_frame() {
        // Full/Start head is 1 byte, Continue/End head is empty
        let mut buf = [0u8; 2];
        let mut tx = Tx::<TestHead<1, 0>, NopChecksum, NopTail>::new(&mut buf);
        let msg = [0xA1, 0xA2, 0xA3];

        // Start with 1 byte of payload fills the frame completely
        assert_eq!(tx.write(0, &msg), Ok(false));
        // empty Continue head fits into a full frame, but no room for data: use next frame
        assert_eq!(tx.write(0, &msg), Ok(false));
        let len = tx.flush();
        assert_eq!(&tx.buf()[..len], &[MessageKind::Start as u8, 0xA1]);

        // remaining 2 bytes fit with an empty End head
        assert_eq!(tx.write(0, &msg), Ok(true));
        let len = tx.flush();
        assert_eq!(&tx.buf()[..len], &[0xA2, 0xA3]);
    }

    #[test]
    fn start_leaves_at_least_one_byte_for_end() {
        let mut buf = [0u8; 6];
        let mut tx = Tx::<U2Head, XorChecksum, NopTail>::new(&mut buf);
        let msg = [1, 2, 3, 4, 5];

        // all 5 payload bytes would fit, but not the checksum: keep 1 byte for End
        assert_eq!(tx.write(0, &msg), Ok(false));
        let len = tx.flush();
        assert_eq!(&tx.buf()[..len], &[START_5, 1, 2, 3, 4]);

        // End with remaining = 1 + checksum
        assert_eq!(tx.write(0, &msg), Ok(true));
        let len = tx.flush();
        assert_eq!(&tx.buf()[..len], &[END_1, 5, 1 ^ 2 ^ 3 ^ 4 ^ 5]);
    }

    #[test]
    fn end_not_fitting_with_checksum_stays_continue() {
        let mut buf = [0u8; 6];
        let mut tx = Tx::<U2Head, XorChecksum, NopTail>::new(&mut buf);
        let msg = [1, 2, 3, 4, 5, 6, 7, 8, 9];

        // 2 byte head (len = 9), 4 bytes of payload
        assert_eq!(tx.write(0, &msg), Ok(false));
        let len = tx.flush();
        assert_eq!(&tx.buf()[..len], &[START_9[0], START_9[1], 1, 2, 3, 4]);

        // remaining 5 bytes would fit, but not the checksum: Continue with 4, 1 byte unused
        assert_eq!(tx.write(0, &msg), Ok(false));
        let len = tx.flush();
        assert_eq!(&tx.buf()[..len], &[CONT_5, 5, 6, 7, 8]);

        // End with remaining = 1 + checksum
        assert_eq!(tx.write(0, &msg), Ok(true));
        let len = tx.flush();
        let xor = msg.iter().fold(0, |acc, b| acc ^ b);
        assert_eq!(&tx.buf()[..len], &[END_1, 9, xor]);
    }

    #[test]
    fn head_filling_whole_frame_is_an_error_not_empty_frame() {
        // extended user kind + 17-bit length = 4 byte head, same as the frame
        let mut buf = [0u8; 4];
        let mut tx = Tx::<U2Head, NopChecksum, NopTail>::new(&mut buf);
        let msg = [0u8; 1024];
        assert_eq!(tx.write(255, &msg), Err(()));
        assert_eq!(tx.flush(), 0);
        // tx is still usable afterwards
        assert_eq!(tx.write(0, &[]), Ok(true));
        assert_eq!(tx.flush(), 1);
    }

    #[test]
    fn head_not_fitting_mid_frame_uses_next_frame() {
        let mut buf = [0u8; 4];
        let mut tx = Tx::<U2Head, NopChecksum, NopTail>::new(&mut buf);
        // fill 3 bytes of the frame with two empty messages (1 + 2 byte heads)
        assert_eq!(tx.write(0, &[]), Ok(true));
        assert_eq!(tx.write(255, &[]), Ok(true));
        // extended user kind head is 2 bytes, only 1 left: use next frame, not an error
        assert_eq!(tx.write(255, &[3]), Ok(false));
        assert_eq!(tx.flush(), 3);
        assert_eq!(tx.write(255, &[3]), Ok(true));
        assert_eq!(tx.flush(), 3);
    }
}
