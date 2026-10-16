//! `#[omit_if_default]` (SW-58): fields appended with `#[default = ..]` that are not written while they have their
//! default value, so a value that doesn't use them has the same bytes as with the older version of the type.

use shrink_wrap::prelude::*;

#[derive_shrink_wrap(borrowed, derive(Clone, Debug, PartialEq))]
struct ItemV1<'i> {
    name: &'i str,
    on: bool,
}

#[derive_shrink_wrap(borrowed, derive(Clone, Debug, PartialEq))]
struct ItemV2<'i> {
    name: &'i str,
    on: bool,
    #[default = None]
    #[omit_if_default]
    slot: Option<u16>,
    #[default = false]
    #[omit_if_default]
    sparse: bool,
}

/// An item followed by more data, the item's end has to be found without the omitted fields.
#[derive_shrink_wrap(borrowed, derive(Clone, Debug, PartialEq))]
struct ListV1<'i> {
    items: RefVec<'i, ItemV1<'i>>,
    after: u8,
}

#[derive_shrink_wrap(borrowed, derive(Clone, Debug, PartialEq))]
struct ListV2<'i> {
    items: RefVec<'i, ItemV2<'i>>,
    after: u8,
}

#[derive_shrink_wrap(borrowed, derive(Clone, Debug, PartialEq), ww_repr = u4)]
enum KindV1 {
    Method { args: u8 },
    Stream,
}

#[derive_shrink_wrap(borrowed, derive(Clone, Debug, PartialEq), ww_repr = u4)]
enum KindV2 {
    Method {
        args: u8,
        #[default = None]
        #[omit_if_default]
        errors: Option<u8>,
    },
    Stream,
}

fn bytes<T: SerializeShrinkWrap>(value: &T) -> Vec<u8> {
    let mut buf = [0u8; 128];
    value.to_ww_bytes(&mut buf).unwrap().to_vec()
}

fn item(slot: Option<u16>, sparse: bool) -> ItemV2<'static> {
    ItemV2 {
        name: "ab",
        on: true,
        slot,
        sparse,
    }
}

#[test]
fn defaults_are_not_written() {
    let v1 = ItemV1 {
        name: "ab",
        on: true,
    };
    assert_eq!(bytes(&item(None, false)), bytes(&v1));
}

#[test]
fn a_field_that_is_not_default_is_written_with_the_ones_before_it() {
    for (slot, sparse) in [(Some(5), false), (None, true), (Some(0), true)] {
        let v2 = item(slot, sparse);
        let b = bytes(&v2);
        assert_ne!(b, bytes(&item(None, false)));
        assert_eq!(ItemV2::from_ww_bytes(&b).unwrap(), v2);
        // an older reader skips them
        let v1 = ItemV1::from_ww_bytes(&b).unwrap();
        assert_eq!((v1.name, v1.on), ("ab", true));
    }
}

#[test]
fn omitted_fields_read_as_their_defaults() {
    let b = bytes(&item(None, false));
    assert_eq!(ItemV2::from_ww_bytes(&b).unwrap(), item(None, false));
}

#[test]
fn nested_values_keep_their_bounds() {
    let v1_items = [
        ItemV1 {
            name: "ab",
            on: true,
        },
        ItemV1 {
            name: "ab",
            on: true,
        },
    ];
    let v1 = ListV1 {
        items: RefVec::Slice { slice: &v1_items },
        after: 0xA5,
    };
    let v2_items = [item(None, false), item(None, false)];
    let v2 = ListV2 {
        items: RefVec::Slice { slice: &v2_items },
        after: 0xA5,
    };
    assert_eq!(bytes(&v2), bytes(&v1));

    let mixed_items = [item(Some(9), false), item(None, false), item(None, true)];
    let mixed = ListV2 {
        items: RefVec::Slice {
            slice: &mixed_items,
        },
        after: 0xA5,
    };
    let b = bytes(&mixed);
    let read = ListV2::from_ww_bytes(&b).unwrap();
    let read_items: Vec<_> = read.items.iter().map(|i| i.unwrap()).collect();
    assert_eq!(read_items, mixed_items);
    assert_eq!(read.after, 0xA5);
}

#[test]
fn enum_variant_fields() {
    let v1 = KindV1::Method { args: 3 };
    let v2 = KindV2::Method {
        args: 3,
        errors: None,
    };
    assert_eq!(bytes(&v2), bytes(&v1));
    assert_eq!(KindV2::from_ww_bytes(&bytes(&v2)).unwrap(), v2);
    let with_errors = KindV2::Method {
        args: 3,
        errors: Some(1),
    };
    let b = bytes(&with_errors);
    assert_ne!(b, bytes(&v1));
    assert_eq!(KindV2::from_ww_bytes(&b).unwrap(), with_errors);
    assert_eq!(KindV1::from_ww_bytes(&b).unwrap(), v1);
}

#[derive_shrink_wrap(borrowed, derive(Clone, Debug, PartialEq))]
struct KindsV2<'i> {
    kinds: RefVec<'i, KindV2>,
    after: u8,
}

#[derive_shrink_wrap(borrowed, derive(Clone, Debug, PartialEq))]
struct KindsV1<'i> {
    kinds: RefVec<'i, KindV1>,
    after: u8,
}

#[test]
fn enum_variants_in_a_list_keep_their_bounds() {
    let v2_kinds = [
        KindV2::Method {
            args: 1,
            errors: None,
        },
        KindV2::Stream,
        KindV2::Method {
            args: 2,
            errors: None,
        },
    ];
    let v2 = KindsV2 {
        kinds: RefVec::Slice { slice: &v2_kinds },
        after: 0x5A,
    };
    let v1_kinds = [
        KindV1::Method { args: 1 },
        KindV1::Stream,
        KindV1::Method { args: 2 },
    ];
    let v1 = KindsV1 {
        kinds: RefVec::Slice { slice: &v1_kinds },
        after: 0x5A,
    };
    assert_eq!(bytes(&v2), bytes(&v1));

    let mixed_kinds = [
        KindV2::Method {
            args: 1,
            errors: Some(4),
        },
        KindV2::Method {
            args: 2,
            errors: None,
        },
    ];
    let mixed = KindsV2 {
        kinds: RefVec::Slice {
            slice: &mixed_kinds,
        },
        after: 0x5A,
    };
    let read = KindsV2::from_ww_bytes(&bytes(&mixed)).unwrap();
    let read_kinds: Vec<_> = read.kinds.iter().map(|k| k.unwrap()).collect();
    assert_eq!(read_kinds, mixed_kinds);
    assert_eq!(read.after, 0x5A);
}
