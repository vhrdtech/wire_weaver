//! Types aimed at framed media (USB, CAN, etc.)

use shrink_wrap::{BufReader, BufWriter, Nibble};

use crate::traits::{Head, MessageKind, RdError, WrError};

/// Head that encodes message kind, 2 or 8 bits of user kind and length up to 24 bits (16MiB)
/// in as little as 1 byte.
///
/// Most common cases (data messages):
/// - `mmuu_0lll` - 1 byte; length ∈ 0..=7 - `common case`
/// - `mmuu_10ll llll_llll` - 2 bytes; length < 1KiB - `common case`
/// - `mmuu_110l llll_llll llll_llll` - 3 bytes; length < 128KiB (feature "large")
/// - `mmuu_1110 llll_llll llll_llll llll_llll` - 4 bytes; length < 16MiB (feature "very_large")
///
/// Less common cases (link setup, diagnostics, etc.):
/// - `mm11_uuuu uuuu_0lll` - 2 bytes, aligned to next byte later on, for each case below
/// - `mm11_uuuu uuuu_10ll llll_llll` - 3 bytes
/// - `mm11_uuuu uuuu_110l llll_llll llll_llll` - 4 bytes
/// - `mm11_uuuu uuuu_1110 llll_llll llll_llll llll_llll` - 5 bytes
///
/// Where `u` - user_kind, `l` - length, `m` - framer bits ([MessageKind](crate::traits::MessageKind))
/// For all lengths that does not fit into the smallest form, next one is used.
///
/// - user_kind = {0, 1, 2} => 2 bits are used as is (most common data messages, up to 3 sub-channels)
/// - user_kind >= 3 => 0b11 followed by 4 bits (link setup and diagnostics, rare)
pub struct U2Head {}

impl Head for U2Head {
    type UserKind = u8;
    #[cfg(all(not(feature = "large"), not(feature = "very_large")))]
    const MIN_FRAME_SIZE: usize = 4;
    #[cfg(all(feature = "large", not(feature = "very_large")))]
    const MIN_FRAME_SIZE: usize = 5;
    #[cfg(all(feature = "large", feature = "very_large"))]
    const MIN_FRAME_SIZE: usize = 6;

    fn write(
        kind: MessageKind,
        user_kind: Self::UserKind,
        len: usize,
        wr: &mut BufWriter<'_>,
    ) -> Result<(), WrError> {
        wr.write_un8(2, kind as u8)?;
        if user_kind <= 2 {
            wr.write_un8(2, user_kind)?;
        } else {
            wr.write_un8(2, 0b11)?;
            wr.write_un8(8, user_kind)?;
        }
        if len <= 7 {
            wr.write_nib(unsafe { Nibble::new(len as u8).unwrap_unchecked() })?;
            return Ok(());
        }
        if len < 1024 {
            wr.write_un8(2, 0b10)?;
            wr.write_un16(10, len as u16)?;
            return Ok(());
        }
        #[cfg(any(feature = "large", test))]
        if len < 131_072 {
            wr.write_un8(3, 0b110)?;
            wr.write_un32(17, len as u32)?;
            return Ok(());
        }
        #[cfg(any(feature = "very_large", test))]
        if len < 16_777_216 {
            wr.write_nib(unsafe { Nibble::new(0b1110).unwrap_unchecked() })?;
            wr.write_un32(24, len as u32)?;
            return Ok(());
        }
        Err(WrError::TooBig)
    }

