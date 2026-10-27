//! `#[cfg_attr(pred, ..)]` on fields with an attribute the derive reads: read as if `pred` holds, as a crate with
//! `shrink_wrap` optional writes them (`#[cfg_attr(feature = "shrink_wrap", default = 7)]`), and a compile error
//! where it doesn't.

use shrink_wrap::prelude::*;

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq))]
struct ConfigV1 {
    rate: u16,
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq))]
struct ConfigV2 {
    rate: u16,
    #[cfg_attr(all(), default = 7)]
    timeout: u16,
    #[cfg_attr(not(any()), default = None)]
    retries: Option<u8>,
}

#[test]
fn defaults_behind_cfg_attr() {
    let mut buf = [0u8; 16];
    let v1 = ConfigV1 { rate: 1000 }
        .to_ww_bytes(&mut buf)
        .unwrap()
        .to_vec();
    assert_eq!(
        ConfigV2::from_ww_bytes(&v1).unwrap(),
        ConfigV2 {
            rate: 1000,
            timeout: 7,
            retries: None,
        }
    );
}

#[test]
fn same_bytes_as_plain_attributes() {
    #[derive_shrink_wrap(borrowed, derive(Debug, PartialEq))]
    struct Plain {
        rate: u16,
        #[default = 7]
        timeout: u16,
        #[default = None]
        retries: Option<u8>,
    }
    let mut a = [0u8; 16];
    let mut b = [0u8; 16];
    let gated = ConfigV2 {
        rate: 5,
        timeout: 6,
        retries: Some(3),
    }
    .to_ww_bytes(&mut a)
    .unwrap()
    .to_vec();
    let plain = Plain {
        rate: 5,
        timeout: 6,
        retries: Some(3),
    }
    .to_ww_bytes(&mut b)
    .unwrap()
    .to_vec();
    assert_eq!(gated, plain);
}
