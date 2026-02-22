use hex_literal::hex;
use shrink_wrap::prelude::*;

#[derive_shrink_wrap]
#[owned = "std"]
#[derive(Debug, PartialEq)]
struct Linked<'i> {
    a: u8,
    next: Option<RefBox<'i, Linked<'i>>>,
}

#[test]
fn box_ref_box() {
    let linked = Linked {
        a: 1,
        next: Some(RefBox::Ref {
            value: &Linked { a: 2, next: None },
        }),
    };

    let mut buf = [0u8; 64];
    let bytes = linked.to_ww_bytes(&mut buf).unwrap();
    assert_eq!(bytes, hex!("01 80 02 00 02"));

    let linked_des = Linked::from_ww_bytes(bytes).unwrap();
    assert_eq!(linked_des, linked);

    let linked_owned = LinkedOwned {
        a: 1,
        next: Some(Box::new(LinkedOwned { a: 2, next: None })),
    };
    let mut buf2 = [0u8; 64];
    let bytes_from_owned = linked_owned.to_ww_bytes(&mut buf2).unwrap();
    assert_eq!(bytes_from_owned, bytes);

    let linked_des_to_owned = LinkedOwned::from_ww_bytes(bytes_from_owned).unwrap();
    assert_eq!(linked_des_to_owned, linked_owned);
}
