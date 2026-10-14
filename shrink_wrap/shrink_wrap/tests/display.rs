//! `display` (DER-12): `core::fmt::Display` and `core::error::Error` generated from the first doc line of each
//! variant/struct, `{field}` filled in, `#[display("..")]` overriding it. Checked for borrowed and owned types,
//! unit/named/tuple variants, and the `#[future_version] Unknown(u32)` variant added by SW-47.

use shrink_wrap::prelude::*;

/// Errors every crate can return.
#[derive_shrink_wrap(
    owned(feature = "std"),
    derive(Debug, PartialEq, Eq, Clone),
    display,
    ww_repr = unib32
)]
pub enum IoError<'i> {
    /// Not supported by this device.
    Unsupported,
    /// In use by {by}.
    InUse { by: &'i str },
    /// Hardware fault {code:#x}.
    Fault { code: u32 },
    /// ignored: overridden below
    #[display("wrapping: {0}")]
    Wrapped(u8),
}

/// Chip id {chip_id:#x} did not match.
#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq), display)]
pub struct ChipIdMismatch {
    pub chip_id: u8,
}

fn assert_error<E: core::error::Error>(_: &E) {}

#[test]
fn unit_variant_uses_first_doc_line() {
    assert_eq!(
        IoError::Unsupported.to_string(),
        "Not supported by this device."
    );
    assert_error(&IoError::Unsupported);
}

#[test]
fn named_field_is_interpolated() {
    let e = IoError::InUse { by: "USART2" };
    assert_eq!(e.to_string(), "In use by USART2.");
}

#[test]
fn format_spec_is_kept() {
    let e = IoError::Fault { code: 0xBEEF };
    assert_eq!(e.to_string(), "Hardware fault 0xbeef.");
}

#[test]
fn display_override_wins_over_doc_and_binds_tuple_fields_by_position() {
    let e = IoError::Wrapped(7);
    assert_eq!(e.to_string(), "wrapping: 7");
}

#[test]
fn owned_side_also_implements_display_and_error() {
    let owned = IoErrorOwned::InUse {
        by: "USART2".to_string(),
    };
    assert_eq!(owned.to_string(), "In use by USART2.");
    assert_error(&owned);
}

#[test]
fn future_version_variant_has_its_own_message() {
    // #[future_version] Unknown(u32) is added automatically (SW-47); it implements Display too.
    let unknown = IoError::Unknown(99);
    assert!(unknown.to_string().contains("newer version"));
}

#[test]
fn struct_uses_its_own_doc_and_fields() {
    let e = ChipIdMismatch { chip_id: 0x42 };
    assert_eq!(e.to_string(), "Chip id 0x42 did not match.");
    assert_error(&e);
}
