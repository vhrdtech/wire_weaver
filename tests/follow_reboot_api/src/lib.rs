//! The lean app of `tests/follow_reboot` (CLT-21, CLT-34): a node that reports the app image's version and reboots
//! between the app and its updater. Stands in for `ww_dfu::UpdateNode` + `ww_dfu::BootControl` (R7): only the
//! `reboot(into)` shape the client's `follow` logic cares about, not the real API's richer node info.
#![no_std]

use ww_api::prelude::*;

#[ww_api_root]
pub trait FollowRebootApp {
    ww_impl!(node: Node);
    ww_impl!(boot: Boot);
}

/// Who this node is: just the app's image version, for the test to check after a reboot comes back.
#[ww_trait]
#[effect(edit)]
pub trait Node {
    fn node() -> NodeInfo;
}

/// Reboots between the app and its updater.
#[ww_trait]
#[effect(edit)]
pub trait Boot {
    fn reboot(into: BootTarget) -> Result<(), Error>;
}

/// What [Node::node] reports.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Clone, Copy, Debug, PartialEq, Eq)
)]
pub struct NodeInfo {
    /// `None` when there is no valid image (an updater after a cut).
    pub app: Option<ImageInfo>,
}

/// What a node knows of the image in its app slot.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Clone, Copy, Debug, PartialEq, Eq)
)]
pub struct ImageInfo {
    pub version: ImageVersion,
}

/// An image's release version.
#[derive_shrink_wrap(
    borrowed,
    transparent,
    owned(feature = "std"),
    derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)
)]
pub struct ImageVersion(pub u32);

/// Where [Boot::reboot] goes.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Clone, Copy, Debug, PartialEq, Eq),
    self_describing,
    ww_repr = nib
)]
pub enum BootTarget {
    /// The app: the normal reset.
    App,
    /// The updater, to receive an update there.
    Updater,
}

/// Why [Boot::reboot] refused.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Clone, Copy, Debug, PartialEq, Eq),
    self_describing,
    ww_repr = nib
)]
pub enum Error {
    Unsupported,
}

api_error!(Error);
