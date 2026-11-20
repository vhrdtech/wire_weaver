use shrink_wrap::prelude::*;

#[derive_shrink_wrap(
    borrowed,
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, Eq, PartialEq),
    ww_repr = u8,
    future_version = Newer
)]
pub enum DisconnectReason {
    CommanderDropped,
    ApplicationCrash,
    RequestByUser,
    IncompatibleVersion,
    Other(u8),
    Unknown,
    /// The ww hub hands over to a newer one (SRV-30): the client reconnects to the new hub, the device stays.
    /// Appended: an older reader gets `Newer`.
    HubHandover,
    /// The server shuts down (SRV-12): what it had of this client's requests is answered, the rest is not. A
    /// client reconnects when the service is back. Appended: an older reader gets `Newer`.
    Shutdown,
}
