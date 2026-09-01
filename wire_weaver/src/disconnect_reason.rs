use shrink_wrap::prelude::*;

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive_shrink_wrap]
#[ww_repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum DisconnectReason {
    CommanderDropped,
    RequestByUser,
    IncompatibleVersion,
    Other(u8),
    Unknown,
}
