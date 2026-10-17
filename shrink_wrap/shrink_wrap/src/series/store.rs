//! [SeriesStore]: items kept as a struct of arrays until they are written as a [Series](super::Series) (SW-62).

use crate::un::{IN, U3, UN};
use crate::{Nibble, RefVec, UNib32, UVlq32, UVlq64};
use core::marker::PhantomData;

/// Room for `N` items of `Self` as a struct of arrays, one array per leaf field: what a device keeps while it
/// collects the items of a batch, before writing them as one [Series](super::Series) (API-41).
///
/// A struct's store is the stores of its fields, so there is no padding between fields of different sizes
/// (`Timed<Ampere>` with four `NumericValue` variants takes 12 bytes per item where the item itself takes 24); an
/// `Option<X>` is an array of flags and `X`'s store; an enum is an array of discriminants (`u8`, `u16` or `u32`,
/// whichever holds the largest) and the stores of its data variants' fields; numbers, bools and strings are arrays
/// of themselves. `#[derive_shrink_wrap(.., series)]` implements it for structs and enums whose fields all have it
/// (a type with a `#[series(opaque)]` field has none, `series(no_store)` leaves it out to write one by hand).
///
/// [set](Self::set) and [get](Self::get) panic on an index of `N` or more, like an array does. A fresh store
/// ([empty_store](Self::empty_store)) holds zeros, `false`, empty strings, `None` and an enum's first variant at
/// every index, so `get` of an index never `set` gives a value, not garbage.
///
/// ```
/// use shrink_wrap::prelude::*;
/// use shrink_wrap::series::SeriesStore;
///
/// #[derive_shrink_wrap(borrowed, series, final_structure, derive(Clone, Copy, Debug, PartialEq))]
/// struct Sample {
///     ok: bool,
///     value: i32,
/// }
///
/// let mut store = Sample::empty_store::<32>();
/// Sample::set(&mut store, 3, &Sample { ok: true, value: -7 });
/// assert_eq!(Sample::get(&store, 3), Sample { ok: true, value: -7 });
/// assert_eq!(core::mem::size_of_val(&store), 32 * 5); // 8 bytes per item as `[Sample; 32]`
/// ```
pub trait SeriesStore: Sized {
    /// The arrays of `N` items.
    type Store<const N: usize>;

    /// A store with the empty value at every index.
    fn empty_store<const N: usize>() -> Self::Store<N>;

    /// Puts `item` at index `i`.
    fn set<const N: usize>(store: &mut Self::Store<N>, i: usize, item: &Self);

    /// The item at index `i`.
    fn get<const N: usize>(store: &Self::Store<N>, i: usize) -> Self;
}

/// A leaf kept as an array of itself.
macro_rules! copy_leaf {
    ($($ty:ty = $empty:expr),* $(,)?) => {
        $(
            impl SeriesStore for $ty {
                type Store<const N: usize> = [$ty; N];

                fn empty_store<const N: usize>() -> [$ty; N] {
                    [$empty; N]
                }

                fn set<const N: usize>(store: &mut [$ty; N], i: usize, item: &Self) {
                    store[i] = *item;
                }

                fn get<const N: usize>(store: &[$ty; N], i: usize) -> Self {
                    store[i]
                }
            }
        )*
    };
}

copy_leaf!(
    u8 = 0,
    u16 = 0,
    u32 = 0,
    u64 = 0,
    u128 = 0,
    i8 = 0,
    i16 = 0,
    i32 = 0,
    i64 = 0,
    i128 = 0,
    f32 = 0.0,
    f64 = 0.0,
    bool = false,
    Nibble = Nibble::new_masked(0),
    UNib32 = UNib32(0),
    UVlq32 = UVlq32(0),
    UVlq64 = UVlq64(0),
    UN = UN::UN8 {
        bit_count: U3::zero(),
        value: 0
    },
    IN = IN::IN8 {
        bit_count: U3::zero(),
        value: 0
    },
);

macro_rules! un_leaves {
    ($prefix:ident: $($bits:literal),*) => {
        paste::paste! {
            copy_leaf!($(crate::un::[<$prefix $bits>] = crate::un::[<$prefix $bits>]::zero()),*);
        }
    };
}

un_leaves!(U: 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26,
    27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54,
    55, 56, 57, 58, 59, 60, 61, 62, 63, 64);
un_leaves!(I: 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26,
    27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54,
    55, 56, 57, 58, 59, 60, 61, 62, 63, 64);

