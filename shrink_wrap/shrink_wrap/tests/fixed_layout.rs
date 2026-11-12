//! The fixed-layout fast path (SW-44, SW-45): `from_fixed` / `to_fixed` read and write the bytes `des_shrink_wrap` /
//! `ser_shrink_wrap` do, at the offsets the consts say: byte fields, bit fields, a variable last field.

use hex_literal::hex;
use shrink_wrap::fixed_layout::{FixedLayout, write_c_header};
use shrink_wrap::prelude::*;

mod c_run;

const N: usize = 3;

/// Every number type and both kinds of array; detected (no `fixed_layout`), it is `sized`.
#[derive_shrink_wrap(borrowed, sized, derive(Clone, Copy, Debug, PartialEq))]
struct Numbers {
    a: u8,
    b: u16,
    c: u32,
    d: u64,
    e: i8,
    f: i16,
    g: i32,
    h: i64,
    x: f32,
    y: f64,
    bytes: [u8; 5],
    words: [u16; N],
}

#[derive_shrink_wrap(borrowed, fixed_layout, derive(Clone, Copy, Debug, PartialEq))]
struct Inner {
    id: u32,
    hash: [u8; 4],
}

/// Nested types need the directive: the macro cannot see inside them.
#[derive_shrink_wrap(borrowed, fixed_layout, derive(Clone, Copy, Debug, PartialEq))]
struct Outer {
    tag: u8,
    inner: Inner,
    signed: [i16; 2],
    tail: u16,
}

#[derive_shrink_wrap(
    borrowed,
    final_structure,
    fixed_layout,
    derive(Clone, Copy, Debug, PartialEq)
)]
struct Final {
    a: u16,
    b: Inner,
}

#[derive_shrink_wrap(
    borrowed,
    self_describing,
    fixed_layout,
    ww_repr = u8,
    derive(Clone, Copy, Debug, PartialEq)
)]
enum Op {
    Stop = 1,
    Erase { block: u16 } = 4,
    Copy { dst: u16, src: u16, hash: [u8; 8] },
    Inner(Inner),
}

#[derive_shrink_wrap(borrowed, sized, ww_repr = u16, derive(Clone, Copy, Debug, PartialEq))]
enum Wide {
    A(u32),
    B(i32),
}

/// Not fixed: a varint. No fast path is generated, nothing breaks.
#[derive_shrink_wrap(borrowed, self_describing, derive(Clone, Copy, Debug, PartialEq))]
struct WithVarint {
    a: u8,
    n: UNib32,
}

trait NoFastPath {
    const ENCODED_LEN: usize = 0;
}
impl NoFastPath for WithVarint {}

/// Bit runs: a nibble realigning after one bit, a signed field across a byte boundary, a byte field starting a
/// fresh byte, wide fields, and a run at the end. Detected: no directive.
#[derive_shrink_wrap(borrowed, sized, derive(Clone, Copy, Debug, PartialEq))]
struct Bits {
    a: bool,
    n: Nibble,
    b: U3,
    s: I5,
    x: u16,
    wide: U20,
    i: I12,
    last: bool,
    end: U2,
}

/// A byte field after a nested type that ends with bits: fine, it starts a fresh byte.
#[derive_shrink_wrap(borrowed, fixed_layout, derive(Clone, Copy, Debug, PartialEq))]
struct EndsWithBits {
    id: u8,
    on: bool,
}

#[derive_shrink_wrap(borrowed, fixed_layout, derive(Clone, Copy, Debug, PartialEq))]
struct Wrap {
    lead: bool,
    inner: Inner,
    flags: EndsWithBits,
    after: u8,
}

/// An Option as the last field, its flag where shrink_wrap puts it (just before), `self_describing` implied.
#[derive_shrink_wrap(borrowed, fixed_layout, derive(Clone, Copy, Debug, PartialEq))]
struct Maybe {
    id: u16,
    on: bool,
    extra: Option<Inner>,
}

/// The flag moved with `#[flag]`, into a run of other bits.
#[derive_shrink_wrap(borrowed, fixed_layout, derive(Clone, Copy, Debug, PartialEq))]
struct Relocated {
    #[flag]
    value: bool,
    kind: U3,
    id: u8,
    value: Option<[u16; 2]>,
}

