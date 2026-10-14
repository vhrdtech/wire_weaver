//! `SerializeShrinkWrap::MAX_SIZE` (SW-19): a buffer of `MaxSize::buffer_len()` bytes always fits the value, for
//! the largest values of each type, and it is not much larger than what they take.

use shrink_wrap::prelude::*;

/// Serializes `value` into a buffer of exactly `T::MAX_SIZE.buffer_len()` bytes, returns (bound, bytes taken).
fn fits<T: SerializeShrinkWrap>(value: &T) -> (usize, usize) {
    let max = T::MAX_SIZE.expect("bounded").buffer_len();
    let mut buf = vec![0u8; max];
    let len = value
        .to_ww_bytes(&mut buf)
        .unwrap_or_else(|e| panic!("does not fit into {max} bytes: {e:?}"))
        .len();
    (max, len)
}

/// [fits], and the bound is at most `slack` bytes over the largest value.
fn tight<T: SerializeShrinkWrap>(value: &T, slack: usize) {
    let (max, len) = fits(value);
    assert!(
        max <= len + slack,
        "bound {max} is more than {slack} bytes over {len}"
    );
}

#[derive_shrink_wrap(borrowed, derive(Debug, Clone))]
struct Plain {
    a: bool,
    b: u8,
    c: U3,
    d: u16,
    e: Nibble,
    f: UNib32,
    g: f64,
    h: I7,
}

fn plain() -> Plain {
    Plain {
        a: true,
        b: 0xFF,
        c: U3::new(7).unwrap(),
        d: 0xFFFF,
        e: Nibble::new(0xF).unwrap(),
        f: UNib32(u32::MAX),
        g: 1.0,
        h: I7::new(-1).unwrap(),
    }
}

#[derive_shrink_wrap(borrowed, derive(Debug, Clone), ww_repr = u3)]
enum Choice<'i> {
    Unit,
    Wide(u32),
    Named {
        flag: bool,
        name: MaxLen<&'i str, 5>,
    },
}

#[derive_shrink_wrap(borrowed, derive(Debug, Clone), ww_repr = unib32)]
enum Far {
    A,
    B(u8),
    #[discriminant = 100]
    C(u16),
}

#[derive_shrink_wrap(borrowed, derive(Debug, Clone), self_describing, ww_repr = nib)]
enum Small {
    Off,
    On { level: u8 },
}

#[derive_shrink_wrap(borrowed, derive(Debug, Clone), sized)]
struct Point {
    x: i16,
    y: i16,
    on: bool,
}

#[derive_shrink_wrap(borrowed, derive(Debug, Clone), final_structure)]
struct Final<'i> {
    on: bool,
    name: MaxLen<&'i str, 3>,
}

#[derive_shrink_wrap(borrowed, derive(Debug, Clone))]
struct Outer<'i> {
    flag: bool,
    plain: Plain,
    choice: Choice<'i>,
    far: Far,
    small: Small,
    point: Point,
    fin: Final<'i>,
    maybe: Option<Choice<'i>>,
    result: Result<u8, Choice<'i>>,
    array: [Option<u16>; 3],
    tuple: (bool, u64, Small),
    list: MaxLen<RefVec<'i, Plain>, 3>,
    names: MaxLen<RefVec<'i, MaxLen<&'i str, 300>>, 20>,
    bytes: MaxLen<RefVec<'i, u8>, 1000>,
    range: core::ops::Range<u8>,
    vlq: UVlq32,
}

fn named() -> Choice<'static> {
    Choice::Named {
        flag: true,
        name: MaxLen::new("abcde").unwrap(),
    }
}

#[test]
fn plain_types() {
    tight(&plain(), 1);
    tight(&true, 0);
    tight(&0xFFu8, 0);
    tight(&u128::MAX, 0);
    tight(&UNib32(u32::MAX), 0);
    tight(&UVlq64(u64::MAX), 0);
    tight(&[true; 9], 0);
    tight(&(true, 0u8, true), 0);
    tight(&Some(0u32), 0);
    tight(&Option::<u32>::None, 5);
    tight(&Result::<u8, u32>::Err(0), 0);
}

#[test]
fn enums_take_the_largest_variant() {
    tight(&named(), 4);
    tight(&Choice::Wide(u32::MAX), 6);
    tight(&Far::C(0xFFFF), 1);
    tight(&Small::On { level: 0xFF }, 1);
}

