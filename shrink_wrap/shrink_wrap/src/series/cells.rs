//! [SeriesCells]: a ring of items as a struct of arrays of atomics, written by one context (an interrupt) and
//! copied out by another into a [SeriesStore] (API-39, observe cells).

use super::SeriesStore;
use crate::un::{IN, U3, U4, U5, U6, UN};
use crate::{Nibble, UNib32, UVlq32, UVlq64};
use core::marker::PhantomData;
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU16, AtomicU32, Ordering};

/// Room for `R` items of `Self` as a struct of arrays of atomics, one array per leaf field (two `u32` words for a
/// 64-bit leaf, four for a 128-bit one), shared between one writer and one reader without a lock: the writer
/// [puts](Self::put) items with relaxed stores, the reader [copies](Self::copy_rows) them into a [SeriesStore]
/// with relaxed loads. On a Cortex-M a relaxed store or load is a plain `str` / `ldr`, the same code as a global
/// variable, and there is no data race (no `static mut`, no UB).
///
/// Every value is written and read as whole words, so a value only mixes two items (two words of a 64-bit leaf, or
/// a `Quantity`'s prefix and number) when the reader copies a row while the writer writes the same row. Whoever
/// shares the cells must make sure such rows are not used: the observe cells of `ww_stream` count rows (seqlock
/// style) and drop the rows the writer may have overwritten during a copy.
///
/// `#[derive_shrink_wrap(.., series)]` implements it next to [SeriesStore] (structs: their fields' cells; enums:
/// the discriminants and their data variants' cells), unless a field has none (strings, byte lists, opaque
/// fields) or with `series(no_store)`.
///
/// # Safety
///
/// All-zero bytes must be a valid value of [Cells](Self::Cells) (atomics, arrays and tuples of them, unit
/// types), so that [empty_cells] and `const` constructors can make one with `core::mem::zeroed()`. What a row of
/// zeroed cells copies out as needn't be a meaningful item: only rows that were [put](Self::put) are copied.
pub unsafe trait SeriesCells: SeriesStore {
    /// The arrays of atomics of `R` items.
    type Cells<const R: usize>: Sync;

    /// Puts `item` at row `i`, with relaxed stores.
    fn put<const R: usize>(cells: &Self::Cells<R>, i: usize, item: &Self);

    /// Copies rows `from..from + n` of `cells` to indices `to..to + n` of `store`, with relaxed loads, as
    /// [SeriesStore::set] of each item would put them. Panics when a range is out of bounds, like slicing does.
    fn copy_rows<const R: usize, const N: usize>(
        cells: &Self::Cells<R>,
        from: usize,
        store: &mut Self::Store<N>,
        to: usize,
        n: usize,
    );
}

/// Cells of `R` items, all zero (see [SeriesCells]' safety section).
pub fn empty_cells<T: SeriesCells, const R: usize>() -> T::Cells<R> {
    // SAFETY: `SeriesCells` requires all-zero bytes to be a valid `Cells`
    unsafe { core::mem::zeroed() }
}

/// An unsigned word kept in atomics (two or four `AtomicU32`s for 64 and 128 bits): the leaves of [SeriesCells].
pub trait CellWord: Copy {
    /// `R` of them, all zero is valid.
    type Cells<const R: usize>: Sync;

    fn put<const R: usize>(cells: &Self::Cells<R>, i: usize, value: Self);

    fn load<const R: usize>(cells: &Self::Cells<R>, i: usize) -> Self;
}

macro_rules! cell_word {
    ($($ty:ty: $atomic:ty),*) => {
        $(
            impl CellWord for $ty {
                type Cells<const R: usize> = [$atomic; R];

                #[inline(always)]
                fn put<const R: usize>(cells: &Self::Cells<R>, i: usize, value: Self) {
                    cells[i].store(value, Ordering::Relaxed);
                }

                #[inline(always)]
                fn load<const R: usize>(cells: &Self::Cells<R>, i: usize) -> Self {
                    cells[i].load(Ordering::Relaxed)
                }
            }
        )*
    };
}

cell_word!(u8: AtomicU8, u16: AtomicU16, u32: AtomicU32);

impl CellWord for u64 {
    /// Low and high words.
    type Cells<const R: usize> = [[AtomicU32; R]; 2];

    #[inline(always)]
    fn put<const R: usize>(cells: &Self::Cells<R>, i: usize, value: Self) {
        cells[0][i].store(value as u32, Ordering::Relaxed);
        cells[1][i].store((value >> 32) as u32, Ordering::Relaxed);
    }

