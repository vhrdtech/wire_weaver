//! Plain fields are read and written without an error check each (`read_u8_latch`, `write_u8_latch`, ..), the
//! derived code checks once per type. These tests make sure no error gets lost on the way: every buffer that is
//! too short, for reading or for writing, must still be an error.

use shrink_wrap::prelude::*;

#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Clone, Copy), sized)]
struct Format {
    bit: u32,
    bits: u8,
    msb_first: bool,
    inv: bool,
}

#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Clone, Copy))]
struct Wave {
    line: u8,
    idle: bool,
    delay: u32,
    high: i16,
    gain: f32,
}

#[derive_shrink_wrap(owned, derive(Debug, PartialEq, Clone), ww_repr = u8)]
enum Request<'i> {
    Hello,
    Stop {
        mask: u16,
    },
    Waves {
        waves: RefVec<'i, Wave>,
        defer: bool,
    },
    Urx {
        line: u8,
        fmt: Format,
        glitch: Option<Wave>,
        defer: bool,
    },
    Tuple(u8, u32),
}

/// `value` round-trips, and every shorter read buffer and every shorter write buffer is an error.
fn check<'i, T>(value: &T, scratch: &'i mut [u8; 128])
where
    T: SerializeShrinkWrap + DeserializeShrinkWrap<'i> + PartialEq + core::fmt::Debug,
{
    let len = value.to_ww_bytes(&mut [0u8; 128]).unwrap().len();
    for short in 0..len {
        let mut buf = [0u8; 128];
        assert!(
            value.to_ww_bytes(&mut buf[..short]).is_err(),
            "write into {short} of {len} bytes"
        );
    }
    let bytes: &'i [u8] = value.to_ww_bytes(scratch).unwrap();
    assert_eq!(&T::from_ww_bytes(bytes).unwrap(), value);
    for short in 0..len {
        assert!(
            T::from_ww_bytes(&bytes[..short]).is_err(),
            "read from {short} of {len} bytes"
        );
    }
}

const WAVE: Wave = Wave {
    line: 1,
    idle: true,
    delay: 0x01020304,
    high: -2,
    gain: 1.5,
};
const FORMAT: Format = Format {
    bit: 651,
    bits: 8,
    msb_first: false,
    inv: true,
};

#[test]
fn plain_struct() {
    check(&WAVE, &mut [0u8; 128]);
    check(&FORMAT, &mut [0u8; 128]);
}

#[test]
fn enum_variants() {
    let waves = [WAVE, WAVE];
    let requests = [
        Request::Hello,
        Request::Stop { mask: 0x1234 },
        Request::Waves {
            waves: RefVec::Slice { slice: &waves },
            defer: true,
        },
        Request::Urx {
            line: 3,
            fmt: FORMAT,
            glitch: Some(WAVE),
            defer: true,
        },
        Request::Urx {
            line: 3,
            fmt: FORMAT,
            glitch: None,
            defer: false,
        },
        Request::Tuple(7, 0xAABBCCDD),
    ];
    for request in &requests {
        let mut scratch = [0u8; 128];
        let len = request.to_ww_bytes(&mut [0u8; 128]).unwrap().len();
        for short in 0..len {
            let mut buf = [0u8; 128];
            assert!(
                request.to_ww_bytes(&mut buf[..short]).is_err(),
                "{request:?} into {short} of {len} bytes"
            );
        }
        let bytes = request.to_ww_bytes(&mut scratch).unwrap();
        assert_eq!(&Request::from_ww_bytes(bytes).unwrap(), request);
        // The owned side reads through the same latched code
        assert!(RequestOwned::from_ww_bytes_owned(bytes).is_ok());
        for short in 0..len {
            // a shorter buffer is an error, or (cut inside the reversed lengths at the end) other valid data, never a panic
            if let Ok(other) = Request::from_ww_bytes(&bytes[..short]) {
                assert_ne!(&other, request, "{request:?} from {short} of {len} bytes");
            }
            let _ = RequestOwned::from_ww_bytes_owned(&bytes[..short]);
        }
    }
    // without lists there is nothing at the end of the buffer: every shorter one is an error
    for request in [
        Request::Stop { mask: 0x1234 },
        Request::Tuple(7, 0xAABBCCDD),
    ] {
        let mut scratch = [0u8; 128];
        let bytes = request.to_ww_bytes(&mut scratch).unwrap();
        for short in 0..bytes.len() {
            assert!(Request::from_ww_bytes(&bytes[..short]).is_err());
        }
    }
}

/// A field with a default is read with a check of its own, its flag too: missing in old data, it gets the
/// default and is no error.
#[test]
fn default_fields_still_fall_back() {
    #[derive_shrink_wrap(borrowed, derive(Debug, PartialEq))]
    struct V1 {
        a: u8,
    }
    #[derive_shrink_wrap(borrowed, derive(Debug, PartialEq))]
    struct V2 {
        a: u8,
        #[default = None]
        b: Option<u16>,
    }
    let mut scratch = [0u8; 8];
    let bytes = V1 { a: 1 }.to_ww_bytes(&mut scratch).unwrap();
    assert_eq!(V2::from_ww_bytes(bytes), Ok(V2 { a: 1, b: None }));
    assert!(V2::from_ww_bytes(&[]).is_err());
    let bytes = V2 { a: 1, b: Some(2) }.to_ww_bytes(&mut scratch).unwrap();
    assert_eq!(V1::from_ww_bytes(bytes), Ok(V1 { a: 1 }));
}

/// The owned writer cannot run out of space; it has the same methods so that generated code is the same.
#[test]
fn owned_writer() {
    let bytes = WAVE.to_ww_bytes_owned().unwrap();
    let mut scratch = [0u8; 64];
    assert_eq!(bytes, WAVE.to_ww_bytes(&mut scratch).unwrap());
}