#[test]
fn bounded_lists_and_strings() {
    let s = "x".repeat(300);
    let names: Vec<MaxLen<&str, 300>> = (0..20).map(|_| MaxLen::new(s.as_str()).unwrap()).collect();
    let list =
        MaxLen::<RefVec<'_, MaxLen<&str, 300>>, 20>::new(RefVec::Slice { slice: &names }).unwrap();
    tight(&list, 16);

    let bytes = [0xAA; 1000];
    tight(
        &MaxLen::<RefVec<'_, u8>, 1000>::new(RefVec::new_bytes(&bytes)).unwrap(),
        2,
    );
    tight(&MaxLen::<&str, 7>::new("1234567").unwrap(), 2);
    // a list of Unsized values: each gets its size
    let plains = [plain(), plain(), plain()];
    tight(
        &MaxLen::<RefVec<'_, Plain>, 3>::new(RefVec::Slice { slice: &plains }).unwrap(),
        8,
    );
}

#[test]
fn nested() {
    let s = "x".repeat(300);
    let names: Vec<MaxLen<&str, 300>> = (0..20).map(|_| MaxLen::new(s.as_str()).unwrap()).collect();
    let plains = [plain(), plain(), plain()];
    let bytes = [0xAA; 1000];
    let outer = Outer {
        flag: true,
        plain: plain(),
        choice: named(),
        far: Far::C(0xFFFF),
        small: Small::On { level: 0xFF },
        point: Point {
            x: -1,
            y: -1,
            on: true,
        },
        fin: Final {
            on: true,
            name: MaxLen::new("abc").unwrap(),
        },
        maybe: Some(named()),
        result: Err(named()),
        array: [Some(0xFFFF); 3],
        tuple: (true, u64::MAX, Small::On { level: 0xFF }),
        list: MaxLen::new(RefVec::Slice { slice: &plains }).unwrap(),
        names: MaxLen::new(RefVec::Slice { slice: &names }).unwrap(),
        bytes: MaxLen::new(RefVec::new_bytes(&bytes)).unwrap(),
        range: 0..0xFF,
        vlq: UVlq32(u32::MAX),
    };
    let (max, len) = fits(&outer);
    // the bound adds up the worst case of every part, the value has the worst case of most of them
    assert!(max <= len + len / 20, "{max} vs {len}");
}

#[derive_shrink_wrap(borrowed, derive(Debug, Clone))]
struct WithTailSize<'i> {
    flag: bool,
    size: TailSize<2>,
    name: MaxLen<&'i str, 9>,
    plain: Plain,
    last: bool,
}

#[test]
fn tail_size() {
    tight(
        &WithTailSize {
            flag: true,
            size: TailSize(0),
            name: MaxLen::new("123456789").unwrap(),
            plain: plain(),
            last: true,
        },
        // 2 bytes for the lengths encoded at the slot, the length of `name` counted as a u16
        5,
    );
}

#[test]
fn every_alignment_fits() {
    // the bound holds whatever the bit position a value starts at
    for bools in 0..8 {
        let flags = [true; 8];
        let value = (
            MaxLen::<RefVec<'_, bool>, 8>::new(RefVec::Slice {
                slice: &flags[..bools],
            })
            .unwrap(),
            Small::On { level: 1 },
            plain(),
            named(),
        );
        fits(&value);
    }
}

#[derive_shrink_wrap(borrowed, derive(Debug, Clone))]
struct Unbounded<'i> {
    a: u8,
    list: RefVec<'i, u8>,
}

#[derive_shrink_wrap(borrowed, derive(Debug, Clone))]
struct Recursive<'i> {
    next: Option<RefBox<'i, Recursive<'i>>>,
}

#[test]
fn unbounded_parts_make_it_unbounded() {
    assert_eq!(<Unbounded<'_> as SerializeShrinkWrap>::MAX_SIZE, None);
    assert_eq!(
        <Option<Unbounded<'_>> as SerializeShrinkWrap>::MAX_SIZE,
        None
    );
    assert_eq!(<Recursive<'_> as SerializeShrinkWrap>::MAX_SIZE, None);
    assert_eq!(<&str as SerializeShrinkWrap>::MAX_SIZE, None);
    assert_eq!(<RefVec<'_, u8> as SerializeShrinkWrap>::MAX_SIZE, None);
    assert_eq!(<Vec<u8> as SerializeShrinkWrap>::MAX_SIZE, None);
    assert_eq!(<String as SerializeShrinkWrap>::MAX_SIZE, None);
    assert_eq!(
        <MaxLen<RefVec<'_, Unbounded<'_>>, 2> as SerializeShrinkWrap>::MAX_SIZE,
        None
    );
}

#[test]
fn usable_in_const_context() {
    const PLAIN: usize = MaxSize::buffer_len_of(<Plain as SerializeShrinkWrap>::MAX_SIZE).unwrap();
    let buf = [0u8; PLAIN];
    assert_eq!(buf.len(), fits(&plain()).0);
}