#[derive_shrink_wrap(borrowed, fixed_layout, derive(Clone, Copy, Debug, PartialEq))]
struct Res {
    a: u8,
    r: Result<u32, Inner>,
}

/// A variable-length enum as the last field.
#[derive_shrink_wrap(
    borrowed,
    self_describing,
    fixed_layout,
    derive(Clone, Copy, Debug, PartialEq)
)]
struct TailOp {
    seq: U4,
    op: Op,
}

/// Bits and an Option inside enum variants.
#[derive_shrink_wrap(
    borrowed,
    self_describing,
    fixed_layout,
    ww_repr = u8,
    derive(Clone, Copy, Debug, PartialEq)
)]
enum Msg {
    Ping,
    Data {
        urgent: bool,
        prio: U2,
        seq: u16,
        payload: Option<[u8; 4]>,
    },
}

/// Same bytes both ways, both read back to the value.
fn same<T>(value: &T) -> Vec<u8>
where
    T: SerializeShrinkWrap
        + for<'i> DeserializeShrinkWrap<'i>
        + FixedLayout
        + PartialEq
        + core::fmt::Debug,
{
    let mut sw = [0u8; 256];
    let sw = value.to_ww_bytes(&mut sw).unwrap().to_vec();
    let mut fixed = [0u8; 256];
    let len = value.write_fixed(&mut fixed).unwrap();
    assert_eq!(&fixed[..len], &sw[..], "{value:?}");
    assert_eq!(len, value.fixed_len());
    assert_eq!(&T::read_fixed(&sw).unwrap(), value);
    assert_eq!(&T::from_ww_bytes(&fixed[..len]).unwrap(), value);
    // too short: an error, never a panic
    for cut in 0..len {
        assert!(T::read_fixed(&sw[..cut]).is_err());
        assert!(value.write_fixed(&mut fixed[..cut]).is_err());
    }
    sw
}

/// xorshift, so the values cover every byte without a dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

#[test]
fn offsets() {
    assert_eq!(Numbers::A_OFFSET, 0);
    assert_eq!(Numbers::B_OFFSET, 1);
    assert_eq!(Numbers::C_OFFSET, 3);
    assert_eq!(Numbers::D_OFFSET, 7);
    assert_eq!(Numbers::E_OFFSET, 15);
    assert_eq!(Numbers::X_OFFSET, 30);
    assert_eq!(Numbers::Y_OFFSET, 34);
    assert_eq!(Numbers::BYTES_OFFSET, 42);
    assert_eq!(Numbers::WORDS_OFFSET, 47);
    assert_eq!(Numbers::ENCODED_LEN, 53);
    assert_eq!(Outer::INNER_OFFSET, 1);
    assert_eq!(Outer::SIGNED_OFFSET, 9);
    assert_eq!(Outer::ENCODED_LEN, 15);
    assert_eq!(Op::TAG_LEN, 1);
    assert_eq!(Op::STOP_TAG, 1);
    assert_eq!(Op::ERASE_TAG, 4);
    assert_eq!(Op::COPY_TAG, 5);
    assert_eq!(Op::STOP_LEN, 1);
    assert_eq!(Op::ERASE_LEN, 3);
    assert_eq!(Op::COPY_SRC_OFFSET, 3);
    assert_eq!(Op::COPY_LEN, 13);
    assert_eq!(Op::INNER_0_OFFSET, 1);
    assert_eq!(Op::ENCODED_LEN, 13);
    const { assert!(!Op::SAME_LEN) };
    const { assert!(Wide::SAME_LEN) };
    assert_eq!(Wide::ENCODED_LEN, 6);
    assert_eq!(<WithVarint as NoFastPath>::ENCODED_LEN, 0);
    // bits: a at 0, the nibble rounded up to 4, b and s after it, x on byte 2, then a run of 20 + 12 + 1 + 2
    assert_eq!(Bits::A_BIT_OFFSET, 0);
    assert_eq!(Bits::N_BIT_OFFSET, 4);
    assert_eq!(Bits::B_BIT_OFFSET, 8);
    assert_eq!(Bits::S_BIT_OFFSET, 11);
    assert_eq!(Bits::S_BITS, 5);
    assert_eq!(Bits::X_OFFSET, 2);
    assert_eq!(Bits::WIDE_BIT_OFFSET, 32);
    assert_eq!(Bits::I_BIT_OFFSET, 52);
    assert_eq!(Bits::END_BIT_OFFSET, 65);
    assert_eq!(Bits::ENCODED_LEN, 9);
    assert_eq!(Wrap::INNER_OFFSET, 1);
    assert_eq!(Wrap::FLAGS_OFFSET, 9);
    assert_eq!(Wrap::AFTER_OFFSET, 11);
    assert_eq!(Maybe::EXTRA_FLAG_BIT_OFFSET, 17);
    assert_eq!(Maybe::EXTRA_OFFSET, 3);
    assert_eq!((Maybe::MIN_LEN, Maybe::ENCODED_LEN), (3, 11));
    assert_eq!(Relocated::VALUE_FLAG_BIT_OFFSET, 0);
    assert_eq!(Relocated::KIND_BIT_OFFSET, 1);
    assert_eq!(Relocated::ID_OFFSET, 1);
    assert_eq!(Relocated::VALUE_OFFSET, 2);
    assert_eq!((Res::MIN_LEN, Res::ENCODED_LEN), (6, 10));
    assert_eq!(TailOp::OP_OFFSET, 1);
    assert_eq!((TailOp::MIN_LEN, TailOp::ENCODED_LEN), (2, 14));
    assert_eq!(Msg::DATA_URGENT_BIT_OFFSET, 8);
    assert_eq!(Msg::DATA_SEQ_OFFSET, 2);
    assert_eq!(Msg::DATA_PAYLOAD_FLAG_BIT_OFFSET, 32);
    assert_eq!((Msg::MIN_LEN, Msg::ENCODED_LEN), (1, 9));
    const { assert!(!Maybe::SAME_LEN) };
    const { assert!(<EndsWithBits as FixedLayout>::TRAILS_WITH_BITS) };
    const { assert!(<Wrap as FixedLayout>::LEADS_WITH_BITS) };
}

