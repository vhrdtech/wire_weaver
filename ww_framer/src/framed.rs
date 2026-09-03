//! Types aimed at framed media (USB, CAN, etc.)

use shrink_wrap::{BufReader, BufWriter, Nibble};

use crate::traits::{Head, MessageKind, RdError, WrError};

/// Head that encodes message kind, 2 or 4 bits of user kind and length up to 24 bits (16MiB)
/// in as little as 1 byte.
///
/// Most common cases (data messages):
/// - `mmuu_0lll` - 1 byte; length ∈ {0, 4, 5, 6, 7, 8, 9, 10} - `common case`
/// - `mmuu_10ll llll_llll` - 2 bytes; length < 1KiB - `common case`
/// - `mmuu_110l llll_llll llll_llll` - 3 bytes; length < 128KiB
/// - `mmuu_1110 llll_llll llll_llll llll_llll` - 4 bytes; length < 16MiB
///
/// Less common cases (link setup, diagnostics, etc.):
/// - `mm11_uuuu 0lll` - 1.5 bytes, aligned to next byte later on, for each case below
/// - `mm11_uuuu 10ll_llll llll` - 2.5 bytes
/// - `mm11_uuuu 110l_llll llll_llll llll` - 3.5 bytes
/// - `mm11_uuuu 1110_llll llll_llll llll_llll llll` - 4.5 bytes
///
/// Where `u` - user_kind, `l` - length, `m` - framer bits ([MessageKind](crate::traits::MessageKind))
/// For all lengths that does not fit into the smallest form, next one is used.
/// This optimizes for the fact, that very small messages are not used at all, or very rarely.
///
/// - user_kind = {0, 1, 2} => 2 bits are used as is (most common data messages, up to 3 sub-channels)
/// - user_kind >= 3 => 0b11 followed by 4 bits (link setup and diagnostics, rare)
pub struct U2Head {}

impl Head for U2Head {
    type UserKind = Nibble;

    fn write(
        kind: MessageKind,
        user_kind: Self::UserKind,
        len: usize,
        wr: &mut BufWriter<'_>,
    ) -> Result<(), WrError> {
        wr.write_un8(2, kind as u8)?;
        if user_kind.value() <= 2 {
            wr.write_un8(2, user_kind.value())?;
        } else {
            wr.write_un8(2, 0b11)?;
            wr.write_nib(user_kind)?;
        }
        if len == 0 {
            wr.write_nib(Nibble::zero())?;
            return Ok(());
        }
        if len >= 4 && len <= 10 {
            wr.write_nib(unsafe { Nibble::new((len - 3) as u8).unwrap_unchecked() })?;
            return Ok(());
        }
        if len < 1024 {
            wr.write_un8(2, 0b10)?;
            wr.write_un16(10, len as u16)?;
            return Ok(());
        }
        #[cfg(feature = "large")]
        if len < 131_072 {
            wr.write_un8(3, 0b110)?;
            wr.write_un32(17, len as u32)?;
            return Ok(());
        }
        #[cfg(feature = "very_large")]
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
            unsafe { Nibble::new(user_kind).unwrap_unchecked() }
        } else {
            rd.read_nib()?
        };
        let three_bit_len = !rd.read_bool()?;
        if three_bit_len {
            let len = rd.read_un8(3)?;
            if len == 0 {
                return Ok((kind, user_kind, 0));
            } else {
                return Ok((kind, user_kind, len as usize + 3));
            }
        }
        let ten_bit_len = !rd.read_bool()?;
        if ten_bit_len {
            let len = rd.read_un16(10)?;
            return Ok((kind, user_kind, len as usize));
        }
        #[cfg(feature = "large")]
        let seventeen_bit_len = !rd.read_bool()?;
        #[cfg(feature = "large")]
        if seventeen_bit_len {
            let len = rd.read_un32(17)?;
            return Ok((kind, user_kind, len as usize));
        }
        #[cfg(feature = "very_large")]
        let twenty_four_bit_len = !rd.read_bool()?;
        #[cfg(feature = "very_large")]
        if twenty_four_bit_len {
            let len = rd.read_un32(24)?;
            return Ok((kind, user_kind, len as usize));
        }
        Err(RdError::BadLength)
    }
}

