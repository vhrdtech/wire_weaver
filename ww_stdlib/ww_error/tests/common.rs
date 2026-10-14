//! `#[common]` variants: `From<CommonError>`, `HasCommon`, and the bytes on the wire.

use shrink_wrap::prelude::*;
use ww_error::{CommonError, CommonErrorOwned, HasCommon};

#[derive_shrink_wrap(owned, derive(Debug, PartialEq, Eq, Clone), ww_repr = unib32)]
enum I2cError<'i> {
    #[common]
    Common(CommonError<'i>),
    Nack,
    Arbitration {
        lost_at: u8,
    },
}

fn helper() -> Result<(), CommonError<'static>> {
    Err(CommonError::InUse {
        by: MaxLen::new_unchecked("USART2"),
    })
}

fn handler() -> Result<(), I2cError<'static>> {
    helper()?;
    Ok(())
}

#[test]
fn from_and_has_common() {
    let e = handler().unwrap_err();
    assert_eq!(
        e.common(),
        Some(&CommonError::InUse {
            by: MaxLen::new_unchecked("USART2")
        })
    );
    assert_eq!(I2cError::Nack.common(), None);
    assert_eq!(I2cError::Arbitration { lost_at: 3 }.common(), None);
    assert_eq!(CommonError::Busy.common(), Some(&CommonError::Busy));
}

#[test]
fn owned() {
    let e: I2cErrorOwned = CommonErrorOwned::Timeout.into();
    assert_eq!(e, I2cErrorOwned::Common(CommonErrorOwned::Timeout));
    assert_eq!(e.common(), Some(&CommonErrorOwned::Timeout));
    // generic host code over any crate's error
    fn is_retryable<E: HasCommon<Common = CommonErrorOwned>>(e: &E) -> bool {
        matches!(
            e.common(),
            Some(CommonErrorOwned::Busy | CommonErrorOwned::Timeout)
        )
    }
    assert!(is_retryable(&e));
    assert!(!is_retryable(&I2cErrorOwned::Nack));
}

#[test]
fn wire() {
    let mut buf = [0u8; 32];
    let e = I2cError::Common(CommonError::InUse {
        by: MaxLen::new_unchecked("USART2"),
    });
    let bytes = e.to_ww_bytes(&mut buf).unwrap().to_vec();
    // discriminants of Common (0) and InUse (2), the string, sizes at the back
    assert_eq!(bytes, b"\x00\x20USART2\x06\x09");
    assert_eq!(I2cError::from_ww_bytes(&bytes).unwrap(), e);
    let owned = I2cErrorOwned::from_ww_bytes_owned(&bytes).unwrap();
    assert_eq!(
        owned,
        I2cErrorOwned::Common(CommonErrorOwned::InUse {
            by: MaxLen::new_unchecked("USART2".into())
        })
    );
}

#[test]
fn display() {
    let s = |e: CommonError<'_>| e.to_string();
    assert_eq!(s(CommonError::Unsupported), "Not supported by this device");
    assert_eq!(
        s(CommonError::InvalidArgument {
            arg: 1,
            reason: MaxLen::new_unchecked("out of range")
        }),
        "Argument number 1 is invalid: out of range"
    );
    assert_eq!(
        s(CommonError::InvalidArgument {
            arg: 0,
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