#[test]
fn known_bytes() {
    let outer = Outer {
        tag: 0xaa,
        inner: Inner {
            id: 0x0403_0201,
            hash: hex!("10 11 12 13"),
        },
        signed: [-1, 2],
        tail: 0x0605,
    };
    assert_eq!(same(&outer), hex!("aa 01020304 10111213 ffff 0200 0506"));
    let op = Op::Copy {
        dst: 0x0102,
        src: 0x0304,
        hash: [7; 8],
    };
    assert_eq!(same(&op), hex!("05 0201 0403 0707070707070707"));
    assert_eq!(same(&Op::Stop), hex!("01"));
    assert_eq!(same(&Op::Erase { block: 9 }), hex!("04 0900"));
    assert_eq!(same(&Wide::B(-2)), hex!("0100 feffffff"));
}

#[test]
fn random_values() {
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    for _ in 0..2000 {
        let mut r = || rng.next();
        let numbers = Numbers {
            a: r() as u8,
            b: r() as u16,
            c: r() as u32,
            d: r(),
            e: r() as i8,
            f: r() as i16,
            g: r() as i32,
            h: r() as i64,
            // finite floats, so that PartialEq holds
            x: (r() as u16) as f32 / 7.0,
            y: (r() as u32) as f64 / 3.0,
            bytes: r().to_le_bytes()[..5].try_into().unwrap(),
            words: [r() as u16, r() as u16, r() as u16],
        };
        same(&numbers);
        let inner = Inner {
            id: r() as u32,
            hash: (r() as u32).to_le_bytes(),
        };
        same(&Outer {
            tag: r() as u8,
            inner,
            signed: [r() as i16, r() as i16],
            tail: r() as u16,
        });
        same(&Final {
            a: r() as u16,
            b: inner,
        });
        same(&Op::Inner(inner));
        same(&Op::Copy {
            dst: r() as u16,
            src: r() as u16,
            hash: r().to_le_bytes(),
        });
        same(&Wide::A(r() as u32));
        let bits = Bits {
            a: r() & 1 == 1,
            n: Nibble::new_masked(r() as u8),
            b: U3::__from_wire(r()),
            s: I5::__from_wire(r()),
            x: r() as u16,
            wide: U20::__from_wire(r()),
            i: I12::__from_wire(r()),
            last: r() & 1 == 1,
            end: U2::__from_wire(r()),
        };
        same(&bits);
        let flags = EndsWithBits {
            id: r() as u8,
            on: r() & 1 == 1,
        };
        same(&Wrap {
            lead: r() & 1 == 1,
            inner,
            flags,
            after: r() as u8,
        });
        let some = r() & 1 == 1;
        same(&Maybe {
            id: r() as u16,
            on: r() & 1 == 1,
            extra: some.then_some(inner),
        });
        same(&Relocated {
            kind: U3::__from_wire(r()),
            id: r() as u8,
            value: some.then_some([r() as u16, r() as u16]),
        });
        same(&Res {
            a: r() as u8,
            r: if some { Ok(r() as u32) } else { Err(inner) },
        });
        let op = if some {
            Op::Stop
        } else {
            Op::Copy {
                dst: r() as u16,
                src: r() as u16,
                hash: r().to_le_bytes(),
            }
        };
        same(&TailOp {
            seq: U4::__from_wire(r()),
            op,
        });
        same(&Msg::Data {
            urgent: r() & 1 == 1,
            prio: U2::__from_wire(r()),
            seq: r() as u16,
            payload: some.then_some((r() as u32).to_le_bytes()),
        });
        same(&Msg::Ping);
    }
}

