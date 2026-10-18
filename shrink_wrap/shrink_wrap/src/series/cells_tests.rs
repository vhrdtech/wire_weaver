//! [SeriesCells] round trips (API-50): items put into zeroed cells come out of `copy_rows` as `SeriesStore::set`
//! would have put them, for every impl here and the derive's. Also run under miri (`just miri`).

use super::{SeriesCells, SeriesStore, empty_cells};
use crate::un::{I12, IN, U3, U5, U40, UN};
use crate::{Nibble, UNib32, UVlq32, UVlq64, derive_shrink_wrap};
use core::fmt::Debug;
use core::marker::PhantomData;
use proptest::prelude::*;

const R: usize = 8;
const N: usize = 12;

/// Puts `items` at `rows` (mod `R`) of zeroed cells, then copies every row that was put, in runs given by `runs`
/// (row, store index), and reads them back from the store: the item put there last.
fn check_with<T: SeriesCells + Clone + Debug>(
    items: &[T],
    rows: &[usize],
    runs: &[(usize, usize, usize)],
    eq: impl Fn(&T, &T) -> bool,
) {
    let cells = empty_cells::<T, R>();
    let mut model: [Option<T>; R] = Default::default();
    for (k, item) in items.iter().enumerate() {
        let i = rows.get(k).copied().unwrap_or(k) % R;
        T::put::<R>(&cells, i, item);
        model[i] = Some(item.clone());
    }
    // every row on its own
    let mut store = T::empty_store::<N>();
    for (i, m) in model.iter().enumerate() {
        if let Some(want) = m {
            T::copy_rows::<R, N>(&cells, i, &mut store, i, 1);
            let got = T::get(&store, i);
            assert!(eq(&got, want), "row {i}: {got:?} != {want:?}");
        }
    }
    // runs of rows that were all put
    for &(from, to, n) in runs {
        let from = from % R;
        let n = n % (R - from + 1);
        let to = to % (N - n + 1);
        if model[from..from + n].iter().any(Option::is_none) {
            continue;
        }
        let mut store = T::empty_store::<N>();
        T::copy_rows::<R, N>(&cells, from, &mut store, to, n);
        for k in 0..n {
            let got = T::get(&store, to + k);
            let want = model[from + k].as_ref().unwrap();
            assert!(
                eq(&got, want),
                "run {from}+{k} -> {}: {got:?} != {want:?}",
                to + k
            );
        }
    }
}

fn check<T: SeriesCells + Clone + Debug + PartialEq>(
    items: &[T],
    rows: &[usize],
    runs: &[(usize, usize, usize)],
) {
    check_with(items, rows, runs, |a, b| a == b)
}

#[derive_shrink_wrap(
    borrowed,
    series(cells),
    final_structure,
    derive(Clone, Copy, Debug, PartialEq)
)]
struct Pair {
    ok: bool,
    value: i32,
}

/// Nested, `Option`s of structs and enums, an array, a tuple.
#[derive_shrink_wrap(borrowed, series(cells), derive(Clone, Debug, PartialEq))]
struct Rig {
    pair: Pair,
    maybe: Option<Pair>,
    mode: Option<Mode>,
    arr: [u16; 3],
    tuple: (u8, f32),
    _unit: PhantomData<u64>,
}

/// A variant compiled out, data variants, a first discriminant that isn't 0.
#[derive_shrink_wrap(borrowed, series(cells), ww_repr = u4, derive(Clone, Debug, PartialEq))]
enum Mode {
    #[cfg(any())]
    Turbo(u16),
    Off = 3,
    On(i8),
    Both {
        a: u32,
        b: Option<u8>,
    },
}

/// A `#[future_version]` enum: `u32` discriminants, the unknown ones kept.
#[derive_shrink_wrap(borrowed, series(cells), ww_repr = unib32, derive(Clone, Debug, PartialEq))]
enum Shape {
    Empty,
    Circle(u8),
    Rect { w: u16, h: u16 },
}