    #[inline(always)]
    fn load<const R: usize>(cells: &Self::Cells<R>, i: usize) -> Self {
        let lo = cells[0][i].load(Ordering::Relaxed);
        let hi = cells[1][i].load(Ordering::Relaxed);
        (hi as u64) << 32 | lo as u64
    }
}

impl CellWord for u128 {
    /// Words from the lowest.
    type Cells<const R: usize> = [[AtomicU32; R]; 4];

    #[inline(always)]
    fn put<const R: usize>(cells: &Self::Cells<R>, i: usize, value: Self) {
        for (w, cell) in cells.iter().enumerate() {
            cell[i].store((value >> (32 * w)) as u32, Ordering::Relaxed);
        }
    }

    #[inline(always)]
    fn load<const R: usize>(cells: &Self::Cells<R>, i: usize) -> Self {
        let mut value = 0u128;
        for (w, cell) in cells.iter().enumerate() {
            value |= (cell[i].load(Ordering::Relaxed) as u128) << (32 * w);
        }
        value
    }
}

impl CellWord for bool {
    type Cells<const R: usize> = [AtomicBool; R];

    #[inline(always)]
    fn put<const R: usize>(cells: &Self::Cells<R>, i: usize, value: Self) {
        cells[i].store(value, Ordering::Relaxed);
    }

    #[inline(always)]
    fn load<const R: usize>(cells: &Self::Cells<R>, i: usize) -> Self {
        cells[i].load(Ordering::Relaxed)
    }
}

/// A leaf whose store is an array of itself, kept as a [CellWord] `$word`.
macro_rules! word_leaf {
    ($($ty:ty: $word:ty = |$v:ident| $to:expr, |$w:ident| $from:expr;)*) => {
        $(
            // SAFETY: the cells are arrays of atomics, all zero is valid
            unsafe impl SeriesCells for $ty {
                type Cells<const R: usize> = <$word as CellWord>::Cells<R>;

                #[inline(always)]
                fn put<const R: usize>(cells: &Self::Cells<R>, i: usize, item: &Self) {
                    let $v = *item;
                    <$word as CellWord>::put::<R>(cells, i, $to);
                }

                #[inline]
                fn copy_rows<const R: usize, const N: usize>(
                    cells: &Self::Cells<R>,
                    from: usize,
                    store: &mut [$ty; N],
                    to: usize,
                    n: usize,
                ) {
                    for (k, slot) in store[to..to + n].iter_mut().enumerate() {
                        let $w = <$word as CellWord>::load::<R>(cells, from + k);
                        *slot = $from;
                    }
                }
            }
        )*
    };
}

word_leaf! {
    u8: u8 = |v| v, |w| w;
    u16: u16 = |v| v, |w| w;
    u32: u32 = |v| v, |w| w;
    u64: u64 = |v| v, |w| w;
    u128: u128 = |v| v, |w| w;
    i8: u8 = |v| v as u8, |w| w as i8;
    i16: u16 = |v| v as u16, |w| w as i16;
    i32: u32 = |v| v as u32, |w| w as i32;
    i64: u64 = |v| v as u64, |w| w as i64;
    i128: u128 = |v| v as u128, |w| w as i128;
    f32: u32 = |v| v.to_bits(), |w| f32::from_bits(w);
    f64: u64 = |v| v.to_bits(), |w| f64::from_bits(w);
    bool: bool = |v| v, |w| w;
    Nibble: u8 = |v| v.value(), |w| Nibble::new_masked(w);
    UNib32: u32 = |v| v.0, |w| UNib32(w);
    UVlq32: u32 = |v| v.0, |w| UVlq32(w);
    UVlq64: u64 = |v| v.0, |w| UVlq64(w);
}

macro_rules! un_cells {
    ($word:ty: $($ty:ident),*) => {
        word_leaf! {
            $(
                crate::un::$ty: $word = |v| v.value() as $word,
                    |w| crate::un::$ty::new(w as _).unwrap_or(crate::un::$ty::zero());
            )*
        }
    };
}

un_cells!(u8: U1, U2, U3, U4, U5, U6, U7, U8);
un_cells!(u16: U9, U10, U11, U12, U13, U14, U15, U16);
un_cells!(u32: U17, U18, U19, U20, U21, U22, U23, U24, U25, U26, U27, U28, U29, U30, U31, U32);
un_cells!(u64: U33, U34, U35, U36, U37, U38, U39, U40, U41, U42, U43, U44, U45, U46, U47, U48, U49, U50, U51, U52,
    U53, U54, U55, U56, U57, U58, U59, U60, U61, U62, U63, U64);

