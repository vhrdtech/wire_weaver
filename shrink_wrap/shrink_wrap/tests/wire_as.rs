//! `#[wire(W)] field: T` (DER-19): a plain integer in Rust, the bytes of `W` on the wire. Each type here has a twin
//! with `W` as the field type, whose bytes it must match; a value `W` can't hold is an error when it is written, a
//! value `T` can't hold an error when it is read.

use shrink_wrap::Error;
use shrink_wrap::prelude::*;
use shrink_wrap::series::{
    CodecPolicy, SeriesCells, SeriesColumns, SeriesOwned, SeriesStore, empty_cells,
    write_store_series,
};

#[derive_shrink_wrap(borrowed, owned, series(cells), derive(Debug, PartialEq, Clone, Copy))]
struct Packed {
    #[wire(I4)]
    offset: i8,
    #[wire(UNib32)]
    count: u64,
    #[wire(Nibble)]
    nib: u8,
    #[wire(U12)]
    addr: u16,
    #[wire(u8)]
    small: u32,
    #[wire(i16)]
    temp: i64,
    plain: u8,
}

#[derive_shrink_wrap(borrowed, owned, series, derive(Debug, PartialEq, Clone, Copy))]
struct PackedTwin {
    offset: I4,
    count: UNib32,
    nib: Nibble,
    addr: U12,
    small: u8,
    temp: i16,
    plain: u8,
}

fn packed(offset: i8, count: u64) -> Packed {
    Packed {
        offset,
        count,
        nib: 9,
        addr: 0xABC,
        small: 200,
        temp: -300,
        plain: 7,
    }
}

fn twin(p: &Packed) -> PackedTwin {
    PackedTwin {
        offset: I4::new(p.offset).unwrap(),
        count: UNib32(p.count as u32),
        nib: Nibble::new(p.nib).unwrap(),
        addr: U12::new(p.addr).unwrap(),
        small: p.small as u8,
        temp: p.temp as i16,
        plain: p.plain,
    }
}

#[test]
fn bytes_are_the_wire_types() {
    for p in [
        packed(-8, 0),
        packed(7, 1_000_000),
        packed(0, u32::MAX as u64),
    ] {
        let mut buf = [0u8; 64];
        let bytes = p.to_ww_bytes(&mut buf).unwrap().to_vec();
        let mut buf = [0u8; 64];
        assert_eq!(bytes, twin(&p).to_ww_bytes(&mut buf).unwrap());
        assert_eq!(Packed::from_ww_bytes(&bytes).unwrap(), p);
        // owned: the same type, its fields are all plain
        assert_eq!(p.to_ww_bytes_owned().unwrap(), bytes);
        assert_eq!(Packed::from_ww_bytes_owned(&bytes).unwrap(), p);
    }
}

#[test]
fn out_of_range_write_is_an_error() {
    let mut buf = [0u8; 64];
    for p in [
        packed(8, 0),
        packed(-9, 0),
        packed(0, u32::MAX as u64 + 1),
        Packed {
            nib: 16,
            ..packed(0, 0)
        },
        Packed {
            addr: 0x1000,
            ..packed(0, 0)
        },
        Packed {
            small: 256,
            ..packed(0, 0)
        },
        Packed {
            temp: i16::MIN as i64 - 1,
            ..packed(0, 0)
        },
    ] {
        assert_eq!(
            p.to_ww_bytes(&mut buf),
            Err(Error::SubtypeOutOfRange),
            "{p:?}"
        );
    }
    assert_eq!(
        packed(8, 0).to_ww_bytes_owned(),
        Err(Error::SubtypeOutOfRange)
    );
}

/// The wire type holds more than the field: what doesn't fit is an error, never cut.
#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq))]
struct Narrow {
    #[wire(UNib32)]
    count: u16,
    #[wire(i32)]
    value: i8,
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq))]
struct NarrowTwin {
    count: UNib32,
    value: i32,
}

