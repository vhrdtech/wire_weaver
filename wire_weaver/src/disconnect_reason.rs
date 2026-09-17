use shrink_wrap::prelude::*;

#[derive_shrink_wrap(borrowed, ww_repr = u8, derive(Copy, Clone, Debug, Eq, PartialEq), cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)))]
pub enum DisconnectReason {
    CommanderDropped,
    ApplicationCrash,
    RequestByUser,
    IncompatibleVersion,
    Other(u8),
    Unknown,
}
