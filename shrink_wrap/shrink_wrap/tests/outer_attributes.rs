//! Outer attributes next to `#[derive_shrink_wrap]` (`#[derive(..)]`, `#[cfg_attr(..)]`, `#[allow(..)]`) stay on the
//! generated type that has the item's own name, so the derive can sit behind a `cfg_attr` of an optional feature
//! with the plain derives written once.

#![deny(dead_code)]

use shrink_wrap::prelude::*;

/// A borrowed type with an owned mirror: the outer derives are the borrowed type's, `derive_owned` the mirror's.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive_owned(Clone, Debug, PartialEq),
    final_structure
)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(all(), derive(PartialOrd))]
#[cfg_attr(any(), derive(NotADerive))]
pub struct Named<'i> {
    pub name: &'i str,
    pub exp: i8,
}

/// The derive behind a `cfg_attr` that is on: wire format and plain derives.
#[cfg_attr(all(), derive_shrink_wrap(borrowed, final_structure))]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct WithFeature {
    pub a: u8,
}

/// The derive behind a `cfg_attr` that is off: a plain struct with its derives, no wire format.
#[cfg_attr(any(), derive_shrink_wrap(final_structure))]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct WithoutFeature {
    pub a: u8,
}

/// A plain type with `borrowed` and `owned`: one type, the outer derives once.
#[derive_shrink_wrap(borrowed, owned(feature = "std"), ww_repr = u4)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Plain {
    A,
    B(u8),
}

/// `allow` reaches the type: `Unused` is never read, and the file denies dead code.
#[derive_shrink_wrap(borrowed, final_structure)]
#[allow(dead_code)]
struct Unused {
    a: u8,
}

/// A type named `..Owned` with a borrowed form: the outer attributes stay on the type as written.
#[derive_shrink_wrap(borrowed, final_structure)]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LabelOwned {
    pub text: String,
}

fn hash<T: core::hash::Hash>(_: &T) {}

#[test]
fn outer_derives_on_the_type() {
    let a = Named { name: "x", exp: -1 };
    let b = a; // Copy
    assert_eq!(a, b);
    assert!(a <= b); // the cfg_attr that is on
    hash(&a);
    let mut buf = [0u8; 16];
    let bytes = a.to_ww_bytes(&mut buf).unwrap();
    assert_eq!(Named::from_ww_bytes(bytes).unwrap(), a);

    let owned = NamedOwned {
        name: "x".into(),
        exp: -1,
    };
    assert_eq!(owned.clone(), owned);

    let p = Plain::B(3);
    let q = p;
    assert_eq!(p, q);

    let label = LabelOwned { text: "t".into() };
    hash(&label);
    assert_eq!(label.clone(), label);
    let _ = Label { text: "t" };
}

#[test]
fn derive_behind_cfg_attr() {
    let w = WithFeature { a: 7 };
    let mut buf = [0u8; 4];
    assert_eq!(w.to_ww_bytes(&mut buf).unwrap(), &[7]);
    let v = WithoutFeature { a: 7 };
    let copy = v;
    assert_eq!(copy, v);
}
