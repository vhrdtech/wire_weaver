//! [Checksum] implementation on top of the [crc](https://docs.rs/crc) crate.
//!
//! Width (u8 / u16 / u32 / u64) and algorithm are selected via a [CrcAlgorithm] type:
//!
//! ```
//! use ww_framer::crc::{CrcAlgorithm, CrcChecksum};
//!
//! // pick one of the provided algorithms
//! type LinkChecksum = CrcChecksum<ww_framer::crc::Crc16Usb>;
//!
//! // or define your own, any algorithm from `crc::*` works
//! struct Crc8Autosar;
//! impl CrcAlgorithm for Crc8Autosar {
//!     type Width = u8;
//!     const CRC: crc::Crc<u8> = crc::Crc::<u8>::new(&crc::CRC_8_AUTOSAR);
//! }
//! type Link8Checksum = CrcChecksum<Crc8Autosar>;
//! ```
//!
//! By default only split messages are checksummed, as frame based media (USB, CAN, etc.) already
//! protects each frame and a checksum over the whole message only helps to detect lost or
//! reordered frames. Set `FULL = true` to also checksum [MessageKind::Full](crate::traits::MessageKind) messages:
//!
//! ```
//! # use ww_framer::crc::{CrcChecksum, Crc16Usb};
//! type StreamChecksum = CrcChecksum<Crc16Usb, true>;
//! ```

use core::marker::PhantomData;

use shrink_wrap::{BufReader, BufWriter};

use crate::traits::{Checksum, RdError, WrError};

/// CRC width that can be written to and read from a frame. Implemented for u8, u16, u32 and u64.
pub trait CrcWidth: crc::Width + Copy + PartialEq {
    const LEN_BYTES: usize;

