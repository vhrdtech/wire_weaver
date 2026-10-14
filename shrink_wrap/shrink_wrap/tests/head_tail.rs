//! The head+tail encoding (SW-46): offsets, exact bytes, round trips, garbage and short buffers, the C reader.

use hex_literal::hex;
use shrink_wrap::head_tail::{HeadTail, List, write_c_header};
use shrink_wrap::prelude::*;

#[derive_shrink_wrap(borrowed, fixed_layout, derive(Clone, Copy, Debug, PartialEq))]
pub struct Wave {
    pub line: u8,
    pub idle: bool,
    pub hold: bool,
    pub delay: u32,
}

/// Two options in the middle, no lifetime
#[derive_shrink_wrap(head_tail, derive(Clone, Copy, Debug, PartialEq))]
pub struct Slave {
    pub on: bool,
    pub scl: u8,
    pub a10: Option<u16>,
    pub ptr: Option<u8>,
    pub filt: u8,
}

/// Every kind of variable part, bits after them
#[derive_shrink_wrap(head_tail, derive(Clone, Copy, Debug, PartialEq))]
pub struct Utx<'i> {
    pub ch: u8,
    pub glitch: Option<Wave>,
    pub perr: List<'i, u16>,
    pub s: Slave,
    pub name: &'i str,
    pub defer: bool,
}

#[derive_shrink_wrap(head_tail, ww_repr = u8, derive(Clone, Copy, Debug, PartialEq))]
pub enum Request<'i> {
    Hello,
    Stop { mask: u16 },
    Waves { waves: List<'i, Wave>, defer: bool },
    Utx(Utx<'i>),
    Res { r: Result<u32, u8>, tag: [u8; 2] },
}

const WAVE: Wave = Wave {
    line: 1,
    idle: true,
    hold: false,
    delay: 0x0102_0304,
};

fn utx() -> Utx<'static> {
    Utx {
        ch: 7,
        glitch: Some(WAVE),
        perr: List::new(&[0x0a0b]),
        s: Slave {
            on: true,
            scl: 2,
            a10: Some(0x0304),
            ptr: None,
            filt: 9,
        },
        name: "hi",
        defer: true,
    }
}

const UTX_BYTES: [u8; 23] = hex!(
    "07 80 0100 80 02 80 09 0200 80" // head: ch, glitch's flag, perr's count, s's head, name's count, defer
    "01 80 04030201" // glitch
    "0b0a" // perr
    "0403" // s's tail: a10
    "6869" // name
);

#[test]
fn offsets() {
    assert_eq!(Slave::HEAD_LEN, 4);
    assert_eq!(Slave::ON_BIT_OFFSET, 0);
    assert_eq!(Slave::SCL_OFFSET, 1);
    assert_eq!(Slave::A10_FLAG_BIT_OFFSET, 16);
    assert_eq!(Slave::PTR_FLAG_BIT_OFFSET, 17);
    assert_eq!(Slave::FILT_OFFSET, 3);
    assert_eq!(Utx::GLITCH_FLAG_BIT_OFFSET, 8);
    assert_eq!(Utx::PERR_OFFSET, 2);
    assert_eq!(Utx::S_OFFSET, 4);
    assert_eq!(Utx::NAME_OFFSET, 8);
    assert_eq!(Utx::DEFER_BIT_OFFSET, 80);
    assert_eq!(Utx::HEAD_LEN, 11);
    assert_eq!(<Wave as HeadTail>::HEAD_LEN, 6);
    assert_eq!(Request::HEAD_LEN, 1);
    assert_eq!(Request::WAVES_HEAD_LEN, 3);
    assert_eq!(Request::UTX_HEAD_LEN, 11);
    assert_eq!(Request::RES_R_FLAG_BIT_OFFSET, 0);
    assert_eq!(Request::RES_TAG_OFFSET, 1);
}

#[test]
fn exact_bytes() {
    let v = utx();
    let mut buf = [0u8; 64];
    let n = v.to_head_tail(&mut buf).unwrap();
    assert_eq!(&buf[..n], &UTX_BYTES);
    assert_eq!(v.head_tail_len(), n);
    let back = Utx::from_head_tail(&UTX_BYTES).unwrap();
    assert_eq!(back, v);
    assert_eq!(
        back.perr.iter().map(Result::unwrap).collect::<Vec<_>>(),
        [0x0a0b]
    );

    // an enum: the tag, then the variant's head and tail
    let req = Request::Utx(v);
    let n = req.to_head_tail(&mut buf).unwrap();
    assert_eq!(buf[0], Request::UTX_TAG);
    assert_eq!(&buf[1..n], &UTX_BYTES);
}