#[test]
fn out_of_range_read_is_an_error() {
    let mut buf = [0u8; 64];
    let fits = NarrowTwin {
        count: UNib32(u16::MAX as u32),
        value: -128,
    };
    let bytes = fits.to_ww_bytes(&mut buf).unwrap().to_vec();
    assert_eq!(
        Narrow::from_ww_bytes(&bytes).unwrap(),
        Narrow {
            count: u16::MAX,
            value: -128
        }
    );
    for twin in [
        NarrowTwin {
            count: UNib32(u16::MAX as u32 + 1),
            value: 0,
        },
        NarrowTwin {
            count: UNib32(0),
            value: -129,
        },
    ] {
        let mut buf = [0u8; 64];
        let bytes = twin.to_ww_bytes(&mut buf).unwrap();
        assert_eq!(Narrow::from_ww_bytes(bytes), Err(Error::SubtypeOutOfRange));
        assert_eq!(
            Narrow::from_ww_bytes_owned(bytes),
            Err(Error::SubtypeOutOfRange)
        );
    }
}

/// Every family of pairs, its edges and one past them.
macro_rules! pair {
    ($name:ident, $plain:ty, $wire:ty, $min:expr, $max:expr, $below:expr, $above:expr) => {
        #[test]
        fn $name() {
            #[derive_shrink_wrap(borrowed, derive(Debug, PartialEq))]
            struct S {
                #[wire($wire)]
                v: $plain,
                tail: u8,
            }
            #[derive_shrink_wrap(borrowed, derive(Debug, PartialEq))]
            struct Twin {
                v: $wire,
                tail: u8,
            }
            for v in [$min, $max] {
                let mut buf = [0u8; 32];
                let bytes = S { v, tail: 0xA5 }.to_ww_bytes(&mut buf).unwrap().to_vec();
                let twin = Twin {
                    v: <$wire as WireAs<$plain>>::from_plain(v).unwrap(),
                    tail: 0xA5,
                };
                let mut buf = [0u8; 32];
                assert_eq!(bytes, twin.to_ww_bytes(&mut buf).unwrap(), "{v}");
                assert_eq!(S::from_ww_bytes(&bytes).unwrap(), S { v, tail: 0xA5 });
            }
            let out: [Option<$plain>; 2] = [$below, $above];
            for v in out.into_iter().flatten() {
                let mut buf = [0u8; 32];
                assert_eq!(
                    S { v, tail: 0 }.to_ww_bytes(&mut buf),
                    Err(Error::SubtypeOutOfRange),
                    "{v}"
                );
            }
        }
    };
}

pair!(u8_as_u1, u8, U1, 0, 1, None, Some(2));
pair!(u8_as_nibble, u8, Nibble, 0, 15, None, Some(16));
pair!(u16_as_u8, u16, u8, 0, 255, None, Some(256));
pair!(u32_as_u12, u32, U12, 0, 4095, None, Some(4096));
pair!(u64_as_u33, u64, U33, 0, (1 << 33) - 1, None, Some(1 << 33));
pair!(u64_as_u64, u64, u64, 0, u64::MAX, None, None);
pair!(
    u128_as_u64,
    u128,
    u64,
    0,
    u64::MAX as u128,
    None,
    Some(u64::MAX as u128 + 1)
);
pair!(usize_as_u16, usize, u16, 0, 65535, None, Some(65536));
pair!(u8_as_u16, u8, u16, 0, 255, None, None);
pair!(
    u64_as_unib32,
    u64,
    UNib32,
    0,
    u32::MAX as u64,
    None,
    Some(u32::MAX as u64 + 1)
);
pair!(
    u64_as_uvlq32,
    u64,
    UVlq32,
    0,
    u32::MAX as u64,
    None,
    Some(u32::MAX as u64 + 1)
);
pair!(
    u128_as_uvlq64,
    u128,
    UVlq64,
    0,
    u64::MAX as u128,
    None,
    Some(u64::MAX as u128 + 1)
);
pair!(u16_as_uvlq64, u16, UVlq64, 0, u16::MAX, None, None);
pair!(i8_as_i2, i8, I2, -2, 1, Some(-3), Some(2));
pair!(i8_as_i4, i8, I4, -8, 7, Some(-9), Some(8));
pair!(i32_as_i8, i32, i8, -128, 127, Some(-129), Some(128));
pair!(
    i64_as_i40,
    i64,
    I40,
    -(1 << 39),
    (1 << 39) - 1,
    Some(-(1 << 39) - 1),
    Some(1 << 39)
);
pair!(i64_as_i64, i64, i64, i64::MIN, i64::MAX, None, None);
pair!(
    i128_as_i64,
    i128,
    i64,
    i64::MIN as i128,
    i64::MAX as i128,
    Some(i64::MIN as i128 - 1),
    Some(i64::MAX as i128 + 1)
);
pair!(
    isize_as_i16,
    isize,
    i16,
    -32768,
    32767,
    Some(-32769),
    Some(32768)
);
pair!(i8_as_i32, i8, i32, i8::MIN, i8::MAX, None, None);

