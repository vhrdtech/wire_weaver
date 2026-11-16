//! `CommonError`: the bytes on the wire, its messages, names and the owned form.

use shrink_wrap::prelude::*;
use ww_error::{CommonError, CommonErrorOwned};
use ww_si::Index;

#[test]
fn wire() {
    let mut buf = [0u8; 32];
    let e = CommonError::InUse {
        by: MaxLen::new_unchecked("USART2"),
    };
    let bytes = e.to_ww_bytes(&mut buf).unwrap().to_vec();
    // discriminant of InUse (2), the string, its size at the back
    assert_eq!(bytes, b"\x20USART2\x06");
    assert_eq!(CommonError::from_ww_bytes(&bytes).unwrap(), e);
    let owned = CommonErrorOwned::from_ww_bytes_owned(&bytes).unwrap();
    assert_eq!(
        owned,
        CommonErrorOwned::InUse {
            by: MaxLen::new_unchecked("USART2".into())
        }
    );
}

#[test]
fn display() {
    let s = |e: CommonError<'_>| e.to_string();
    assert_eq!(s(CommonError::Unsupported), "Not supported by this device");
    assert_eq!(
        s(CommonError::InvalidArgument {
            arg: Index::new(1),
            reason: MaxLen::new_unchecked("out of range")
        }),
        "Argument number 1 is invalid: out of range"
    );
    assert_eq!(
        s(CommonError::InvalidArgument {
            arg: Index::new(0),
            reason: MaxLen::new_unchecked("")
        }),
        "Argument number 0 is invalid"
    );
    assert_eq!(
        s(CommonError::InUse {
            by: MaxLen::new_unchecked("USART2")
        }),
        "In use by USART2"
    );
    assert_eq!(
        s(CommonError::NoPower {
            rail: MaxLen::new_unchecked("")
        }),
        "No power"
    );
    assert_eq!(s(CommonError::Fault { code: 0x2a }), "Hardware fault 0x2a");
    assert_eq!(
        s(CommonError::Unknown(12)),
        "Common error 12 from a newer version"
    );
    assert_eq!(
        CommonErrorOwned::Refused {
            reason: MaxLen::new_unchecked("locked".into())
        }
        .to_string(),
        "Refused: locked"
    );
}

#[test]
fn make_owned() {
    let e = CommonError::InUse {
        by: MaxLen::new_unchecked("USART2"),
    };
    assert_eq!(
        e.make_owned(),
        CommonErrorOwned::InUse {
            by: MaxLen::new_unchecked("USART2".into())
        }
    );
    assert_eq!(
        CommonError::Fault { code: 3 }.make_owned(),
        CommonErrorOwned::Fault { code: 3 }
    );
}

/// Every variant is in `DOCS`, in discriminant order, and `name()` finds it there.
#[test]
fn names_and_docs() {
    let all = [
        CommonError::Unsupported,
        CommonError::InvalidArgument {
            arg: Index::new(0),
            reason: MaxLen::new_unchecked(""),
        },
        CommonError::InUse {
            by: MaxLen::new_unchecked(""),
        },
        CommonError::Busy,
        CommonError::WrongState {
            reason: MaxLen::new_unchecked(""),
        },
        CommonError::NoPower {
            rail: MaxLen::new_unchecked(""),
        },
        CommonError::Refused {
            reason: MaxLen::new_unchecked(""),
        },
        CommonError::Timeout,
        CommonError::Fault { code: 0 },
    ];
    assert_eq!(all.len(), CommonError::DOCS.len());
    for e in &all {
        let (name, doc) = CommonError::DOCS[e.discriminant() as usize];
        assert_eq!(e.name(), name);
        assert_eq!(e.make_owned().name(), name);
        assert!(!doc.is_empty(), "{name} has no doc line");
        // the names are the variants' Rust names
        assert!(format!("{e:?}").starts_with(name), "{e:?} vs {name}");
    }
    assert_eq!(CommonError::Unknown(12).name(), "Unknown");
    assert_eq!(
        CommonError::DOCS[3],
        ("Busy", "Busy with another operation, try again later")
    );
}