    fn read(rd: &mut BufReader<'_>) -> Result<(MessageKind, Self::UserKind, usize), RdError> {
        let kind = rd.read_un8(2)?;
        let kind = match kind {
            0 => MessageKind::Full,
            1 => MessageKind::Start,
            2 => MessageKind::Continue,
            _ => MessageKind::End,
        };
        let user_kind = rd.read_un8(2)?;
        let user_kind = if user_kind <= 2 {
            user_kind
        } else {
            rd.read_un8(8)?
        };
        let three_bit_len = !rd.read_bool()?;
        if three_bit_len {
            let len = rd.read_un8(3)?;
            return Ok((kind, user_kind, len as usize));
        }
        let ten_bit_len = !rd.read_bool()?;
        if ten_bit_len {
            let len = rd.read_un16(10)?;
            return Ok((kind, user_kind, len as usize));
        }
        #[cfg(any(feature = "large", test))]
        let seventeen_bit_len = !rd.read_bool()?;
        #[cfg(any(feature = "large", test))]
        if seventeen_bit_len {
            let len = rd.read_un32(17)?;
            return Ok((kind, user_kind, len as usize));
        }
        #[cfg(any(feature = "very_large", test))]
        let twenty_four_bit_len = !rd.read_bool()?;
        #[cfg(any(feature = "very_large", test))]
        if twenty_four_bit_len {
            let len = rd.read_un32(24)?;
            return Ok((kind, user_kind, len as usize));
        }
        Err(RdError::BadLength)
    }
}

#[cfg(test)]
mod tests {
    use shrink_wrap::{BufReader, BufWriter};

    use crate::{
        Tx,
        framed::U2Head,
        tests::common::*,
        traits::{Head, MessageKind, NopChecksum, NopTail, RdError, WrError},
    };

    #[test]
    fn sanity() {
        let mut buf = [0u8; 6];
        let mut tx = Tx::<U2Head, NopChecksum, NopTail>::new(&mut buf);

        assert_eq!(tx.write(0, &[]), Ok(true));
        let len = tx.flush();
        assert_eq!(&tx.buf()[..len], &[FULL_0]);

        assert_eq!(tx.write(255, &[]), Ok(true));
        let len = tx.flush();
        assert_eq!(&tx.buf()[..len], &FULL_0_UK255);

        let msg4 = &[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF][..];
        assert_eq!(tx.write(0, msg4), Ok(false));
        let len = tx.flush();
        assert_eq!(&tx.buf()[..len], &[START_6, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE]);
        assert_eq!(tx.write(0, msg4), Ok(true));
        let len = tx.flush();
        // End carries remaining length (1), which now fits the smallest 3-bit form
        assert_eq!(&tx.buf()[..len], &[END_1, 0xFF]);
    }

    #[test]
    fn split_message_with_extended_user_kind() {
        let mut buf = [0u8; 6];
        let mut tx = Tx::<U2Head, NopChecksum, NopTail>::new(&mut buf);
        let msg3 = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE];

        // lengths 0..=7 all fit the 2-byte extended-user-kind head, so 2 payload bytes fit here
        assert_eq!(tx.write(255, &msg3), Ok(false));
        let len = tx.flush();
        assert_eq!(
            &tx.buf()[..len],
            &[START_5_UK255[0], START_5_UK255[1], 0xAA, 0xBB, 0xCC, 0xDD]
        );