#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq))]
struct V1 {
    a: u8,
}

/// `V1` with a field added later: an older writer's data reads as its default.
#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq))]
struct V2 {
    a: u8,
    #[default = 3]
    #[wire(U4)]
    b: u16,
}

#[test]
fn default_of_an_evolved_field() {
    let mut buf = [0u8; 16];
    let old = V1 { a: 1 }.to_ww_bytes(&mut buf).unwrap().to_vec();
    assert_eq!(V2::from_ww_bytes(&old).unwrap(), V2 { a: 1, b: 3 });
    assert_eq!(V2::from_ww_bytes_owned(&old).unwrap(), V2 { a: 1, b: 3 });
    let mut buf = [0u8; 16];
    let new = V2 { a: 1, b: 15 }.to_ww_bytes(&mut buf).unwrap().to_vec();
    assert_eq!(V2::from_ww_bytes(&new).unwrap(), V2 { a: 1, b: 15 });
    assert_eq!(V1::from_ww_bytes(&new).unwrap(), V1 { a: 1 });
}

#[derive_shrink_wrap(borrowed, owned, series, self_describing, ww_repr = u4, derive(Debug, PartialEq, Clone, Copy))]
enum Reading {
    Idle,
    Offset(#[wire(I4)] i8, #[wire(U4)] u8),
    Count {
        #[wire(UNib32)]
        n: u64,
        #[wire(U3)]
        kind: u8,
    },
}

#[derive_shrink_wrap(borrowed, owned, series, self_describing, ww_repr = u4, derive(Debug, PartialEq, Clone, Copy))]
enum ReadingTwin {
    Idle,
    Offset(I4, U4),
    Count { n: UNib32, kind: U3 },
}

#[test]
fn enum_fields() {
    let cases = [
        (Reading::Idle, ReadingTwin::Idle),
        (
            Reading::Offset(-3, 12),
            ReadingTwin::Offset(I4::new(-3).unwrap(), U4::new(12).unwrap()),
        ),
        (
            Reading::Count { n: 300, kind: 5 },
            ReadingTwin::Count {
                n: UNib32(300),
                kind: U3::new(5).unwrap(),
            },
        ),
    ];
    for (r, t) in cases {
        let mut buf = [0u8; 16];
        let bytes = r.to_ww_bytes(&mut buf).unwrap().to_vec();
        let mut buf = [0u8; 16];
        assert_eq!(bytes, t.to_ww_bytes(&mut buf).unwrap());
        assert_eq!(Reading::from_ww_bytes(&bytes).unwrap(), r);
        assert_eq!(
            Reading::from_ww_bytes_owned(&bytes)
                .unwrap()
                .to_ww_bytes_owned()
                .unwrap(),
            bytes
        );
    }
    let mut buf = [0u8; 16];
    assert_eq!(
        Reading::Offset(8, 0).to_ww_bytes(&mut buf),
        Err(Error::SubtypeOutOfRange)
    );
    assert_eq!(
        Reading::Count { n: 0, kind: 8 }.to_ww_bytes(&mut buf),
        Err(Error::SubtypeOutOfRange)
    );
}

/// A `sized` type is the wire types' bits: 4 + 4 = 8.
#[derive_shrink_wrap(borrowed, sized, derive(Debug, PartialEq))]
struct Tuple(#[wire(I4)] i8, #[wire(U4)] u8);

#[test]
fn tuple_struct_and_size() {
    assert_eq!(
        <Tuple as SerializeShrinkWrap>::ELEMENT_SIZE,
        ElementSize::Sized { size_bits: 8 }
    );
    let mut buf = [0u8; 4];
    let bytes = Tuple(-1, 2).to_ww_bytes(&mut buf).unwrap().to_vec();
    assert_eq!(bytes.len(), 1);
    assert_eq!(Tuple::from_ww_bytes(&bytes).unwrap(), Tuple(-1, 2));
}

/// The form an API crate with shrink_wrap optional writes: the attribute under the derive's cfg.
#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq))]
struct UnderCfg {
    #[cfg_attr(all(), wire(I4))]
    offset: i8,
    #[cfg_attr(any(test, not(test)), wire(U4))]
    gain: u8,
}

#[test]
fn cfg_attr_form() {
    let mut buf = [0u8; 4];
    let bytes = UnderCfg {
        offset: -2,
        gain: 3,
    }
    .to_ww_bytes(&mut buf)
    .unwrap()
    .to_vec();
    assert_eq!(bytes.len(), 1);
    assert_eq!(
        UnderCfg::from_ww_bytes(&bytes).unwrap(),
        UnderCfg {
            offset: -2,
            gain: 3
        }
    );
}

fn series_bytes<T: for<'i> SeriesColumns<'i>>(items: &[T]) -> Result<Vec<u8>, Error> {
    Series::new(items).to_ww_bytes_owned()
}

#[test]
fn series_columns_are_the_wire_types() {
    let items: Vec<Packed> = (0..20)
        .map(|k| Packed {
            nib: (k % 16) as u8,
            ..packed((k % 16) as i8 - 8, k * 1000)
        })
        .collect();
    let twins: Vec<PackedTwin> = items.iter().map(twin).collect();
    let bytes = series_bytes(&items).unwrap();
    assert_eq!(bytes, series_bytes(&twins).unwrap());
    assert_eq!(
        SeriesOwned::<Packed>::from_ww_bytes_owned(&bytes)
            .unwrap()
            .0,
        items
    );

    let mut out = items.clone();
    out[13].offset = 8;
    assert_eq!(series_bytes(&out), Err(Error::SubtypeOutOfRange));

    let readings = [
        Reading::Offset(-3, 12),
        Reading::Idle,
        Reading::Count { n: 300, kind: 5 },
        Reading::Offset(7, 0),
    ];
    let twins = [
        ReadingTwin::Offset(I4::new(-3).unwrap(), U4::new(12).unwrap()),
        ReadingTwin::Idle,
        ReadingTwin::Count {
            n: UNib32(300),
            kind: U3::new(5).unwrap(),
        },
        ReadingTwin::Offset(I4::new(7).unwrap(), U4::new(0).unwrap()),
    ];
    let bytes = series_bytes(&readings).unwrap();
    assert_eq!(bytes, series_bytes(&twins).unwrap());
    assert_eq!(
        SeriesOwned::<Reading>::from_ww_bytes_owned(&bytes)
            .unwrap()
            .0,
        readings
    );
}

#[test]
fn series_store_and_cells() {
    const N: usize = 8;
    let items: Vec<Packed> = (0..N as u64).map(|k| packed(k as i8 - 4, k * 77)).collect();
    let mut store = Packed::empty_store::<N>();
    for (i, item) in items.iter().enumerate() {
        Packed::set(&mut store, i, item);
    }
    for (i, item) in items.iter().enumerate() {
        assert_eq!(&Packed::get(&store, i), item);
    }
    let write = |store: &<Packed as SeriesStore>::Store<N>| {
        let mut buf = [0u8; 512];
        let mut wr = BufWriter::new(&mut buf);
        write_store_series::<Packed, _, N>(&mut wr, store, 0..N, &CodecPolicy::SMALLEST)?;
        Ok::<_, Error>(wr.finish_and_take()?.to_vec())
    };
    assert_eq!(write(&store).unwrap(), series_bytes(&items).unwrap());

    let cells = empty_cells::<Packed, N>();
    for (i, item) in items.iter().enumerate() {
        Packed::put::<N>(&cells, i, item);
    }
    let mut copied = Packed::empty_store::<N>();
    Packed::copy_rows::<N, N>(&cells, 0, &mut copied, 0, N);
    for (i, item) in items.iter().enumerate() {
        assert_eq!(&Packed::get(&copied, i), item);
    }

    Packed::set(&mut store, 5, &packed(-9, 0));
    assert_eq!(write(&store), Err(Error::SubtypeOutOfRange));
}