#[test]
fn unknown_tag() {
    for tag in [0u8, 2, 3, 7, 0xff] {
        assert_eq!(
            Op::from_fixed(&[tag; 16]),
            Err(ShrinkWrapError::EnumFutureVersionOrMalformedData)
        );
    }
}

#[test]
fn known_bits() {
    let bits = Bits {
        a: true,
        n: Nibble::new_masked(0xa),
        b: U3::__from_wire(0b101),
        s: I5::__from_wire((-3i32) as u64),
        x: 0x1234,
        wide: U20::__from_wire(0xfedcb),
        i: I12::__from_wire((-2i32) as u64),
        last: true,
        end: U2::__from_wire(0b10),
    };
    // a, 3 zero bits, the nibble | b, s (11101) | x LE | wide, i, last, end, zero padding
    assert_eq!(same(&bits), hex!("8a bd 3412 fedcb ffe c0").to_vec());
}

/// Every type of this file with a C reader, in the order the header needs them.
const C_TYPES: &[fn(&str, &mut dyn core::fmt::Write) -> core::fmt::Result] = &[
    Numbers::write_c,
    Inner::write_c,
    Outer::write_c,
    Final::write_c,
    Op::write_c,
    Wide::write_c,
    Bits::write_c,
    EndsWithBits::write_c,
    Wrap::write_c,
    Maybe::write_c,
    Relocated::write_c,
    Res::write_c,
    TailOp::write_c,
    Msg::write_c,
];

/// The generated C reader, checked in and compiled by `just lint-c` with every warning an error.
#[test]
fn c_header() {
    let mut out = String::new();
    write_c_header(&mut out, "TEST_H", "t_", C_TYPES).unwrap();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixed_layout.h");
    if std::env::var_os("SW_UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, &out).unwrap();
    }
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        out,
        "tests/fixed_layout.h is not what the types produce: SW_UPDATE_GOLDEN=1 cargo test -p shrink_wrap --test fixed_layout"
    );
    for line in [
        "#define T_OUTER_LEN 15u",
        "#define T_OUTER_SIGNED_OFFSET 9u",
        "#define T_OUTER_SIGNED_COUNT 2u",
        "static inline uint32_t t_inner_id(t_inner_view_t v) { if (v.p == NULL) { return 0; } return sw_fixed_u32(v.p + 0u); }",
        "SW_NODISCARD static inline int t_outer_view(const uint8_t *buf, size_t len, t_outer_view_t *v) {",
        "SW_NODISCARD static inline int t_op_as_copy(t_op_view_t v, t_op_copy_view_t *out) {",
        "static inline uint16_t t_op_copy_src(t_op_copy_view_t v) { if (v.p == NULL) { return 0; } return sw_fixed_u16(v.p + 3u); }",
        "static inline int32_t t_bits_s(t_bits_view_t v) { if (v.p == NULL) { return 0; } return sw_fixed_sbits(v.p, 11u, 5u); }",
        "SW_NODISCARD static inline int t_maybe_extra(t_maybe_view_t v, t_inner_view_t *out) {",
    ] {
        assert!(out.contains(line), "missing {line:?} in\n{out}");
    }
}

