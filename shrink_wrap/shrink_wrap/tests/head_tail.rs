//! The head+tail encoding (SW-46): offsets, exact bytes, round trips, garbage and short buffers, the C reader.

use hex_literal::hex;
use shrink_wrap::head_tail::{HeadTail, List, write_c_header};
use shrink_wrap::prelude::*;

mod c_run;

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

/// A fixed-layout enum of one length: a nested head and list items whose tag may be unknown.
#[derive_shrink_wrap(
    borrowed,
    sized,
    fixed_layout,
    ww_repr = u8,
    derive(Clone, Copy, Debug, PartialEq)
)]
pub enum Pin {
    Low(u8) = 1,
    High(u8) = 2,
}

/// The other variable parts: a head_tail payload, a Result of a fixed-layout type, a list of a nested fixed-layout
/// type and one of bytes, an array payload
#[derive_shrink_wrap(head_tail, derive(Clone, Copy, Debug, PartialEq))]
pub struct Mixed<'i> {
    pub pin: Pin,
    pub slave: Option<Slave>,
    pub r: Result<Wave, u16>,
    pub pins: List<'i, Pin>,
    pub raw: List<'i, u8>,
    pub words: Option<[u16; 2]>,
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
        let view = Request::view(&buf[..len]);
        assert_eq!(view.is_ok(), Request::from_head_tail(&buf[..len]).is_ok());
        if let Ok(req) = Request::from_head_tail(&buf[..len]) {
            assert_eq!(from_view(&view.unwrap()), req);
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

/// A value back from its view (SW-52), through every accessor
fn from_view<'i>(v: &RequestView<'i>) -> Request<'i> {
    let slave = |s: &SlaveView| Slave {
        on: s.on(),
        scl: s.scl(),
        a10: s.a10(),
        ptr: s.ptr(),
        filt: s.filt(),
    };
    match v {
        RequestView::Hello => Request::Hello,
        RequestView::Stop(v) => Request::Stop { mask: v.mask() },
        RequestView::Waves(v) => Request::Waves {
            waves: v.waves(),
            defer: v.defer(),
        },
        RequestView::Utx(u) => Request::Utx(Utx {
            ch: u.ch(),
            glitch: *u.glitch(),
            perr: u.perr(),
            s: slave(u.s()),
            name: u.name(),
            defer: u.defer(),
        }),
        RequestView::Res(v) => Request::Res {
            r: v.r(),
            tag: v.tag(),
        },
    }
}

/// Writes a request through the writer of its variant (SW-52)
fn write_direct(req: &Request, buf: &mut [u8]) -> Result<usize, ShrinkWrapError> {
    match *req {
        Request::Hello => Request::write_hello(buf),
        Request::Stop { mask } => Request::write_stop(buf, mask),
        Request::Waves { waves, defer } => Request::write_waves(buf, waves, defer),
        Request::Utx(u) => Request::write_utx(buf, u),
        Request::Res { r, tag } => Request::write_res(buf, r, tag),
    }
}

#[test]
fn views_and_writers() {
    for req in requests() {
        let mut buf = [0u8; 128];
        let n = req.to_head_tail(&mut buf).unwrap();
        let view = Request::view(&buf[..n]).unwrap();
        assert_eq!(from_view(&view), req);
        let mut direct = [0u8; 128];
        assert_eq!(write_direct(&req, &mut direct), Ok(n), "{req:?}");
        assert_eq!(&direct[..n], &buf[..n], "{req:?}");
        for short in 0..n {
            assert!(
                Request::view(&buf[..short]).is_err(),
                "{req:?} viewed in {short} bytes"
            );
            let mut small = vec![0u8; short];
            assert!(
                write_direct(&req, &mut small).is_err(),
                "{req:?} written into {short} bytes"
            );
        }
    }
    // a struct of its own
    let view = Utx::view(&UTX_BYTES).unwrap();
    assert_eq!(
        (view.ch(), view.name(), view.s().a10(), view.defer()),
        (7, "hi", Some(0x0304), true)
    );
    // bad UTF-8 is found when viewing, as when reading
    let mut bytes = UTX_BYTES;
    bytes[21] = 0xff;
    assert!(Utx::view(&bytes).is_err());
    // a variant behind a #[cfg]
    let mut buf = [0u8; 32];
    let n = Gated::write_kept(&mut buf, 7, List::new(&[WAVE])).unwrap();
    let Ok(GatedView::Kept(k)) = Gated::view(&buf[..n]) else {
        panic!()
    };
    assert_eq!((k.mask(), k.w().get(0).unwrap().unwrap()), (7, WAVE));
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
            <Pin as HeadTail>::write_c_ht,
            <Mixed as HeadTail>::write_c_ht,
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

/// Variants behind a `#[cfg]` (SW-54): one compiled out, one in. Tags stay where they are.
#[derive_shrink_wrap(head_tail, ww_repr = u8, derive(Clone, Copy, Debug, PartialEq))]
pub enum Gated<'i> {
    First,
    #[cfg(any())]
    Gone {
        x: u32,
        w: List<'i, Wave>,
    },
    #[cfg(test)]
    Kept {
        mask: u16,
        w: List<'i, Wave>,
    },
}

