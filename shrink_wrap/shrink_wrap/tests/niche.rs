//! `niche` (DER-24): an enum without `#[repr]`, so Rust keeps the tag in a field's niche; the same bytes as the enum
//! with its repr.

use shrink_wrap::prelude::*;

/// A one-byte handle with spare values: the niche.
#[derive_shrink_wrap(borrowed, owned, sized, series(cells), ww_repr = u4, derive(Debug, PartialEq, Clone, Copy))]
enum Handle {
    A,
    B,
    C,
}

#[derive_shrink_wrap(
    borrowed,
    owned,
    self_describing,
    series(cells),
    ww_repr = unib32,
    niche,
    derive(Debug, PartialEq, Clone, Copy)
)]
enum Packed {
    Unit,
    Milli,
    Kilo,
    Exact(Handle),
}

#[derive_shrink_wrap(
    borrowed,
    owned,
    self_describing,
    series(cells),
    ww_repr = unib32,
    derive(Debug, PartialEq, Clone, Copy)
)]
enum Tagged {
    Unit,
    Milli,
    Kilo,
    Exact(Handle),
}

/// A named field: one dataful variant, so the niche holds the other tags.
#[derive_shrink_wrap(borrowed, owned, self_describing, ww_repr = unib32, niche, derive(Debug, PartialEq))]
enum NamedField {
    Off,
    On { handle: Handle },
}

/// `niche` with a catch-all: the catch-all holds its discriminant.
#[derive_shrink_wrap(borrowed, owned, ww_repr = unib32, niche, derive(Debug, PartialEq))]
enum Evolving {
    First,
    Second(u8),
    #[future_version]
    Unknown(u32),
}

fn pairs() -> [(Packed, Tagged); 6] {
    [
        (Packed::Unit, Tagged::Unit),
        (Packed::Milli, Tagged::Milli),
        (Packed::Kilo, Tagged::Kilo),
        (Packed::Exact(Handle::A), Tagged::Exact(Handle::A)),
        (Packed::Exact(Handle::C), Tagged::Exact(Handle::C)),
        (Packed::Exact(Handle::B), Tagged::Exact(Handle::B)),
    ]
}

#[test]
fn the_tag_sits_in_the_niche() {
    assert_eq!(size_of::<Packed>(), 1);
    assert_eq!(size_of::<Option<Packed>>(), 1);
    assert!(size_of::<Tagged>() > 1);
    assert_eq!(size_of::<NamedField>(), 1);
    assert_eq!(NamedField::On { handle: Handle::C }.discriminant(), 1);
}

#[test]
fn same_bytes_and_discriminants_as_with_a_repr() {
    for (p, t) in pairs() {
        let mut a = [0u8; 16];
        let mut b = [0u8; 16];
        let a = p.to_ww_bytes(&mut a).unwrap();
        let b = t.to_ww_bytes(&mut b).unwrap();
        assert_eq!(a, b, "{p:?}");
        assert_eq!(p.discriminant(), t.discriminant());
        assert_eq!(Packed::from_ww_bytes(a).unwrap(), p);
        assert_eq!(Packed::from_ww_bytes_owned(a).unwrap(), p);
    }
    let packed: Vec<Packed> = pairs().iter().map(|(p, _)| *p).collect();
    let tagged: Vec<Tagged> = pairs().iter().map(|(_, t)| *t).collect();
    let a = Series::new(&packed).to_ww_bytes_owned().unwrap();
    assert_eq!(a, Series::new(&tagged).to_ww_bytes_owned().unwrap());
    assert_eq!(
        shrink_wrap::series::SeriesOwned::<Packed>::from_ww_bytes_owned(&a)
            .unwrap()
            .0,
        packed
    );
}

#[test]
fn a_catch_all_keeps_its_discriminant() {
    assert_eq!(Evolving::First.discriminant(), 0);
    assert_eq!(Evolving::Second(7).discriminant(), 1);
    assert_eq!(Evolving::Unknown(9).discriminant(), 9);
}
