#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;

/// Represents one unique ID associated with a device.
#[ww_trait]
pub trait Uid {
    property!(ro value: [u8]);
    property!(ro kind: UidKind<'i>);
    property!(ro source: UidSource<'i>);
}

#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Clone, Debug),
    ww_repr = unib32
)]
enum UidKind<'i> {
    UniqueSequence,
    MACAddress,
    UUID,
    Other(&'i str),
}

#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Clone, Debug),
    ww_repr = unib32
)]
enum UidSource<'i> {
    InternalIC,
    ExternalIC,
    Firmware,
    HardwareStraps,
    Other(&'i str),
}