/// Type and const parameters, a phantom one.
#[derive_shrink_wrap(borrowed, series(cells), derive(Clone, Debug, PartialEq))]
struct Window<T: Copy + Default, U, const M: usize> {
    samples: [T; M],
    tag: Option<T>,
    _unit: PhantomData<U>,
}

/// Unit-only: just the discriminants.
#[derive_shrink_wrap(borrowed, series(cells), ww_repr = u1, derive(Clone, Copy, Debug, PartialEq))]
enum Level {
    Low,
    High,
}

fn un(k: u8, v: u64) -> UN {
    match k % 4 {
        0 => UN::UN8 {
            bit_count: U3::new(k % 8).unwrap(),
            value: v as u8,
        },
        1 => UN::UN16 {
            bit_count: crate::un::U4::new(k % 16).unwrap(),
            value: v as u16,
        },
        2 => UN::UN32 {
            bit_count: U5::new(k % 32).unwrap(),
            value: v as u32,
        },
        _ => UN::UN64 {
            bit_count: crate::un::U6::new(k % 64).unwrap(),
            value: v,
        },
    }
}

fn in_(k: u8, v: i64) -> IN {
    match k % 4 {
        0 => IN::IN8 {
            bit_count: U3::new(k % 8).unwrap(),
            value: v as i8,
        },
        1 => IN::IN16 {
            bit_count: crate::un::U4::new(k % 16).unwrap(),
            value: v as i16,
        },
        2 => IN::IN32 {
            bit_count: U5::new(k % 32).unwrap(),
            value: v as i32,
        },
        _ => IN::IN64 {
            bit_count: crate::un::U6::new(k % 64).unwrap(),
            value: v,
        },
    }
}

#[test]
fn cells_sizes() {
    use core::mem::size_of;
    // struct of arrays, no padding: a byte and a word per item
    assert_eq!(size_of::<<Pair as SeriesCells>::Cells<32>>(), 32 * 5);
    assert_eq!(size_of::<<u64 as SeriesCells>::Cells<32>>(), 32 * 8);
    assert_eq!(size_of::<<Level as SeriesCells>::Cells<32>>(), 32);
    assert_eq!(size_of::<<PhantomData<u64> as SeriesCells>::Cells<32>>(), 0);
    // the same as the store
    assert_eq!(
        size_of::<<Pair as SeriesCells>::Cells<32>>(),
        size_of::<<Pair as SeriesStore>::Store<32>>()
    );
}

#[test]
fn const_zeroed() {
    // what generated code does in a `const fn`: `core::mem::zeroed()` of the cells
    struct Holder {
        cells: <Pair as SeriesCells>::Cells<4>,
    }
    static HOLDER: Holder = Holder {
        // SAFETY: `SeriesCells` requires all zero to be valid
        cells: unsafe { core::mem::zeroed() },
    };
    Pair::put::<4>(
        &HOLDER.cells,
        1,
        &Pair {
            ok: true,
            value: -3,
        },
    );
    let mut store = Pair::empty_store::<2>();
    Pair::copy_rows::<4, 2>(&HOLDER.cells, 1, &mut store, 0, 1);
    assert_eq!(
        Pair::get(&store, 0),
        Pair {
            ok: true,
            value: -3
        }
    );
}

fn mode(k: u8, v: u32) -> Mode {
    match k % 3 {
        0 => Mode::Off,
        1 => Mode::On(v as i8),
        _ => Mode::Both {
            a: v,
            b: k.is_multiple_of(2).then_some(k),
        },
    }
}