impl<'i> SeriesStore for &'i str {
    type Store<const N: usize> = [&'i str; N];

    fn empty_store<const N: usize>() -> Self::Store<N> {
        [""; N]
    }

    fn set<const N: usize>(store: &mut Self::Store<N>, i: usize, item: &Self) {
        store[i] = item;
    }

    fn get<const N: usize>(store: &Self::Store<N>, i: usize) -> Self {
        store[i]
    }
}

impl<'i> SeriesStore for &'i [u8] {
    type Store<const N: usize> = [&'i [u8]; N];

    fn empty_store<const N: usize>() -> Self::Store<N> {
        [&[]; N]
    }

    fn set<const N: usize>(store: &mut Self::Store<N>, i: usize, item: &Self) {
        store[i] = item;
    }

    fn get<const N: usize>(store: &Self::Store<N>, i: usize) -> Self {
        store[i]
    }
}

impl<'i> SeriesStore for RefVec<'i, u8> {
    type Store<const N: usize> = [RefVec<'i, u8>; N];

    fn empty_store<const N: usize>() -> Self::Store<N> {
        [RefVec::new_bytes(&[]); N]
    }

    fn set<const N: usize>(store: &mut Self::Store<N>, i: usize, item: &Self) {
        store[i] = *item;
    }

    fn get<const N: usize>(store: &Self::Store<N>, i: usize) -> Self {
        store[i]
    }
}

/// Owned leaves: an array of values, cloned in and out.
#[cfg(feature = "std")]
macro_rules! clone_leaf {
    ($($ty:ty),*) => {
        $(
            impl SeriesStore for $ty {
                type Store<const N: usize> = [$ty; N];

                fn empty_store<const N: usize>() -> Self::Store<N> {
                    core::array::from_fn(|_| <$ty>::new())
                }

                fn set<const N: usize>(store: &mut Self::Store<N>, i: usize, item: &Self) {
                    store[i].clone_from(item);
                }

                fn get<const N: usize>(store: &Self::Store<N>, i: usize) -> Self {
                    store[i].clone()
                }
            }
        )*
    };
}

#[cfg(feature = "std")]
clone_leaf!(String, Vec<u8>);

/// No arrays: a `PhantomData` field takes no space.
impl<T: ?Sized> SeriesStore for PhantomData<T> {
    type Store<const N: usize> = ();

    fn empty_store<const N: usize>() {}

    fn set<const N: usize>(_store: &mut (), _i: usize, _item: &Self) {}

    fn get<const N: usize>(_store: &(), _i: usize) -> Self {
        PhantomData
    }
}

/// The flags, and `X`'s store for the `Some` items (left as it was at a `None`).
impl<X: SeriesStore> SeriesStore for Option<X> {
    type Store<const N: usize> = ([bool; N], X::Store<N>);

    fn empty_store<const N: usize>() -> Self::Store<N> {
        ([false; N], X::empty_store::<N>())
    }

    fn set<const N: usize>(store: &mut Self::Store<N>, i: usize, item: &Self) {
        store.0[i] = item.is_some();
        if let Some(x) = item {
            X::set::<N>(&mut store.1, i, x);
        }
    }

    fn get<const N: usize>(store: &Self::Store<N>, i: usize) -> Self {
        store.0[i].then(|| X::get::<N>(&store.1, i))
    }
}

/// One store per element.
impl<X: SeriesStore, const M: usize> SeriesStore for [X; M] {
    type Store<const N: usize> = [X::Store<N>; M];

    fn empty_store<const N: usize>() -> Self::Store<N> {
        core::array::from_fn(|_| X::empty_store::<N>())
    }

    fn set<const N: usize>(store: &mut Self::Store<N>, i: usize, item: &Self) {
        for (s, x) in store.iter_mut().zip(item) {
            X::set::<N>(s, i, x);
        }
    }

    fn get<const N: usize>(store: &Self::Store<N>, i: usize) -> Self {
        core::array::from_fn(|j| X::get::<N>(&store[j], i))
    }
}

macro_rules! tuple {
    ($($t:ident $idx:tt),+) => {
        /// One store per element.
        impl<$($t: SeriesStore),+> SeriesStore for ($($t,)+) {
            type Store<const N: usize> = ($($t::Store<N>,)+);

            fn empty_store<const N: usize>() -> Self::Store<N> {
                ($($t::empty_store::<N>(),)+)
            }

            fn set<const N: usize>(store: &mut Self::Store<N>, i: usize, item: &Self) {
                $($t::set::<N>(&mut store.$idx, i, &item.$idx);)+
            }

            fn get<const N: usize>(store: &Self::Store<N>, i: usize) -> Self {
                ($($t::get::<N>(&store.$idx, i),)+)
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
