use wire_weaver::prelude::*;

use crate::Error;

/// Boot state and the switch between images.
///
/// Implemented by the app (the minimum is `BootControl::reboot` with [BootTarget::Updater], for a node that
/// receives updates elsewhere) and by an updater (bootloader with a transport, recovery app), where it at least
/// reboots back into the app.
#[ww_trait]
#[claim(exclusive)]
pub trait BootControl {
    /// Where the device stands between two updates.
    fn boot_state() -> BootState;

    /// The running image works: keep it.
    ///
    /// Only meaningful in [BootState::Trial]; in any other state it does nothing and returns `Ok`. An image that
    /// never confirms is reverted at the next reset, so the app should confirm only after it has checked what
    /// matters to it (its peripherals, its link to the host, its config).
    fn confirm() -> Result<(), Error>;

    /// Give up the running trial image now, or un-stage a staged one: the previous image runs after the next reset.
    ///
    /// [Error::Unsupported] on profiles without a previous image ([Profile::InPlace]), [Error::WrongState] outside
    /// [BootState::Trial] and [BootState::Staged].
    #[errors(Unsupported, WrongState)]
    fn revert() -> Result<(), Error>;

    /// Reset into the app or into the updater.
    ///
    /// The reply is sent before the reset, so the host knows the request arrived; the node resets shortly after
    /// (implementations wait long enough for the reply to leave, typically a few tens of milliseconds). The host then
    /// reconnects to the same device by serial number; what answers may be a different firmware with a different API
    /// (an updater after [BootTarget::Updater]).
    ///
    /// [BootTarget::App] after a successful `Stage::finish` is what starts the update: the bootloader switches to the
    /// staged image (A/B) or applies the stored plan (in place).
    ///
    /// [Error::Unsupported] into [BootTarget::Updater] on an A/B node (there is no separate updater to reboot into).
    #[errors(Unsupported)]
    fn reboot(into: BootTarget) -> Result<(), Error>;
}

/// Where the device stands between two updates.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    self_describing,
    ww_repr = nib
)]
pub enum BootState {
    /// The running image is confirmed. An update may be started.
    Confirmed,
    /// The last update did not confirm and was reverted. An update may be started.
    Reverted,
    /// An image is staged (A/B) and switches in at the next reset.
    Staged,
    /// The running image is on its trial boot: it must `BootControl::confirm`, or the next reset reverts it.
    Trial,
    /// A complete, verified plan is in the plan area and is applied in place at the next reset (mode 2).
    PlanStored,
    /// In place, an update is under way and the app slot does not hold a bootable image (mode 3, in the updater).
    Updating,
}

impl BootState {
    /// True when a new update may be started: not while the inactive slot holds the image to switch to or to revert
    /// to, and not while a stored plan waits to be applied.
    pub fn may_stage(self) -> bool {
        matches!(
            self,
            BootState::Confirmed | BootState::Reverted | BootState::Updating
        )
    }
}

/// Where `BootControl::reboot` goes.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    self_describing,
    ww_repr = nib
)]
pub enum BootTarget {
    /// The app: the normal reset. Switches to a staged image or applies a stored plan if there is one.
    App,
    /// The updater (bootloader with a transport, or recovery app), to receive an update there (mode 3).
    Updater,
}