fn requests() -> Vec<Request<'static>> {
    static WAVES: [Wave; 2] = [
        WAVE,
        Wave {
            line: 3,
            idle: false,
            hold: true,
            delay: 5,
        },
    ];
    let mut other = utx();
    other.glitch = None;
    other.perr = List::new(&[]);
    other.s.ptr = Some(4);
    other.s.a10 = None;
    other.name = "";
    vec![
        Request::Hello,
        Request::Stop { mask: 0xbeef },
        Request::Waves {
            waves: List::new(&WAVES),
            defer: true,
        },
        Request::Waves {
            waves: List::new(&[]),
            defer: false,
        },
        Request::Utx(utx()),
        Request::Utx(other),
        Request::Res {
            r: Ok(0x1234_5678),
            tag: [1, 2],
        },
        Request::Res {
            r: Err(9),
            tag: [3, 4],
        },
    ]
}

#[test]
fn round_trips() {
    for req in requests() {
        let mut buf = [0u8; 128];
        let n = req.to_head_tail(&mut buf).unwrap();
        assert_eq!(n, req.head_tail_len(), "{req:?}");
        let back = Request::from_head_tail(&buf[..n]).unwrap();
        assert_eq!(back, req);
        // what was read writes the same bytes
        let mut again = [0u8; 128];
        assert_eq!(back.to_head_tail(&mut again).unwrap(), n);
        assert_eq!(&again[..n], &buf[..n]);
    }
}

#[test]
fn short_buffers_are_errors() {
    for req in requests() {
        let mut buf = [0u8; 128];
        let n = req.to_head_tail(&mut buf).unwrap();
        for short in 0..n {
            assert!(
                Request::from_head_tail(&buf[..short]).is_err(),
                "{req:?} read from {short} bytes"
            );
            let mut small = vec![0u8; short];
            assert!(
                req.to_head_tail(&mut small).is_err(),
                "{req:?} written into {short} bytes"
            );
        }
    }
}

#[test]
fn garbage_never_panics() {
    let mut x: u64 = 0x2545_f491_4f6c_dd1d;
    let mut next = || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    let mut ok = 0;
    for _ in 0..200_000 {
        let len = (next() % 48) as usize;
        let mut buf = [0u8; 48];
        for b in &mut buf[..len] {
            *b = next() as u8;
        }
        // small tags and counts, so that some of it reads
        buf[0] %= 6;
        if let Ok(req) = Request::from_head_tail(&buf[..len]) {
            ok += 1;
            if let Request::Waves { waves, .. } = &req {
                let _ = waves_of(waves);
            }
            let mut out = [0u8; 64];
            let n = req.to_head_tail(&mut out).unwrap();
            assert_eq!(Request::from_head_tail(&out[..n]).unwrap(), req);
        }
    }
    assert!(ok > 1000, "only {ok} of the random messages read");
}

fn waves_of(waves: &List<Wave>) -> usize {
    waves.iter().filter(Result::is_ok).count()
}

#[test]
fn long_list_is_an_error() {
    let frames = vec![0u16; 70_000];
    let v = Utx {
        perr: List::new(&frames),
        ..utx()
    };
    let mut buf = vec![0u8; 200_000];
    assert_eq!(v.to_head_tail(&mut buf), Err(ShrinkWrapError::LenTooLong));
}

#[test]
fn bad_utf8_is_an_error() {
    let mut bytes = UTX_BYTES;
    bytes[21] = 0xff;
    assert_eq!(
        Utx::from_head_tail(&bytes),
        Err(ShrinkWrapError::MalformedUtf8)
    );
}

/// The generated C reader, checked in and compiled by `just lint-c` with every warning an error.
#[test]
fn c_header() {
    let mut out = String::new();
    write_c_header(
        &mut out,
        "HEAD_TAIL_H",
        "t_",
        &[
            <Wave as HeadTail>::write_c_ht,
            <Slave as HeadTail>::write_c_ht,
            <Utx as HeadTail>::write_c_ht,
            <Request as HeadTail>::write_c_ht,
        ],
    )
    .unwrap();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/head_tail.h");
    if std::env::var_os("SW_UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, &out).unwrap();
    }
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        out,
        "tests/head_tail.h is not what the types produce: SW_UPDATE_GOLDEN=1 cargo test -p shrink_wrap --test head_tail"
    );
}