#[cfg(test)]
mod tests {
    use shrink_wrap::{BufReader, BufWriter, Nibble};

    use crate::{
        Tx,
        framed::U2Head,
        traits::{Head, MessageKind, NopChecksum, NopTail},
    };

    #[test]
    fn sanity() {
        let mut buf = [0u8; 4];
        let mut tx = Tx::<U2Head, NopChecksum, NopTail>::new(&mut buf);

        assert_eq!(tx.write(Nibble::zero(), &[]), Ok(true));
        let len = tx.flush();
        assert_eq!(&tx.buf()[..len], &[0b0000_0000]);

        assert_eq!(tx.write(Nibble::max(), &[]), Ok(true));
        let len = tx.flush();
        assert_eq!(&tx.buf()[..len], &[0b0011_1111, 0]);

        let msg4 = &[0xAA, 0xBB, 0xCC, 0xDD][..];
        assert_eq!(tx.write(Nibble::zero(), msg4), Ok(false));
        let len = tx.flush();
        assert_eq!(&tx.buf()[..len], &[0b0100_0001, 0xAA, 0xBB, 0xCC]);
        assert_eq!(tx.write(Nibble::zero(), msg4), Ok(true));
        let len = tx.flush();
        assert_eq!(&tx.buf()[..len], &[0b1100_0000, 0xDD]);
    }

    #[test]
    fn split_message_with_extended_user_kind() {
        let mut buf = [0u8; 4];
        let mut tx = Tx::<U2Head, NopChecksum, NopTail>::new(&mut buf);
        let msg3 = [0xAA, 0xBB, 0xCC];

        assert_eq!(tx.write(Nibble::max(), &msg3), Ok(false));
        let len = tx.flush();
        assert_eq!(
            &tx.buf()[..len],
            &[0b0111_1111, 0b1000_0000, 0b0011_0000, 0xAA]
        );

        assert_eq!(tx.write(Nibble::max(), &msg3), Ok(true));
        let len = tx.flush();
        assert_eq!(&tx.buf()[..len], &[0b1111_1111, 0, 0xBB, 0xCC]);
    }

    #[test]
    fn split_message_with_continue() {
        let mut buf = [0u8; 4];
        let mut tx = Tx::<U2Head, NopChecksum, NopTail>::new(&mut buf);
        let msg7 = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0xAB];

        assert_eq!(tx.write(Nibble::zero(), &msg7), Ok(false));
        let len = tx.flush();
        assert_eq!(&tx.buf()[..len], &[0b0100_0100, 0xAA, 0xBB, 0xCC]);

        assert_eq!(tx.write(Nibble::zero(), &msg7), Ok(false));
        let len = tx.flush();
        assert_eq!(&tx.buf()[..len], &[0b1000_0000, 0xDD, 0xEE, 0xFF]);

        assert_eq!(tx.write(Nibble::zero(), &msg7), Ok(true));
        let len = tx.flush();
        assert_eq!(&tx.buf()[..len], &[0b1100_0000, 0xAB]);
    }

    #[test]
    fn round_trip() {
        let cases = [
            (MessageKind::Full, Nibble::zero(), 0usize),
            (MessageKind::Full, Nibble::new(3).unwrap(), 0),
            (MessageKind::Start, Nibble::zero(), 4),
            (MessageKind::Continue, Nibble::new(2).unwrap(), 7),
            (MessageKind::End, Nibble::new(1).unwrap(), 10),
            (MessageKind::Full, Nibble::max(), 4),
            (MessageKind::Start, Nibble::max(), 11),
            (MessageKind::Continue, Nibble::max(), 1023),
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
}
