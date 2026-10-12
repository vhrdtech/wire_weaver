//! Plain fields are read and written without an error check each (`read_u8_latch`, `write_u8_latch`, ..), the
//! derived code checks once per type. These tests make sure no error gets lost on the way: every buffer that is
//! too short, for reading or for writing, must still be an error, and the error is the first one, the one reading
//! every field with `?` gives (SW-41), also in types that mix plain fields with lists and nested types.

use shrink_wrap::prelude::*;
use shrink_wrap::{BufReader, Error};

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
    // a plain field right before a list, an evolvable nested value, an option of one
    Load {
        off: u16,
        frames: RefVec<'i, u16>,
    },
    Nested {
        line: u8,
        wave: Wave,
        after: u32,
    },
    Opt {
        a: u16,
        glitch: Option<Wave>,
        b: u8,
    },
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

/// [Request] read the way it was before SW-34: every field with `?`, in the same order.
fn request_with_checks<'i>(rd: &mut BufReader<'i>) -> Result<Request<'i>, Error> {
    fn format(rd: &mut BufReader) -> Result<Format, Error> {
        Ok(Format {
            bit: rd.read_u32()?,
            bits: rd.read_u8()?,
            msb_first: rd.read_bool()?,
            inv: rd.read_bool()?,
        })
    }
    fn wave(rd: &mut BufReader) -> Result<Wave, Error> {
        // an evolvable type: its size from the back, then its fields in a reader of their own
        let size = rd.read_rev_len()?;
        let mut rd = rd.split(size)?;
        Ok(Wave {
            line: rd.read_u8()?,
            idle: rd.read_bool()?,
            delay: rd.read_u32()?,
            high: rd.read_i16()?,
            gain: rd.read_f32()?,
        })
    }
    Ok(match rd.read_u8()? {
        0 => Request::Hello,
        1 => Request::Stop {
            mask: rd.read_u16()?,
        },
        2 => {
            let waves = rd.read()?;
            Request::Waves {
                waves,
                defer: rd.read_bool()?,
            }
        }
        3 => {
            let line = rd.read_u8()?;
            let fmt = format(rd)?;
            let glitch = if rd.read_bool()? {
                Some(wave(rd)?)
            } else {
                None
            };
            Request::Urx {
                line,
                fmt,
                glitch,
                defer: rd.read_bool()?,
            }
        }
        4 => Request::Tuple(rd.read_u8()?, rd.read_u32()?),
        5 => {
            let off = rd.read_u16()?;
            Request::Load {
                off,
                frames: rd.read()?,
            }
        }
        6 => {
            let line = rd.read_u8()?;
            let wave = wave(rd)?;
            Request::Nested {
                line,
                wave,
                after: rd.read_u32()?,
            }
        }
        7 => {
            let a = rd.read_u16()?;
            let glitch = if rd.read_bool()? {
                Some(wave(rd)?)
            } else {
                None
            };
            Request::Opt {
                a,
                glitch,
                b: rd.read_u8()?,
            }
        }
        _ => return Err(Error::EnumFutureVersionOrMalformedData),
    })
}

fn check_first_error(buf: &[u8], what: &str) {
    let (mut rd_old, mut rd_new) = (BufReader::new(buf), BufReader::new(buf));
    // values compared by their bytes: a NaN in a gain is not equal to itself
    let bytes = |r: &Result<Request, Error>| {
        r.as_ref()
            .map(|v| v.to_ww_bytes(&mut [0u8; 128]).unwrap().to_vec())
            .map_err(|e| *e)
    };
    let old = request_with_checks(&mut rd_old);
    let new = Request::des_shrink_wrap(&mut rd_new);
    assert_eq!(bytes(&new), bytes(&old), "{what}");
    if old.is_ok() {
        assert_eq!(rd_new, rd_old, "{what}");
    }
    let owned = RequestOwned::from_ww_bytes_owned(buf);
    assert_eq!(owned.map(|_| ()), old.map(|_| ()), "owned, {what}");
}