/// A few cases under miri, without the regression file (miri can't touch the file system).
fn config() -> ProptestConfig {
    if cfg!(miri) {
        ProptestConfig {
            cases: 2,
            failure_persistence: None,
            ..ProptestConfig::default()
        }
    } else {
        ProptestConfig::with_cases(64)
    }
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn numbers(
        a in prop::collection::vec(any::<(u8, u16, u32, u64, u128)>(), 0..12),
        b in prop::collection::vec(any::<(i8, i16, i32, i64, i128)>(), 0..12),
        rows in prop::collection::vec(any::<usize>(), 0..12),
        runs in prop::collection::vec(any::<(usize, usize, usize)>(), 0..6),
    ) {
        check(&a, &rows, &runs);
        check(&b, &rows, &runs);
        let floats: Vec<(f32, f64)> = a
            .iter()
            .map(|t| (f32::from_bits(t.2), f64::from_bits(t.3)))
            .collect();
        check_with(&floats, &rows, &runs, |x, y| {
            x.0.to_bits() == y.0.to_bits() && x.1.to_bits() == y.1.to_bits()
        });
        let wire: Vec<(Nibble, UNib32, UVlq32, UVlq64, U5, I12, U40)> = a
            .iter()
            .map(|t| {
                (
                    Nibble::new_masked(t.0),
                    UNib32(t.2),
                    UVlq32(t.2.rotate_left(7)),
                    UVlq64(t.3),
                    U5::new(t.0 % 32).unwrap(),
                    I12::new((t.1 % 4096) as i16 - 2048).unwrap(),
                    U40::new(t.3 >> 24).unwrap(),
                )
            })
            .collect();
        check(&wire, &rows, &runs);
        let bit_counts: Vec<(UN, IN)> = a
            .iter()
            .zip(&b)
            .map(|(t, s)| (un(t.0, t.3), in_(t.0.wrapping_add(1), s.3)))
            .collect();
        check(&bit_counts, &rows, &runs);
    }

    #[test]
    fn bools_options_arrays(
        items in prop::collection::vec(any::<(bool, Option<u8>, Option<Option<i16>>, [Option<u8>; 3])>(), 0..12),
        rows in prop::collection::vec(any::<usize>(), 0..12),
        runs in prop::collection::vec(any::<(usize, usize, usize)>(), 0..6),
    ) {
        check(&items, &rows, &runs);
        let phantom: Vec<PhantomData<u8>> = items.iter().map(|_| PhantomData).collect();
        check(&phantom, &rows, &runs);
    }

    #[test]
    fn derived(
        items in prop::collection::vec(any::<(u8, u16, u32, i8, bool)>(), 0..12),
        rows in prop::collection::vec(any::<usize>(), 0..12),
        runs in prop::collection::vec(any::<(usize, usize, usize)>(), 0..6),
    ) {
        let rigs: Vec<Rig> = items
            .iter()
            .map(|&(k, v, w, s, ok)| Rig {
                pair: Pair { ok, value: w as i32 },
                maybe: (k % 3 == 0).then_some(Pair { ok: !ok, value: s as i32 }),
                mode: (k % 4 != 0).then(|| mode(k, w)),
                arr: [v, v.rotate_left(5), k as u16],
                tuple: (k, f32::from_bits(w & 0x7f7f_ffff)),
                _unit: PhantomData,
            })
            .collect();
        check(&rigs, &rows, &runs);
        let modes: Vec<Mode> = items.iter().map(|&(k, _, w, _, _)| mode(k, w)).collect();
        check(&modes, &rows, &runs);
        let shapes: Vec<Shape> = items
            .iter()
            .map(|&(k, v, w, _, _)| match k % 4 {
                0 => Shape::Empty,
                1 => Shape::Circle(k),
                2 => Shape::Rect { w: v, h: v ^ 0x55 },
                _ => Shape::Unknown(3 + (w % 1000)),
            })
            .collect();
        check(&shapes, &rows, &runs);
        let windows: Vec<Window<i8, (), 3>> = items
            .iter()
            .map(|&(k, v, _, s, _)| Window {
                samples: [s, k as i8, v as i8],
                tag: (k % 3 != 0).then_some(s),
                _unit: PhantomData,
            })
            .collect();
        check(&windows, &rows, &runs);
        let levels: Vec<Level> = items
            .iter()
            .map(|&(_, _, _, _, ok)| if ok { Level::High } else { Level::Low })
            .collect();
        check(&levels, &rows, &runs);
    }
}