macro_rules! in_cells {
    ($word:ty, $int:ty: $($ty:ident),*) => {
        word_leaf! {
            $(
                crate::un::$ty: $word = |v| v.value() as $int as $word,
                    |w| crate::un::$ty::new(w as $int).unwrap_or(crate::un::$ty::zero());
            )*
        }
    };
}

in_cells!(u8, i8: I2, I3, I4, I5, I6, I7, I8);
in_cells!(u16, i16: I9, I10, I11, I12, I13, I14, I15, I16);
in_cells!(u32, i32: I17, I18, I19, I20, I21, I22, I23, I24, I25, I26, I27, I28, I29, I30, I31, I32);
in_cells!(u64, i64: I33, I34, I35, I36, I37, I38, I39, I40, I41, I42, I43, I44, I45, I46, I47, I48, I49, I50,
    I51, I52, I53, I54, I55, I56, I57, I58, I59, I60, I61, I62, I63, I64);

/// `UN` / `IN`: the width variant (0 to 3) in the two high bits and the bit count below, and the value's bits.
pub struct UnCells<const R: usize> {
    width: [AtomicU8; R],
    value: <u64 as CellWord>::Cells<R>,
}

const fn width_byte(width: u8, bit_count: u8) -> u8 {
    width << 6 | bit_count
}

// SAFETY: atomics only
unsafe impl SeriesCells for UN {
    type Cells<const R: usize> = UnCells<R>;

    fn put<const R: usize>(cells: &UnCells<R>, i: usize, item: &Self) {
        let (width, bits, value) = match *item {
            UN::UN8 { bit_count, value } => (0, bit_count.value(), value as u64),
            UN::UN16 { bit_count, value } => (1, bit_count.value(), value as u64),
            UN::UN32 { bit_count, value } => (2, bit_count.value(), value as u64),
            UN::UN64 { bit_count, value } => (3, bit_count.value(), value),
        };
        cells.width[i].store(width_byte(width, bits), Ordering::Relaxed);
        <u64 as CellWord>::put::<R>(&cells.value, i, value);
    }

    fn copy_rows<const R: usize, const N: usize>(
        cells: &UnCells<R>,
        from: usize,
        store: &mut [UN; N],
        to: usize,
        n: usize,
    ) {
        for (k, slot) in store[to..to + n].iter_mut().enumerate() {
            let w = cells.width[from + k].load(Ordering::Relaxed);
            let v = <u64 as CellWord>::load::<R>(&cells.value, from + k);
            let bits = w & 0x3F;
            // a bit count or value out of range only comes from a row overwritten while it was copied, which the
            // owner of the cells drops: any valid value does
            *slot = match w >> 6 {
                0 => UN::UN8 {
                    bit_count: U3::new(bits).unwrap_or(U3::zero()),
                    value: v as u8,
                },
                1 => UN::UN16 {
                    bit_count: U4::new(bits).unwrap_or(U4::zero()),
                    value: v as u16,
                },
                2 => UN::UN32 {
                    bit_count: U5::new(bits).unwrap_or(U5::zero()),
                    value: v as u32,
                },
                _ => UN::UN64 {
                    bit_count: U6::new(bits).unwrap_or(U6::zero()),
                    value: v,
                },
            };
        }
    }
}

// SAFETY: atomics only
unsafe impl SeriesCells for IN {
    type Cells<const R: usize> = UnCells<R>;

    fn put<const R: usize>(cells: &UnCells<R>, i: usize, item: &Self) {
        let (width, bits, value) = match *item {
            IN::IN8 { bit_count, value } => (0, bit_count.value(), value as u8 as u64),
            IN::IN16 { bit_count, value } => (1, bit_count.value(), value as u16 as u64),
            IN::IN32 { bit_count, value } => (2, bit_count.value(), value as u32 as u64),
            IN::IN64 { bit_count, value } => (3, bit_count.value(), value as u64),
        };
        cells.width[i].store(width_byte(width, bits), Ordering::Relaxed);
        <u64 as CellWord>::put::<R>(&cells.value, i, value);
    }