#[test]
fn cfg_variants() {
    assert_eq!(Gated::KEPT_TAG, 2);
    let w = [WAVE];
    let v = Gated::Kept {
        mask: 7,
        w: List::new(&w),
    };
    let mut buf = [0u8; 32];
    let n = v.to_head_tail(&mut buf).unwrap();
    assert_eq!(buf[0], 2);
    assert_eq!(Gated::from_head_tail(&buf[..n]).unwrap(), v);
    // the tag of the variant this build has no code for: unknown, as from a newer version
    assert!(Gated::from_head_tail(&[1, 0, 0, 0, 0, 0, 0]).is_err());
    let mut c = String::new();
    write_c_header(
        &mut c,
        "GATED_H",
        "",
        &[Wave::write_c_ht, Gated::write_c_ht],
    )
    .unwrap();
    assert!(c.contains("GATED_KEPT 2u") && !c.contains("GONE"), "{c}");
}

/// Valid encodings of every type with a C reader, by its C name.
fn encodings(rng: &mut c_run::Rng) -> Vec<(&'static str, Vec<u8>)> {
    fn enc<'i, T: HeadTail<'i>>(v: &T) -> Vec<u8> {
        let mut buf = [0u8; 128];
        let n = shrink_wrap::head_tail::write(v, &mut buf).unwrap();
        buf[..n].to_vec()
    }
    static PINS: [Pin; 3] = [Pin::Low(1), Pin::High(2), Pin::Low(3)];
    static RAW: [u8; 3] = [7, 8, 9];
    let mut out: Vec<(&'static str, Vec<u8>)> =
        requests().iter().map(|r| ("request", enc(r))).collect();
    out.push(("utx", enc(&utx())));
    for _ in 0..20 {
        let mut r = || rng.next();
        let some = r() & 1 == 1;
        let slave = Slave {
            on: some,
            scl: r() as u8,
            a10: (r() & 1 == 1).then_some(r() as u16),
            ptr: (r() & 1 == 1).then_some(r() as u8),
            filt: r() as u8,
        };
        let wave = Wave {
            line: r() as u8,
            idle: some,
            hold: r() & 1 == 1,
            delay: r() as u32,
        };
        let pin = if some {
            Pin::Low(r() as u8)
        } else {
            Pin::High(r() as u8)
        };
        let mixed = Mixed {
            pin,
            slave: some.then_some(slave),
            r: if r() & 1 == 1 {
                Ok(wave)
            } else {
                Err(r() as u16)
            },
            pins: List::new(&PINS[..r() as usize % 4]),
            raw: List::new(&RAW[..r() as usize % 4]),
            words: (r() & 1 == 1).then_some([r() as u16, r() as u16]),
        };
        out.extend([
            ("wave", enc(&wave)),
            ("slave", enc(&slave)),
            ("pin", enc(&pin)),
            ("mixed", enc(&mixed)),
        ]);
    }
    out
}

/// What `from_head_tail` makes of `buf` as the type of this C name: the C program's answer for it. A string that
/// is not UTF-8 is an error in Rust and bytes in C: `None` then, any answer is right.
fn rust_answer(ty: &str, buf: &[u8]) -> Option<String> {
    fn len<'i, T: HeadTail<'i>>(buf: &'i [u8]) -> Result<usize, ShrinkWrapError> {
        let v: T = shrink_wrap::head_tail::read(buf)?;
        Ok(T::HEAD_LEN + v.tail_len())
    }
    let got = match ty {
        "wave" => len::<Wave>(buf),
        "slave" => len::<Slave>(buf),
        "utx" => len::<Utx>(buf),
        "request" => len::<Request>(buf),
        "pin" => len::<Pin>(buf),
        "mixed" => len::<Mixed>(buf),
        other => panic!("no type {other}"),
    };
    match got {
        Err(ShrinkWrapError::MalformedUtf8) => None,
        got => Some(c_run::expect(got.ok())),
    }
}

/// The C views (SW-79) on valid, truncated, empty, changed and random buffers of every type: a view is made
/// exactly when Rust reads a value, with its length, and every reader of it stays in its bytes (the C program
/// reads each case from a buffer of exactly its length, under AddressSanitizer where it links).
#[test]
fn c_views_check_bounds() {
    let mut rng = c_run::Rng(0x9e37_79b9_7f4a_7c15);
    let valid = encodings(&mut rng);
    let mut input = String::new();
    let mut want = Vec::new();
    for name in ["wave", "slave", "utx", "request", "pin", "mixed"] {
        let of: Vec<Vec<u8>> = valid
            .iter()
            .filter(|(n, _)| *n == name)
            .map(|(_, b)| b.clone())
            .collect();
        for (ty, bytes) in c_run::cases(name, &of, 40, &mut rng) {
            input.push_str(&c_run::line(ty, &bytes));
            want.push(rust_answer(ty, &bytes).map(|a| format!("{ty} {a}")));
        }
    }
    let exe = c_run::build("head_tail_c.c");
    let got = c_run::run(&exe, &input);
    let got: Vec<&str> = got.lines().collect();
    assert_eq!(got.len(), want.len());
    let mut checked = 0;
    for ((got, want), case) in got.iter().zip(&want).zip(input.lines()) {
        if let Some(want) = want {
            assert_eq!(got, want, "case {case}");
            checked += 1;
        }
    }
    assert!(
        checked * 10 > want.len() * 9,
        "only {checked} of {} cases checked",
        want.len()
    );
}