/// Byte-tagged and sized, so it would qualify, but a `#[future_version]` variant (SW-16) has a discriminant of a
/// newer version and no known length: no fast path, the usual serdes as before.
#[derive_shrink_wrap(borrowed, sized, ww_repr = u8, derive(Debug, PartialEq))]
enum Later {
    A,
    B,
    #[future_version]
    Unknown(u32),
}

#[test]
fn future_version_has_no_fast_path() {
    let mut buf = [0u8; 8];
    let bytes = Later::B.to_ww_bytes(&mut buf).unwrap();
    assert_eq!(Later::from_ww_bytes(bytes).unwrap(), Later::B);
    assert_eq!(Later::from_ww_bytes(&[7]).unwrap(), Later::Unknown(7));
}

/// Valid encodings of random values of every type with a C reader, by its C name.
fn encodings(rng: &mut c_run::Rng, out: &mut Vec<(&'static str, Vec<u8>)>) {
    fn enc<T: FixedLayout>(v: T) -> Vec<u8> {
        let mut buf = [0u8; 64];
        let n = v.write_fixed(&mut buf).unwrap();
        buf[..n].to_vec()
    }
    let mut r = || rng.next();
    let inner = Inner {
        id: r() as u32,
        hash: (r() as u32).to_le_bytes(),
    };
    let some = r() & 1 == 1;
    let op = match r() % 4 {
        0 => Op::Stop,
        1 => Op::Erase { block: r() as u16 },
        2 => Op::Copy {
            dst: r() as u16,
            src: r() as u16,
            hash: r().to_le_bytes(),
        },
        _ => Op::Inner(inner),
    };
    out.extend([
        (
            "numbers",
            enc(Numbers {
                a: r() as u8,
                b: r() as u16,
                c: r() as u32,
                d: r(),
                e: r() as i8,
                f: r() as i16,
                g: r() as i32,
                h: r() as i64,
                x: f32::from_bits(r() as u32),
                y: f64::from_bits(r()),
                bytes: r().to_le_bytes()[..5].try_into().unwrap(),
                words: [r() as u16, r() as u16, r() as u16],
            }),
        ),
        ("inner", enc(inner)),
        (
            "outer",
            enc(Outer {
                tag: r() as u8,
                inner,
                signed: [r() as i16, r() as i16],
                tail: r() as u16,
            }),
        ),
        (
            "final",
            enc(Final {
                a: r() as u16,
                b: inner,
            }),
        ),
        ("op", enc(op)),
        (
            "wide",
            enc(if some {
                Wide::A(r() as u32)
            } else {
                Wide::B(r() as i32)
            }),
        ),
        (
            "bits",
            enc(Bits {
                a: some,
                n: Nibble::new_masked(r() as u8),
                b: U3::__from_wire(r()),
                s: I5::__from_wire(r()),
                x: r() as u16,
                wide: U20::__from_wire(r()),
                i: I12::__from_wire(r()),
                last: r() & 1 == 1,
                end: U2::__from_wire(r()),
            }),
        ),
        (
            "ends_with_bits",
            enc(EndsWithBits {
                id: r() as u8,
                on: some,
            }),
        ),
        (
            "wrap",
            enc(Wrap {
                lead: some,
                inner,
                flags: EndsWithBits {
                    id: r() as u8,
                    on: r() & 1 == 1,
                },
                after: r() as u8,
            }),
        ),
        (
            "maybe",
            enc(Maybe {
                id: r() as u16,
                on: r() & 1 == 1,
                extra: some.then_some(inner),
            }),
        ),
        (
            "relocated",
            enc(Relocated {
                kind: U3::__from_wire(r()),
                id: r() as u8,
                value: some.then_some([r() as u16, r() as u16]),
            }),
        ),
        (
            "res",
            enc(Res {
                a: r() as u8,
                r: if some { Ok(r() as u32) } else { Err(inner) },
            }),
        ),
        (
            "tail_op",
            enc(TailOp {
                seq: U4::__from_wire(r()),
                op,
            }),
        ),
        (
            "msg",
            enc(if r() & 1 == 1 {
                Msg::Ping
            } else {
                Msg::Data {
                    urgent: r() & 1 == 1,
                    prio: U2::__from_wire(r()),
                    seq: r() as u16,
                    payload: some.then_some((r() as u32).to_le_bytes()),
                }
            }),
        ),
    ]);
}

/// What `from_fixed` makes of `buf` as the type of this C name: the length of the value, `None` refused.
fn rust_len(ty: &str, buf: &[u8]) -> Option<usize> {
    fn len<T: FixedLayout>(buf: &[u8]) -> Option<usize> {
        T::read_fixed(buf).ok().map(|v| v.fixed_len())
    }
    match ty {
        "numbers" => len::<Numbers>(buf),
        "inner" => len::<Inner>(buf),
        "outer" => len::<Outer>(buf),
        "final" => len::<Final>(buf),
        "op" => len::<Op>(buf),
        "wide" => len::<Wide>(buf),
        "bits" => len::<Bits>(buf),
        "ends_with_bits" => len::<EndsWithBits>(buf),
        "wrap" => len::<Wrap>(buf),
        "maybe" => len::<Maybe>(buf),
        "relocated" => len::<Relocated>(buf),
        "res" => len::<Res>(buf),
        "tail_op" => len::<TailOp>(buf),
        "msg" => len::<Msg>(buf),
        other => panic!("no type {other}"),
    }
}

/// The C views (SW-79) on valid, truncated, empty, changed and random buffers of every type: a view is made
/// exactly when `from_fixed` reads a value, with its length, and every reader of it stays in its bytes (the C
/// program reads each case from a buffer of exactly its length, under AddressSanitizer where it links).
#[test]
fn c_views_check_bounds() {
    let mut rng = c_run::Rng(0x9e37_79b9_7f4a_7c15);
    let mut valid = Vec::new();
    for _ in 0..50 {
        encodings(&mut rng, &mut valid);
    }
    let mut input = String::new();
    let mut want = Vec::new();
    let names: Vec<&'static str> = valid[..14].iter().map(|(n, _)| *n).collect();
    for name in names {
        let of: Vec<Vec<u8>> = valid
            .iter()
            .filter(|(n, _)| *n == name)
            .map(|(_, b)| b.clone())
            .collect();
        for (ty, bytes) in c_run::cases(name, &of, 20, &mut rng) {
            input.push_str(&c_run::line(ty, &bytes));
            want.push(format!("{ty} {}", c_run::expect(rust_len(ty, &bytes))));
        }
    }
    let exe = c_run::build("fixed_layout_c.c");
    let got = c_run::run(&exe, &input);
    let got: Vec<&str> = got.lines().collect();
    assert_eq!(got.len(), want.len());
    for ((got, want), case) in got.iter().zip(&want).zip(input.lines()) {
        assert_eq!(got, want, "case {case}");
    }
}

/// Longer than a 16-bit `size_t` can count: its C text refuses to build there instead of cutting the length check
/// (SW-79).
#[derive_shrink_wrap(borrowed, fixed_layout, derive(Clone, Copy, Debug, PartialEq))]
struct Huge {
    id: u8,
    data: [u8; 70000],
}

#[test]
fn c_huge_needs_size_t() {
    let mut out = String::new();
    Huge::write_c("h_", &mut out).unwrap();
    assert!(
        out.contains("#define H_HUGE_MIN_LEN 70001u\n#if 70001u > SIZE_MAX\n#error \"H_HUGE_LEN: longer than size_t can count on this target\"\n#endif"),
        "{out}"
    );
    let mut small = String::new();
    Inner::write_c("t_", &mut small).unwrap();
    assert!(!small.contains("SIZE_MAX"), "{small}");
}
