use shrink_wrap::prelude::*;

#[derive_shrink_wrap(
    borrowed,
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, Eq, PartialEq),
    ww_repr = u8
)]
pub enum DisconnectReason {
    CommanderDropped,
    ApplicationCrash,
    RequestByUser,
    IncompatibleVersion,
    Other(u8),
    Unknown,
}