    fn copy_rows<const R: usize, const N: usize>(
        cells: &UnCells<R>,
        from: usize,
        store: &mut [IN; N],
        to: usize,
        n: usize,
    ) {
        for (k, slot) in store[to..to + n].iter_mut().enumerate() {
            let w = cells.width[from + k].load(Ordering::Relaxed);
            let v = <u64 as CellWord>::load::<R>(&cells.value, from + k);
            let bits = w & 0x3F;
            *slot = match w >> 6 {
                0 => IN::IN8 {
                    bit_count: U3::new(bits).unwrap_or(U3::zero()),
                    value: v as u8 as i8,
                },
                1 => IN::IN16 {
                    bit_count: U4::new(bits).unwrap_or(U4::zero()),
                    value: v as u16 as i16,
                },
                2 => IN::IN32 {
                    bit_count: U5::new(bits).unwrap_or(U5::zero()),
                    value: v as u32 as i32,
                },
                _ => IN::IN64 {
                    bit_count: U6::new(bits).unwrap_or(U6::zero()),
                    value: v as i64,
                },
            };
        }
    }
}

// SAFETY: no cells
unsafe impl<T: ?Sized> SeriesCells for PhantomData<T> {
    type Cells<const R: usize> = ();

    fn put<const R: usize>(_cells: &(), _i: usize, _item: &Self) {}

    fn copy_rows<const R: usize, const N: usize>(
        _cells: &(),
        _from: usize,
        _store: &mut (),
        _to: usize,
        _n: usize,
    ) {
    }
}

/// The flags, and `X`'s cells for the `Some` items (left as they were at a `None`). Copied out as
/// [SeriesStore::set] does: `X`'s store only at the `Some` items.
// SAFETY: atomics and `X`'s cells, which are valid all zero
unsafe impl<X: SeriesCells> SeriesCells for Option<X> {
    type Cells<const R: usize> = ([AtomicBool; R], X::Cells<R>);

    #[inline]
    fn put<const R: usize>(cells: &Self::Cells<R>, i: usize, item: &Self) {
        cells.0[i].store(item.is_some(), Ordering::Relaxed);
        if let Some(x) = item {
            X::put::<R>(&cells.1, i, x);
        }
    }

    fn copy_rows<const R: usize, const N: usize>(
        cells: &Self::Cells<R>,
        from: usize,
        store: &mut Self::Store<N>,
        to: usize,
        n: usize,
    ) {
        for k in 0..n {
            let some = cells.0[from + k].load(Ordering::Relaxed);
            store.0[to + k] = some;
            if some {
                X::copy_rows::<R, N>(&cells.1, from + k, &mut store.1, to + k, 1);
            }
        }
    }
}

/// One set of cells per element.
// SAFETY: arrays of valid-all-zero cells
unsafe impl<X: SeriesCells, const M: usize> SeriesCells for [X; M] {
    type Cells<const R: usize> = [X::Cells<R>; M];

    #[inline]
    fn put<const R: usize>(cells: &Self::Cells<R>, i: usize, item: &Self) {
        for (c, x) in cells.iter().zip(item) {
            X::put::<R>(c, i, x);
        }
    }

    fn copy_rows<const R: usize, const N: usize>(
        cells: &Self::Cells<R>,
        from: usize,
        store: &mut Self::Store<N>,
        to: usize,
        n: usize,
    ) {
        for (c, s) in cells.iter().zip(store.iter_mut()) {
            X::copy_rows::<R, N>(c, from, s, to, n);
        }
    }
}

macro_rules! tuple {
    ($($t:ident $idx:tt),+) => {
        /// One set of cells per element.
        // SAFETY: tuples of valid-all-zero cells
        unsafe impl<$($t: SeriesCells),+> SeriesCells for ($($t,)+) {
            type Cells<const R: usize> = ($($t::Cells<R>,)+);

            #[inline]
            fn put<const R: usize>(cells: &Self::Cells<R>, i: usize, item: &Self) {
                $($t::put::<R>(&cells.$idx, i, &item.$idx);)+
            }

            fn copy_rows<const R: usize, const N: usize>(
                cells: &Self::Cells<R>,
                from: usize,
                store: &mut Self::Store<N>,
                to: usize,
                n: usize,
            ) {
                $($t::copy_rows::<R, N>(&cells.$idx, from, &mut store.$idx, to, n);)+
            }
        }
    };
}

tuple!(T0 0);
tuple!(T0 0, T1 1);
tuple!(T0 0, T1 1, T2 2);
tuple!(T0 0, T1 1, T2 2, T3 3);
tuple!(T0 0, T1 1, T2 2, T3 3, T4 4);
tuple!(T0 0, T1 1, T2 2, T3 3, T4 4, T5 5);
tuple!(T0 0, T1 1, T2 2, T3 3, T4 4, T5 5, T6 6);
tuple!(T0 0, T1 1, T2 2, T3 3, T4 4, T5 5, T6 6, T7 7);