/// Every truncation of every variant (each field position cut), every byte damaged: the same result as reading
/// field by field, error included.
#[test]
fn first_error_in_mixed_types() {
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
        Request::Load {
            off: 0x0102,
            frames: RefVec::Slice { slice: &[1, 2, 3] },
        },
        Request::Nested {
            line: 9,
            wave: WAVE,
            after: 0x0A0B0C0D,
        },
        Request::Opt {
            a: 0x0304,
            glitch: Some(WAVE),
            b: 5,
        },
        Request::Opt {
            a: 0x0304,
            glitch: None,
            b: 5,
        },
    ];
    let mut errors = 0;
    for request in &requests {
        let mut scratch = [0u8; 128];
        let bytes = request.to_ww_bytes(&mut scratch).unwrap().to_vec();
        check_first_error(&bytes, &format!("{request:?}"));
        for cut in 0..bytes.len() {
            check_first_error(&bytes[..cut], &format!("{request:?} cut at {cut}"));
            check_first_error(&bytes[cut..], &format!("{request:?} from {cut}"));
            errors += Request::from_ww_bytes(&bytes[..cut]).is_err() as usize;
        }
        for i in 0..bytes.len() {
            for xor in [0x01, 0x04, 0x10, 0x80, 0xFF] {
                let mut buf = bytes.clone();
                buf[i] ^= xor;
                check_first_error(&buf, &format!("{request:?} byte {i} ^ {xor:#x}"));
            }
        }
    }
    assert!(errors > 50, "{errors}");
    // random bytes
    let mut x = 0x5357_3431_0000_0001u64;
    for _ in 0..20_000 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let len = (x >> 59) as usize;
        let buf: Vec<u8> = (0..len)
            .map(|i| (x >> (i % 8 * 8)) as u8 ^ (i as u8).wrapping_mul(0x9D))
            .collect();
        // mostly known discriminants
        let mut buf = buf;
        if let Some(first) = buf.first_mut() {
            *first %= 9;
        }
        check_first_error(&buf, &format!("{buf:02x?}"));
    }
}

/// An enum without a variant 0, in the same reader after a plain field: a discriminant read after a failed read
/// gives 0, which is no variant, and the first error still comes first (not `EnumFutureVersionOrMalformedData`).
#[test]
fn first_error_before_an_unknown_discriminant() {
    #[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone, Copy), ww_repr = u8, sized)]
    enum NoZero {
        A = 1,
        B = 2,
    }
    #[derive_shrink_wrap(borrowed, derive(Debug, PartialEq), sized)]
    struct S {
        a: u32,
        e: NoZero,
        b: u8,
    }
    fn s_with_checks(rd: &mut BufReader) -> Result<S, Error> {
        let a = rd.read_u32()?;
        let e = match rd.read_u8()? {
            1 => NoZero::A,
            2 => NoZero::B,
            _ => return Err(Error::EnumFutureVersionOrMalformedData),
        };
        Ok(S {
            a,
            e,
            b: rd.read_u8()?,
        })
    }
    for buf in [
        &[][..],
        &[1],
        &[1, 2, 3],
        &[1, 2, 3, 4],
        &[1, 2, 3, 4, 2],
        &[1, 2, 3, 4, 7],
        &[1, 2, 3, 4, 7, 9],
        &[1, 2, 3, 4, 1, 9],
    ] {
        let (mut rd_old, mut rd_new) = (BufReader::new(buf), BufReader::new(buf));
        assert_eq!(
            S::des_shrink_wrap(&mut rd_new),
            s_with_checks(&mut rd_old),
            "{buf:?}"
        );
        let (mut rd_old, mut rd_new) = (BufReader::new(buf), BufReader::new(buf));
        let old = match rd_old.read_u8() {
            Ok(1) => Ok(NoZero::A),
            Ok(2) => Ok(NoZero::B),
            Ok(_) => Err(Error::EnumFutureVersionOrMalformedData),
            Err(e) => Err(e),
        };
        assert_eq!(NoZero::des_shrink_wrap(&mut rd_new), old, "{buf:?}");
    }
}