        // remaining = 1, still fits the 2-byte head
        assert_eq!(tx.write(255, &msg3), Ok(true));
        let len = tx.flush();
        assert_eq!(&tx.buf()[..len], &[END_1_UK255[0], END_1_UK255[1], 0xEE]);
    }

    #[test]
    fn split_message_with_continue() {
        let mut buf = [0u8; 6];
        let mut tx = Tx::<U2Head, NopChecksum, NopTail>::new(&mut buf);
        let msg7 = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0xAB, 0xAC, 0xAD, 0xAE];

        assert_eq!(tx.write(0, &msg7), Ok(false));
        let len = tx.flush();
        assert_eq!(
            &tx.buf()[..len],
            &[START_10[0], START_10[1], 0xAA, 0xBB, 0xCC, 0xDD]
        );

        assert_eq!(tx.write(0, &msg7), Ok(false));
        let len = tx.flush();
        // Continue with remaining = 6
        assert_eq!(&tx.buf()[..len], &[CONT_6, 0xEE, 0xFF, 0xAB, 0xAC, 0xAD]);

        assert_eq!(tx.write(0, &msg7), Ok(true));
        let len = tx.flush();
        // End with remaining = 1
        assert_eq!(&tx.buf()[..len], &[END_1, 0xAE]);
    }

    #[test]
    fn round_trip() {
        let cases = [
            (MessageKind::Full, 0, 0usize),
            (MessageKind::Full, 3, 0),
            (MessageKind::Start, 0, 4),
            (MessageKind::Continue, 2, 7),
            (MessageKind::End, 1, 10),
            (MessageKind::Full, 255, 4),
            (MessageKind::Start, 255, 11),
            (MessageKind::Continue, 255, 1023),
        ];

        for (kind, user_kind, len) in cases {
            let mut buf = [0u8; 5];
            let mut wr = BufWriter::new(&mut buf[..]);
            U2Head::write(kind, user_kind, len, &mut wr).unwrap();
            let raw = wr.finish().unwrap();

            let mut rd = BufReader::new(raw);
            let (rt_kind, rt_user_kind, rt_len) = U2Head::read(&mut rd).unwrap();

            assert_eq!(rt_kind, kind);
            assert_eq!(rt_user_kind, user_kind);
            assert_eq!(rt_len, len);
        }
    }

    #[test]
    fn round_trip_large_lengths() {
        // 17-bit (feature "large") and 24-bit (feature "very_large") forms
        let cases = [
            (MessageKind::Full, 0, 1024usize),
            (MessageKind::Start, 2, 131_071),
            (MessageKind::Continue, 255, 65_536),
            (MessageKind::End, 0, 131_072),
            (MessageKind::Full, 1, 1_000_000),
            (MessageKind::Start, 255, 16_777_215),
        ];

        for (kind, user_kind, len) in cases {
            let mut buf = [0u8; 8];
            let mut wr = BufWriter::new(&mut buf[..]);
            U2Head::write(kind, user_kind, len, &mut wr).unwrap();
            let raw = wr.finish().unwrap();

            let mut rd = BufReader::new(raw);
            let (rt_kind, rt_user_kind, rt_len) = U2Head::read(&mut rd).unwrap();

            assert_eq!(rt_kind, kind);
            assert_eq!(rt_user_kind, user_kind);
            assert_eq!(rt_len, len);
        }
    }

    #[test]
    fn write_seventeen_bit_len_encoding() {
        let mut buf = [0u8; 8];
        let mut wr = BufWriter::new(&mut buf[..]);
        // Full, user_kind 0, len 1024 = 0b1_0000_0000_0000_0000 => `mmuu_110l llll_llll llll_llll`
        U2Head::write(MessageKind::Full, 0, 1024, &mut wr).unwrap();
        let raw = wr.finish().unwrap();
        assert_eq!(raw, &FULL_1024);
    }

    #[test]
    fn write_twenty_four_bit_len_encoding() {
        let mut buf = [0u8; 8];
        let mut wr = BufWriter::new(&mut buf[..]);
        // Full, user_kind 0, len 131_072 = 0x02_0000 => `mmuu_1110 llll_llll llll_llll llll_llll`
        U2Head::write(MessageKind::Full, 0, 131_072, &mut wr).unwrap();
        let raw = wr.finish().unwrap();
        assert_eq!(raw, &FULL_131072);
    }

    #[test]
    fn write_too_big() {
        let mut buf = [0u8; 8];
        let mut wr = BufWriter::new(&mut buf[..]);
        assert_eq!(
            U2Head::write(MessageKind::Full, 0, 16_777_216, &mut wr),
            Err(WrError::TooBig)
        );

        let mut wr = BufWriter::new(&mut buf[..]);
        assert_eq!(
            U2Head::write(MessageKind::End, 255, usize::MAX, &mut wr),
            Err(WrError::TooBig)
        );
    }

    #[test]
    fn read_bad_length() {
        // Full, user_kind 0, then 0b1111 length prefix => no valid length form
        let raw = [FULL_BAD_LEN, 0, 0, 0, 0];
        let mut rd = BufReader::new(&raw);
        assert_eq!(U2Head::read(&mut rd), Err(RdError::BadLength));

        // Extended user kind followed by 0b1111 length prefix
        let raw = [FULL_BAD_LEN_UK255[0], FULL_BAD_LEN_UK255[1], 0, 0, 0];
        let mut rd = BufReader::new(&raw);
        assert_eq!(U2Head::read(&mut rd), Err(RdError::BadLength));
    }
}
