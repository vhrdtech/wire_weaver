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

    /// 1 byte XOR of all message bytes, for both Full and split messages. Leaves the header out, so that the bytes
    /// in tests stay easy to write.
    pub(crate) struct XorChecksum;

    impl Checksum for XorChecksum {
        const LEN_BYTES_FULL: usize = 1;
        const LEN_BYTES_SPLIT: usize = 1;

        fn write(
            _header: &[u8],
            message: &[u8],
            _is_split: bool,
            wr: &mut BufWriter<'_>,
        ) -> Result<(), WrError> {
            let xor = message.iter().fold(0u8, |acc, b| acc ^ b);
            wr.write_u8(xor)?;
            Ok(())
        }

        fn read(
            _header: &[u8],
            message: &[u8],
            _is_split: bool,
            rd: &mut BufReader<'_>,
        ) -> Result<(), RdError> {
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
mod tx_edge_cases {
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
        const MAX_HEAD_SIZE: usize = if FULL > CONT { FULL } else { CONT };

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
        assert_eq!(&tx.buf()[..len], &[END_1, 5, 4 ^ 5]);
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

/// Frames with a [SeqPrefix](crate::prefix::SeqPrefix): layout, and what [FramedRx] does with lost, duplicated,
/// reordered and stale frames.
#[cfg(test)]
mod prefixed {
    extern crate std;
    use std::vec::Vec;

    use super::common::*;
    use crate::crc::{Crc16IbmSdlc, CrcChecksum};
    use crate::framed::U2Head;
    use crate::pieces::{PieceErrorKind, pieces_with_prefix, verify_split};
    use crate::prefix::{FrameCtx, FrameReport, SeqPrefix};
    use crate::traits::{MessageKind, NopChecksum, NopTail};
    use crate::{FramedRx, Tx};

    type STx<'a> = Tx<'a, U2Head, NopChecksum, NopTail, SeqPrefix>;
    type SRx<'a> = FramedRx<'a, U2Head, NopChecksum, NopTail, SeqPrefix>;
    type Crc = CrcChecksum<Crc16IbmSdlc, true>;
    type CTx<'a> = Tx<'a, U2Head, Crc, NopTail, SeqPrefix>;
    type CRx<'a> = FramedRx<'a, U2Head, Crc, NopTail, SeqPrefix>;

    /// Frames of `messages` written by `tx`, flushed at the end.
    fn frames(tx: &mut STx<'_>, messages: &[(u8, &[u8])]) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        for (uk, m) in messages {
            while !tx.write(*uk, m).unwrap() {
                let n = tx.flush();
                out.push(tx.buf()[..n].to_vec());
            }
        }
        let n = tx.flush();
        if n > 0 {
            out.push(tx.buf()[..n].to_vec());
        }
        out
    }

    /// Stage a frame and collect what comes out of it.
    fn feed(rx: &mut SRx<'_>, frame: &[u8]) -> (FrameReport, Vec<(u8, Vec<u8>)>) {
        let report = rx.stage(frame).unwrap();
        let mut got = Vec::new();
        loop {
            rx.reassemble();
            match rx.message() {
                Some((uk, m)) => got.push((uk, m.to_vec())),
                None => break,
            }
        }
        (report, got)
    }

    #[test]
    fn every_frame_starts_with_the_prefix() {
        let mut buf = [0u8; 6];
        let mut tx = STx::new(&mut buf);
        let msg = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let f = frames(&mut tx, &[(0, &[0xAA]), (0, &msg)]);
        // prefix, Full 1, then Start 10 with 2 bytes; prefix 1, Continue 8 with 4; prefix 2, End 4
        assert_eq!(
            f,
            [
                std::vec![0x00, FULL_1, 0xAA, START_10[0], START_10[1], 1],
                std::vec![0x01, 0b1000_1000, 9, 2, 3, 4],
                std::vec![0x02, 0b1000_0110, 5, 6, 7, 8],
                std::vec![0x03, 0b1100_0010, 9, 10],
            ]
        );
        assert_eq!(tx.frame_ctx(), FrameCtx { seq: 4, epoch: 0 });
        // an empty flush doesn't count
        assert_eq!(tx.flush(), 0);
        assert_eq!(tx.frame_ctx().seq, 4);
        // a new session: epoch 1, counted from 0, the half-written message is gone
        assert_eq!(tx.write(0, &msg), Ok(false));
        tx.new_session();
        assert_eq!(tx.write(0, &[7]), Ok(true));
        let n = tx.flush();
        assert_eq!(&tx.buf()[..n], &[0b0001_0000, FULL_1, 7]);
    }

    #[test]
    fn frame_too_small_for_prefix_and_head() {
        // prefix + 1 byte head + 1 byte: smallest frame
        let mut buf = [0u8; 2];
        let mut tx = STx::new(&mut buf);
        assert_eq!(tx.write(0, &[]), Ok(true));
        assert_eq!(tx.write(0, &[]), Ok(false));
        assert_eq!(tx.flush(), 2);
        // extended user kind head doesn't fit after the prefix: never
        assert_eq!(tx.write(255, &[]), Err(()));
        assert_eq!(tx.flush(), 0);
        assert_eq!(tx.write_full(255, &[]), Err(()));
        assert_eq!(tx.flush(), 0);
        assert_eq!(tx.frame_ctx().seq, 1);
    }

    #[test]
    fn round_trip_in_order() {
        let mut tx_buf = [0u8; 8];
        let mut tx = STx::new(&mut tx_buf);
        let messages: Vec<(u8, Vec<u8>)> = (0..40)
            .map(|len| ((len % 3) as u8, (0..len).map(|i| i as u8 ^ 0x5A).collect()))
            .collect();
        let refs: Vec<(u8, &[u8])> = messages.iter().map(|(k, m)| (*k, &m[..])).collect();
        let f = frames(&mut tx, &refs);
        assert!(f.len() > 16, "counter wraps");
        let mut rx_buf = [0u8; 64];
        let mut rx = SRx::new(&mut rx_buf);
        let mut got = Vec::new();
        for (i, frame) in f.iter().enumerate() {
            let (report, m) = feed(&mut rx, frame);
            if i == 0 {
                assert!(matches!(report, FrameReport::Synced(_)));
            } else {
                assert_eq!(report, FrameReport::InOrder, "frame {i}");
            }
            got.extend(m);
        }
        assert_eq!(got, messages);
        let stats = rx.prefix_stats();
        assert_eq!((stats.frames as usize, stats.lost), (f.len(), 0));
    }

    #[test]
    fn duplicate_frames_are_dropped() {
        let mut tx_buf = [0u8; 8];
        let mut tx = STx::new(&mut tx_buf);
        // Start 10 with 5 bytes, End 5, then a Full in the third frame
        let f = frames(&mut tx, &[(0, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]), (1, &[9])]);
        let mut rx_buf = [0u8; 64];
        let mut rx = SRx::new(&mut rx_buf);
        assert_eq!(feed(&mut rx, &f[0]).1, []);
        // the first frame again: would have looked like a Start of another message
        assert_eq!(feed(&mut rx, &f[0]), (FrameReport::Duplicate, std::vec![]));
        let (report, got) = feed(&mut rx, &f[1]);
        assert_eq!(report, FrameReport::InOrder);
        assert_eq!(got, [(0, std::vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10])]);
        assert_eq!(feed(&mut rx, &f[1]), (FrameReport::Duplicate, std::vec![]));
        assert_eq!(feed(&mut rx, &f[2]).1, [(1, std::vec![9])]);
        assert_eq!(rx.prefix_stats().duplicate, 2);
    }

    #[test]
    fn gap_drops_the_message_being_assembled_and_decodes_the_frame() {
        let mut tx_buf = [0u8; 8];
        let mut tx = STx::new(&mut tx_buf);
        // a: frames 0..=2 (5 + 5 + 4 bytes), a Full after its End in frame 2, another Full in frame 3
        let a: Vec<u8> = (1..=14).collect();
        let f = frames(&mut tx, &[(0, &a), (1, &[7]), (2, &[8; 5])]);
        assert_eq!(f.len(), 4);
        let mut rx_buf = [0u8; 64];
        let mut rx = SRx::new(&mut rx_buf);
        feed(&mut rx, &f[0]);
        // frame 1 lost: frame 2 has End of a (dropped) and the Full after it
        let (report, got) = feed(&mut rx, &f[2]);
        assert_eq!(report, FrameReport::Gap { lost: 1 });
        assert_eq!(got, [(1, std::vec![7])]);
        // frame 1 arrives late: dropped, it can't be put back
        assert_eq!(feed(&mut rx, &f[1]), (FrameReport::Late, std::vec![]));
        let (report, got) = feed(&mut rx, &f[3]);
        assert_eq!(report, FrameReport::InOrder);
        assert_eq!(got, [(2, std::vec![8; 5])]);
        assert_eq!(rx.free(), 64);
    }

    #[test]
    fn gap_between_continue_frames_with_equal_lengths() {
        // without a prefix, two Continues of the same length swapped look right to the remaining-length check; the
        // counter tells
        let mut tx_buf = [0u8; 8];
        let mut tx = STx::new(&mut tx_buf);
        let a: Vec<u8> = (1..=25).collect();
        let f = frames(&mut tx, &[(0, &a)]);
        let mut rx_buf = [0u8; 64];
        let mut rx = SRx::new(&mut rx_buf);
        feed(&mut rx, &f[0]);
        let (report, _) = feed(&mut rx, &f[2]);
        assert_eq!(report, FrameReport::Gap { lost: 1 });
        assert_eq!(feed(&mut rx, &f[1]).0, FrameReport::Late);
        for frame in &f[3..] {
            assert_eq!(feed(&mut rx, frame).1, []);
        }
        // the link goes on
        let f = frames(&mut tx, &[(0, &[1])]);
        assert_eq!(feed(&mut rx, &f[0]).1, [(0, std::vec![1])]);
    }

    #[test]
    fn stale_session_is_dropped_new_session_taken() {
        let mut tx_buf = [0u8; 8];
        let mut tx = STx::new(&mut tx_buf);
        let old_start = frames(&mut tx, &[(0, &[1; 10])]);
        tx.new_session();
        let new = frames(&mut tx, &[(1, &[2])]);
        let mut rx_buf = [0u8; 64];
        let mut rx = SRx::new(&mut rx_buf);
        feed(&mut rx, &old_start[0]);
        // a half-sent message of the old session is dropped by the new one
        let (report, got) = feed(&mut rx, &new[0]);
        assert_eq!(report, FrameReport::NewEpoch(FrameCtx { seq: 0, epoch: 1 }));
        assert_eq!(got, [(1, std::vec![2])]);
        // the rest of the old session arriving late
        assert_eq!(
            feed(&mut rx, &old_start[1]),
            (FrameReport::StaleEpoch, std::vec![])
        );
        assert_eq!(rx.free(), 64);
    }

    #[test]
    fn too_short_frames() {
        let mut rx_buf = [0u8; 16];
        let mut rx = SRx::new(&mut rx_buf);
        assert_eq!(rx.stage(&[]), Ok(FrameReport::TooShort));
        assert_eq!(rx.commit_staged(0), Ok(FrameReport::TooShort));
        assert_eq!(rx.prefix_stats().too_short, 2);
        // only the prefix: nothing in it
        assert!(matches!(rx.stage(&[0x00]), Ok(FrameReport::Synced(_))));
        rx.reassemble();
        assert_eq!(rx.message(), None);
    }

    #[test]
    fn commit_staged_skips_the_prefix() {
        let mut tx_buf = [0u8; 8];
        let mut tx = STx::new(&mut tx_buf);
        let f = frames(&mut tx, &[(0, &[1, 2, 3, 4, 5, 6, 7, 8]), (2, &[3])]);
        let mut rx_buf = [0u8; 32];
        let mut rx = SRx::new(&mut rx_buf);
        let mut got = Vec::new();
        for frame in &f {
            rx.staging_buf()[..frame.len()].copy_from_slice(frame);
            rx.commit_staged(frame.len()).unwrap();
            loop {
                rx.reassemble();
                match rx.message() {
                    Some((uk, m)) => got.push((uk, m.to_vec())),
                    None => break,
                }
            }
        }
        assert_eq!(
            got,
            [(0, std::vec![1, 2, 3, 4, 5, 6, 7, 8]), (2, std::vec![3])]
        );
        // a duplicate is not committed
        rx.staging_buf()[..f[1].len()].copy_from_slice(&f[1]);
        assert_eq!(rx.commit_staged(f[1].len()), Ok(FrameReport::Duplicate));
        assert_eq!(rx.free(), 32);
    }

    #[test]
    fn commit_staged_behind_unprocessed_bytes() {
        // a frame committed while bytes of the previous one still wait (not the intended use with a prefix, but
        // nothing breaks): the prefix is cut out
        let mut rx_buf = [0u8; 32];
        let mut rx = SRx::new(&mut rx_buf);
        rx.stage(&[0x00, FULL_1, 0xAA]).unwrap();
        let frame = [0x01, FULL_1, 0xBB];
        rx.staging_buf()[..3].copy_from_slice(&frame);
        assert_eq!(rx.commit_staged(3), Ok(FrameReport::InOrder));
        rx.reassemble();
        assert_eq!(rx.message(), Some((0, &[0xAA][..])));
        rx.reassemble();
        assert_eq!(rx.message(), Some((0, &[0xBB][..])));
        rx.reassemble();
        assert_eq!(rx.message(), None);
    }

    #[test]
    fn checksum_covers_the_prefix() {
        let mut tx_buf = [0u8; 8];
        let mut tx = CTx::new(&mut tx_buf);
        assert_eq!(tx.write(0, &[1, 2]), Ok(true));
        let n = tx.flush();
        let frame = tx.buf()[..n].to_vec();
        // over prefix, head and message
        let crc = crc::Crc::<u16>::new(&crc::CRC_16_IBM_SDLC).checksum(&[0x00, 0b0000_0010, 1, 2]);
        let [c0, c1] = crc.to_le_bytes();
        assert_eq!(frame, [0x00, 0b0000_0010, 1, 2, c0, c1]);

        let mut rx_buf = [0u8; 32];
        let mut rx = CRx::new(&mut rx_buf);
        // a flipped bit in the prefix (still a plausible one) fails the message's CRC
        let mut bad = frame.clone();
        bad[0] = 0x01;
        assert!(matches!(rx.stage(&bad), Ok(FrameReport::Synced(_))));
        rx.reassemble();
        assert_eq!(rx.message(), None);
        // and so does one in the head's user kind
        let mut bad = frame.clone();
        bad[1] = 0b0001_0010;
        rx.resync();
        rx.stage(&bad).unwrap();
        rx.reassemble();
        assert_eq!(rx.message(), None);
        rx.resync();
        rx.stage(&frame).unwrap();
        rx.reassemble();
        assert_eq!(rx.message(), Some((0, &[1, 2][..])));

        // split: the CRC is over the End frame's prefix and the whole message
        let msg: Vec<u8> = (0..10).collect();
        let mut f = Vec::new();
        while !tx.write(0, &msg).unwrap() {
            let n = tx.flush();
            f.push(tx.buf()[..n].to_vec());
        }
        let n = tx.flush();
        f.push(tx.buf()[..n].to_vec());
        for frame in &f {
            rx.stage(frame).unwrap();
            rx.reassemble();
        }
        assert_eq!(rx.message(), Some((0, &msg[..])));

        // the same through pieces
        let last = f.last().unwrap();
        let mut walk = pieces_with_prefix::<U2Head, Crc, NopTail, SeqPrefix>(last);
        let end = walk.next().unwrap().unwrap();
        assert_eq!(end.kind, MessageKind::End);
        assert_eq!(walk.prefix(), &last[..1]);
        let head = &last[end.head.clone()];
        assert!(verify_split::<Crc>(
            &last[..1],
            head,
            &msg,
            &last[end.trailer.clone()]
        ));
        assert!(!verify_split::<Crc>(
            &[],
            head,
            &msg,
            &last[end.trailer.clone()]
        ));
        assert!(!verify_split::<Crc>(
            &last[..1],
            &[],
            &msg,
            &last[end.trailer.clone()]
        ));
        let full = pieces_with_prefix::<U2Head, Crc, NopTail, SeqPrefix>(&frame)
            .next()
            .unwrap()
            .unwrap();
        assert_eq!((full.payload.clone(), full.checksum_ok), (2..4, Some(true)));
        let too_short: Vec<_> =
            pieces_with_prefix::<U2Head, Crc, NopTail, SeqPrefix>(&[]).collect();
        assert_eq!(too_short.len(), 1);
        assert_eq!(
            too_short[0].as_ref().unwrap_err().kind,
            PieceErrorKind::TooShort
        );
    }
}
