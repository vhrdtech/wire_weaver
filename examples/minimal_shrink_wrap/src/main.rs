//! Using the ShrinkWrap wire format directly, through `wire_weaver::shrink_wrap`, with no API trait involved:
//! writing and reading a couple of plain values by hand.
use wire_weaver::shrink_wrap::prelude::*;

fn simple_wr() {
    let mut buf = [0u8; 256];
    let bytes = write_two_values(&mut buf);
    assert_eq!(bytes, [0x80, 0xaa]);
}

/// Writes a `bool` and a `u8` by hand: the `bool` takes one bit, and the `u8` starts on the next byte.
fn write_two_values(buf: &mut [u8]) -> Vec<u8> {
    // --8<-- [start:write]
    let mut wr = BufWriter::new(buf);
    wr.write_bool(true).unwrap();
    wr.write_u8(0xaa).unwrap();
    let bytes = wr.finish().unwrap();
    // --8<-- [end:write]
    bytes.to_vec()
}

fn simple_rd() {
    let buf = [0x80, 0xaa];
    let mut rd = BufReader::new(&buf[..]);
    assert!(rd.read_bool().unwrap());
    assert_eq!(rd.read_u8().unwrap(), 0xaa);
}

fn main() {
    simple_wr();
    simple_rd();
}

/// The derive macro finds `shrink_wrap` through `wire_weaver` when only that is a dependency, so no prelude
/// import is needed and the names it uses don't clash with the module's own.
#[cfg(test)]
mod qualified_names {
    #[allow(dead_code, reason = "name-resolution fixture, never used")]
    struct BufReader;
    #[allow(dead_code, reason = "name-resolution fixture, never used")]
    struct Error;

    #[wire_weaver::derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq))]
    struct Clash<'i> {
        size: wire_weaver::shrink_wrap::TailSize<1>,
        s: &'i str,
    }

    #[test]
    fn round_trip() {
        use wire_weaver::shrink_wrap::{
            DeserializeShrinkWrap, DeserializeShrinkWrapOwned, SerializeShrinkWrap, TailSize,
        };
        let mut buf = [0u8; 8];
        let bytes = Clash {
            size: TailSize(0),
            s: "ok",
        }
        .to_ww_bytes(&mut buf)
        .unwrap();
        assert_eq!(bytes, &[0x03, b'o', b'k', 0x02]);
        assert_eq!(Clash::from_ww_bytes(bytes).unwrap().s, "ok");
        let owned = ClashOwned::from_ww_bytes_owned(bytes).unwrap();
        assert_eq!(owned.s, "ok");
    }
}

/// The bytes `write_two_values` writes, as the book's examples page shows them.
#[cfg(test)]
mod book {
    #[test]
    fn book_output_two_values() {
        let mut buf = [0u8; 8];
        let bytes = super::write_two_values(&mut buf);
        let hex: Vec<String> = bytes.iter().map(|b| format!("{b:02x}")).collect();
        book_outputs::check(
            "minimal_shrink_wrap/two_values.txt",
            &(hex.join(" ") + "\n"),
        );
    }
}
