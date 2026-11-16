//! The CRCs of WireWeaver's framework crates, one lookup table per algorithm, sized by cargo feature.
//!
//! `ww_framer`, `ww_link`, `ww_block`, `ww_settings` and `ww_stream` take their CRCs from here. Each algorithm is a
//! `static`, so a firmware that uses one algorithm from several crates (the CRC-32 of `ww_link`'s UDP framing and of
//! `ww_block`'s writes, the CRC-32C of `ww_settings` and `ww_stream`'s stores) links its table and its code once.
//!
//! The table size is a choice of the whole firmware, made once with a cargo feature on this crate. Libraries never
//! turn one on for `no_std`; their `std` features turn on `table-slice16`, the fastest, for hosts.
//!
//! | Feature | Table per algorithm | Speed |
//! |---|---|---|
//! | none | none, bit by bit | slowest, smallest flash |
//! | `table-256` | 256 entries (1 KiB for a CRC-32) | about 8x bitwise |
//! | `table-slice16` | 16 x 256 entries (16 KiB for a CRC-32) | fastest; wins when both are on |
//!
//! Every table size computes the same bytes.
//!
//! ```
//! assert_eq!(ww_crc::CRC32_ISO_HDLC.checksum(b"123456789"), 0xcbf4_3926);
//!
//! // a CRC over several pieces
//! let mut digest = ww_crc::CRC32C.digest();
//! digest.update(b"1234");
//! digest.update(b"56789");
//! assert_eq!(digest.finalize(), 0xe306_9283);
//! ```
//!
//! Another algorithm of the [crc] catalogue gets the same table size through [Crc]:
//!
//! ```
//! static CRC8_AUTOSAR: ww_crc::Crc<u8> = ww_crc::Crc::<u8>::new(&ww_crc::crc::CRC_8_AUTOSAR);
//! assert_eq!(CRC8_AUTOSAR.checksum(b"123456789"), 0xdf);
//! ```
#![no_std]

pub use crc;

/// The lookup table every algorithm of this crate uses, picked by cargo feature (see the [crate docs](crate)).
#[cfg(feature = "table-slice16")]
pub type Table = crc::Table<16>;
/// The lookup table every algorithm of this crate uses, picked by cargo feature (see the [crate docs](crate)).
#[cfg(all(feature = "table-256", not(feature = "table-slice16")))]
pub type Table = crc::Table<1>;
/// The lookup table every algorithm of this crate uses, picked by cargo feature (see the [crate docs](crate)).
#[cfg(not(any(feature = "table-256", feature = "table-slice16")))]
pub type Table = crc::NoTable;

/// A CRC algorithm of width `W` with the feature-picked [Table]. Keep it in a `static`, so that its table is in
/// flash once.
pub type Crc<W> = crc::Crc<W, Table>;

/// A CRC in progress over several pieces, from [Crc::digest].
pub type Digest<W> = crc::Digest<'static, W, Table>;

/// CRC-32/ISO-HDLC, the common "CRC-32" of zip, PNG and Ethernet (polynomial `0x04c11db7` reflected, init and final
/// xor `0xffffffff`; check value `0xcbf43926`). `ww_link`'s UDP framing and `ww_block`'s writes use it.
pub static CRC32_ISO_HDLC: Crc<u32> = Crc::<u32>::new(&crc::CRC_32_ISO_HDLC);

/// CRC-32C (Castagnoli, `CRC_32_ISCSI`; check value `0xe3069283`). `ww_settings`'s store and `ww_stream`'s log and
/// persistent RAM stores use it.
pub static CRC32C: Crc<u32> = Crc::<u32>::new(&crc::CRC_32_ISCSI);

/// CRC-16/USB (polynomial `0x8005` reflected; check value `0xb4c8`), a `ww_framer` checksum.
pub static CRC16_USB: Crc<u16> = Crc::<u16>::new(&crc::CRC_16_USB);

/// CRC-16/IBM-SDLC, also CRC-16/X-25, of HDLC and PPP (polynomial `0x1021` reflected; check value `0x906e`), a
/// `ww_framer` checksum.
pub static CRC16_IBM_SDLC: Crc<u16> = Crc::<u16>::new(&crc::CRC_16_IBM_SDLC);

/// CRC-8/SMBUS (polynomial `0x07`; check value `0xf4`), a `ww_framer` checksum.
pub static CRC8_SMBUS: Crc<u8> = Crc::<u8>::new(&crc::CRC_8_SMBUS);

#[cfg(test)]
mod tests {
    use super::*;

    const CHECK: &[u8] = b"123456789";

    #[test]
    fn check_values() {
        assert_eq!(CRC32_ISO_HDLC.checksum(CHECK), 0xcbf4_3926);
        assert_eq!(CRC32C.checksum(CHECK), 0xe306_9283);
        assert_eq!(CRC16_USB.checksum(CHECK), 0xb4c8);
        assert_eq!(CRC16_IBM_SDLC.checksum(CHECK), 0x906e);
        assert_eq!(CRC8_SMBUS.checksum(CHECK), 0xf4);
    }

    /// Whichever table the features pick, every length gives the bytes of the 256-entry reference table.
    #[test]
    fn matches_reference_table_at_every_length() {
        let iso_hdlc = crc::Crc::<u32, crc::Table<1>>::new(&crc::CRC_32_ISO_HDLC);
        let crc32c = crc::Crc::<u32, crc::Table<1>>::new(&crc::CRC_32_ISCSI);
        let usb = crc::Crc::<u16, crc::Table<1>>::new(&crc::CRC_16_USB);
        let smbus = crc::Crc::<u8, crc::Table<1>>::new(&crc::CRC_8_SMBUS);
        let data: [u8; 300] = core::array::from_fn(|i| (i as u8).wrapping_mul(37).wrapping_add(11));
        for len in 0..=data.len() {
            let d = &data[..len];
            assert_eq!(
                CRC32_ISO_HDLC.checksum(d),
                iso_hdlc.checksum(d),
                "len={len}"
            );
            assert_eq!(CRC32C.checksum(d), crc32c.checksum(d), "len={len}");
            assert_eq!(CRC16_USB.checksum(d), usb.checksum(d), "len={len}");
            assert_eq!(CRC8_SMBUS.checksum(d), smbus.checksum(d), "len={len}");
        }
    }

    /// A digest continues correctly across a split at any offset, including ones off a 16-byte boundary.
    #[test]
    fn digest_continues_at_any_split() {
        let data: [u8; 100] = core::array::from_fn(|i| i as u8 ^ 0x5a);
        let whole = CRC32_ISO_HDLC.checksum(&data);
        for at in 0..=data.len() {
            let (a, b) = data.split_at(at);
            let mut digest = CRC32_ISO_HDLC.digest();
            digest.update(a);
            digest.update(b);
            assert_eq!(digest.finalize(), whole, "split at {at}");
        }
    }
}