    fn checksum(crc: &crc::Crc<Self>, data: &[u8]) -> Self;
    fn write(self, wr: &mut BufWriter<'_>) -> Result<(), WrError>;
    fn read(rd: &mut BufReader<'_>) -> Result<Self, RdError>;
}

macro_rules! impl_crc_width {
    ($ty:ty, $write:ident, $read:ident) => {
        impl CrcWidth for $ty {
            const LEN_BYTES: usize = size_of::<$ty>();

            #[inline]
            fn checksum(crc: &crc::Crc<Self>, data: &[u8]) -> Self {
                crc.checksum(data)
            }

            #[inline]
            fn write(self, wr: &mut BufWriter<'_>) -> Result<(), WrError> {
                wr.$write(self)?;
                Ok(())
            }

            #[inline]
            fn read(rd: &mut BufReader<'_>) -> Result<Self, RdError> {
                Ok(rd.$read()?)
            }
        }
    };
}

impl_crc_width!(u8, write_u8, read_u8);
impl_crc_width!(u16, write_u16, read_u16);
impl_crc_width!(u32, write_u32, read_u32);
impl_crc_width!(u64, write_u64, read_u64);

/// Selects CRC width and algorithm for [CrcChecksum].
///
/// `CRC` is a const, so that the lookup table (256 entries of `Width`) is computed at compile time.
pub trait CrcAlgorithm {
    type Width: CrcWidth;
    const CRC: crc::Crc<Self::Width>;
}

macro_rules! algorithm {
    ($(#[$meta:meta])* $name:ident, $ty:ty, $alg:ident) => {
        $(#[$meta])*
        pub struct $name;

        impl CrcAlgorithm for $name {
            type Width = $ty;
            const CRC: crc::Crc<$ty> = crc::Crc::<$ty>::new(&crc::$alg);
        }
    };
}

algorithm!(
    /// CRC-8/SMBUS (poly 0x07), 1 byte.
    Crc8Smbus, u8, CRC_8_SMBUS
);
algorithm!(
    /// CRC-16/USB (poly 0x8005, reflected), 2 bytes.
    Crc16Usb, u16, CRC_16_USB
);
algorithm!(
    /// CRC-16/IBM-SDLC aka CRC-16/X-25 (poly 0x1021, reflected), 2 bytes. Used in HDLC / PPP.
    Crc16IbmSdlc, u16, CRC_16_IBM_SDLC
);
algorithm!(
    /// CRC-32/ISO-HDLC, the common "CRC-32" (zlib, Ethernet, PNG), 4 bytes.
    Crc32IsoHdlc, u32, CRC_32_ISO_HDLC
);

/// [Checksum] over a whole message using the [crc](https://docs.rs/crc) crate,
/// written little endian right after the message payload.
///
/// * `A` - width and algorithm, see [CrcAlgorithm].
/// * `FULL` - whether to also checksum messages that fit into one frame. Defaults to `false`,
///   which is enough for frame based media (USB, CAN, etc.) that already protects each frame.
///
/// See the [module docs](self) for examples.
pub struct CrcChecksum<A: CrcAlgorithm, const FULL: bool = false> {
    _phantom: PhantomData<A>,
}

impl<A: CrcAlgorithm, const FULL: bool> Checksum for CrcChecksum<A, FULL> {
    const LEN_BYTES_FULL: usize = if FULL { A::Width::LEN_BYTES } else { 0 };
    const LEN_BYTES_SPLIT: usize = A::Width::LEN_BYTES;

    fn write(message: &[u8], is_split: bool, wr: &mut BufWriter<'_>) -> Result<(), WrError> {
        if is_split || FULL {
            A::Width::checksum(&A::CRC, message).write(wr)?;
        }
        Ok(())
    }

    fn read(message: &[u8], is_split: bool, rd: &mut BufReader<'_>) -> Result<(), RdError> {
        if is_split || FULL {
            let expected = A::Width::checksum(&A::CRC, message);
            if A::Width::read(rd)? != expected {
                return Err(RdError::ChecksumMismatch);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use shrink_wrap::{BufReader, BufWriter};

    use super::*;
    use crate::framed::U2Head;
    use crate::tests::common::*;
    use crate::traits::NopTail;
    use crate::{FramedRx, Tx};

    /// Standard check value of every catalog algorithm is computed over "123456789".
    const CHECK: &[u8] = b"123456789";

    #[test]
    fn known_check_values() {
        let mut buf = [0u8; 8];

        let mut wr = BufWriter::new(&mut buf);
        CrcChecksum::<Crc8Smbus>::write(CHECK, true, &mut wr).unwrap();
        assert_eq!(wr.finish().unwrap(), &[0xF4]);

        let mut wr = BufWriter::new(&mut buf);
        CrcChecksum::<Crc16Usb>::write(CHECK, true, &mut wr).unwrap();
        assert_eq!(wr.finish().unwrap(), &0xB4C8u16.to_le_bytes());

        let mut wr = BufWriter::new(&mut buf);
        CrcChecksum::<Crc16IbmSdlc>::write(CHECK, true, &mut wr).unwrap();
        assert_eq!(wr.finish().unwrap(), &0x906Eu16.to_le_bytes());

        let mut wr = BufWriter::new(&mut buf);
        CrcChecksum::<Crc32IsoHdlc>::write(CHECK, true, &mut wr).unwrap();
        assert_eq!(wr.finish().unwrap(), &0xCBF4_3926u32.to_le_bytes());
    }

    #[test]
    fn full_is_not_checksummed_by_default() {
        assert_eq!(CrcChecksum::<Crc16Usb>::LEN_BYTES_FULL, 0);
        assert_eq!(CrcChecksum::<Crc16Usb>::LEN_BYTES_SPLIT, 2);
        assert_eq!(CrcChecksum::<Crc16Usb, true>::LEN_BYTES_FULL, 2);
        assert_eq!(CrcChecksum::<Crc32IsoHdlc>::LEN_BYTES_SPLIT, 4);

        let mut buf = [0u8; 8];
        let mut wr = BufWriter::new(&mut buf);
        CrcChecksum::<Crc16Usb>::write(CHECK, false, &mut wr).unwrap();
        assert_eq!(wr.finish().unwrap(), &[]);

        let mut wr = BufWriter::new(&mut buf);
        CrcChecksum::<Crc16Usb, true>::write(CHECK, false, &mut wr).unwrap();
        assert_eq!(wr.finish().unwrap(), &0xB4C8u16.to_le_bytes());
    }

    #[test]
    fn read_detects_mismatch() {
        let mut rd = BufReader::new(&[0xC8, 0xB4]);
        assert_eq!(CrcChecksum::<Crc16Usb>::read(CHECK, true, &mut rd), Ok(()));

        let mut rd = BufReader::new(&[0xC8, 0xB5]);
        assert_eq!(
            CrcChecksum::<Crc16Usb>::read(CHECK, true, &mut rd),
            Err(RdError::ChecksumMismatch)
        );

        // not enough bytes
        let mut rd = BufReader::new(&[0xC8]);
        assert_eq!(
            CrcChecksum::<Crc16Usb>::read(CHECK, true, &mut rd),
            Err(RdError::NeedMoreData)
        );

        // Full is not checked by default, nothing is consumed
        let mut rd = BufReader::new(&[0xC8, 0xB5]);
        assert_eq!(CrcChecksum::<Crc16Usb>::read(CHECK, false, &mut rd), Ok(()));
        assert_eq!(rd.bytes_left(), 2);
    }

    #[test]
    fn split_message_layout() {
        type LinkTx<'a> = Tx<'a, U2Head, CrcChecksum<Crc16Usb>, NopTail>;
        let mut buf = [0u8; 8];
        let mut tx = LinkTx::new(&mut buf);
        let msg = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let crc = crc::Crc::<u16>::new(&crc::CRC_16_USB).checksum(&msg);

        assert_eq!(tx.write(0, &msg), Ok(false));
        let len = tx.flush();
        assert_eq!(
            &tx.buf()[..len],
            &[START_10[0], START_10[1], 1, 2, 3, 4, 5, 6]
        );

        // remaining 4 bytes + 2 byte crc fit into 8 byte frame with 1 byte head
        assert_eq!(tx.write(0, &msg), Ok(true));
        let len = tx.flush();
        let [c0, c1] = crc.to_le_bytes();
        assert_eq!(&tx.buf()[..len], &[END_4, 7, 8, 9, 10, c0, c1]);
    }

    #[test]
    fn tx_rx_round_trip_all_widths() {
        fn run<C: Checksum>() {
            let messages: [(u8, &[u8]); 4] = [
                (0, &[]),
                (1, &[0xAA, 0xBB, 0xCC, 0xDD]),
                (
                    2,
                    &[
                        1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
                    ],
                ),
                (255, &[9, 8, 7]),
            ];
            let mut tx_buf = [0u8; 8];
            let mut tx = Tx::<U2Head, C, NopTail>::new(&mut tx_buf);
            let mut rx_buf = [0u8; 40];
            let mut rx = FramedRx::<U2Head, C, NopTail>::new(&mut rx_buf);

            let mut received = 0;
            for (user_kind, message) in messages {
                loop {
                    let done = tx.write(user_kind, message).unwrap();
                    let len = tx.flush();
                    rx.stage(&tx.buf()[..len]).unwrap();
                    loop {
                        rx.reassemble();
                        let Some((k, m)) = rx.message() else { break };
                        assert_eq!((k, m), messages[received]);
                        received += 1;
                    }
                    if done {
                        break;
                    }
                }
            }
            assert_eq!(received, messages.len());
        }

        run::<CrcChecksum<Crc8Smbus>>();
        run::<CrcChecksum<Crc16Usb>>();
        run::<CrcChecksum<Crc16IbmSdlc>>();
        run::<CrcChecksum<Crc32IsoHdlc>>();
        run::<CrcChecksum<Crc8Smbus, true>>();
        run::<CrcChecksum<Crc32IsoHdlc, true>>();
    }

    #[test]
    fn lost_frame_detected_by_crc() {
        type LinkTx<'a> = Tx<'a, U2Head, CrcChecksum<Crc16Usb>, NopTail>;
        type LinkRx<'a> = FramedRx<'a, U2Head, CrcChecksum<Crc16Usb>, NopTail>;
        let mut tx_buf = [0u8; 8];
        let mut tx = LinkTx::new(&mut tx_buf);
        let mut rx_buf = [0u8; 40];
        let mut rx = LinkRx::new(&mut rx_buf);

        // 20 byte message, 4 frames: Start, Continue, Continue, End
        let msg: [u8; 20] = core::array::from_fn(|i| i as u8 + 1);
        let mut frames: [[u8; 8]; 4] = [[0; 8]; 4];
        let mut lens = [0usize; 4];
        for i in 0..4 {
            let done = tx.write(0, &msg).unwrap();
            lens[i] = tx.flush();
            frames[i][..lens[i]].copy_from_slice(&tx.buf()[..lens[i]]);
            assert_eq!(done, i == 3);
        }

        // Continue frames both carry 6 bytes; if they are swapped the lengths still line up,
        // so only the CRC can tell
        for i in [0, 2, 1, 3] {
            rx.stage(&frames[i][..lens[i]]).unwrap();
            rx.reassemble();
            assert_eq!(rx.message(), None);
        }

        // sent in order, message arrives
        for i in 0..4 {
            rx.stage(&frames[i][..lens[i]]).unwrap();
            rx.reassemble();
            if i < 3 {
                assert_eq!(rx.message(), None);
            }
        }
        assert_eq!(rx.message(), Some((0, &msg[..])));
    }
}
