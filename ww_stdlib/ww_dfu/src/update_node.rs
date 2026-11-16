use ww_api::prelude::*;

use crate::BootState;

/// Longest product name in [NodeInfo::product], in bytes. Raising it is compatible.
pub const MAX_PRODUCT_LEN: usize = 32;

/// Who this node is and what it can do. Every node that takes part in updates implements it: the app, the
/// bootloader if it has a transport, a recovery app.
///
/// This is what a host reads first, and on slow links often the only thing it reads before an update: together with
/// [NodeInfo::manifest_digest] it tells the host which plan to send.
#[ww_trait]
pub trait UpdateNode {
    /// Identity, versions and state of this node, and which of the other update traits it implements.
    fn node() -> NodeInfo<'i>;
}

/// What `UpdateNode::node` reports.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Clone, Debug)
)]
pub struct NodeInfo<'i> {
    /// Which firmware is answering.
    pub role: Role,
    /// How this device switches between images.
    pub profile: Profile,
    /// Product name, as the release's manifest names it; the host uses it to pick the release line.
    pub product: MaxLen<&'i str, MAX_PRODUCT_LEN>,
    /// Flash geometry as the protocol spec names it (chip line, slot layout, chunk size). A plan is made for one
    /// geometry and is refused on any other.
    pub geometry: GeometryId,
    /// How this node checks the seal of an image: what a host seals a new image with.
    pub seal: Seal,
    /// The image in the app slot, from its descriptor; `None` when there is no valid image (an updater after a cut in
    /// mode 3).
    pub app: Option<ImageInfo>,
    /// The anti-rollback floor: an image whose [ImageInfo::security_counter] is below it is refused
    /// ([Error::BelowFloor]). Only ever goes up; a host seals a new image with a counter at or above it.
    pub security_floor: SecurityCounter,
    /// Digest of the app slot's manifest, if this node has it at hand (cached, or computed at boot); `None` means
    /// "read it with `Inventory::manifest_digest`".
    pub manifest_digest: Option<[u8; 8]>,
    /// Where the device stands between two updates.
    pub boot_state: BootState,
    /// How the last update ended, for the host's log and a rollout's statistics.
    pub last_result: LastResult,
    /// Which update traits this node implements.
    pub offers: Offers,
}

/// What a node knows of an image from its signed descriptor (the chunky protocol spec's descriptor body).
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct ImageInfo {
    /// The product the image is for. A node refuses a plan whose image is for another product, so a host seals for
    /// this one.
    pub product: ProductId,
    /// The image's release version.
    pub version: ImageVersion,
    /// The image's anti-rollback counter, at or above the node's [NodeInfo::security_floor].
    pub security_counter: SecurityCounter,
}

/// A flash geometry, as the chunky protocol spec names it (`FlashGeometry::id`): the chip line, the slot layout and
/// the chunk size. Only equality means anything.
#[derive_shrink_wrap(
    borrowed,
    transparent,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq, Hash)
)]
pub struct GeometryId(pub u32);

/// A product, as an image's descriptor names it. Only equality means anything.
#[derive_shrink_wrap(
    borrowed,
    transparent,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq, Hash)
)]
pub struct ProductId(pub u32);

/// An image's release version, from its descriptor: informational, the host compares it, the device only reports it.
#[derive_shrink_wrap(
    borrowed,
    transparent,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)
)]
pub struct ImageVersion(pub u32);

/// An image's anti-rollback counter, from its descriptor, and the node's floor on it: an image whose counter is below
/// the floor is refused. Not the release version: several releases may share one counter.
#[derive_shrink_wrap(
    borrowed,
    transparent,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)
)]
pub struct SecurityCounter(pub u32);

/// How a node checks the seal of an image's descriptor (the chunky protocol spec's seal schemes).
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    self_describing,
    ww_repr = nib
)]
pub enum Seal {
    /// An integrity hash, no signature: the node accepts any image that is intact (an explicit opt-out for small
    /// flash or a device whose flash anyone can write anyway).
    Unsigned,
    /// An Ed25519 signature by the release key the node trusts.
    Ed25519,
}

/// Which firmware is answering.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    self_describing,
    ww_repr = nib
)]
pub enum Role {
    /// The application.
    App,
    /// The bootloader, with a transport of its own (mode 3).
    Bootloader,
    /// A recovery app at its own address that updates the app in place (mode 3).
    Recovery,
}

/// How a device switches between images; decides which [Target]s make sense.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    self_describing,
    ww_repr = nib
)]
pub enum Profile {
    /// Two slots: the update is written to the inactive one, which then boots on trial and is kept or reverted
    /// (mode 1). How the switch happens is given.
    AB(SlotSwitch),
    /// One app slot, updated in place: from a stored plan by the bootloader (mode 2) or live by an updater (mode 3).
    /// No revert.
    InPlace,
}

/// How an A/B device switches to the other slot. Both work the same for the host; they differ in the time the first
/// boot after an update takes and in flash wear.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    self_describing,
    ww_repr = nib
)]
pub enum SlotSwitch {
    /// The bootloader copies or swaps the slots' contents page by page, so the image always runs from the same place
    /// (embassy-boot on single-bank parts such as STM32 F1 and G0). The first boot after an update, and a revert,
    /// take the time of rewriting the slot.
    Copy,
    /// Hardware or a boot ROM maps the chosen slot to the address the image runs from; nothing is copied and the
    /// switch is instant. Examples: dual-bank flash with bank swap (STM32 H7, G4, L4), the RP2350 boot ROM (a
    /// partition table with an A/B pair, address translation, try-before-you-buy), ESP32 OTA partitions, nRF
    /// dual-bank.
    Remap,
}

/// Which update traits a node implements. Lets a host decide what to do after a single call, which matters on slow
/// links where introspection is expensive.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct Offers {
    /// `BootControl` is here.
    pub boot_control: bool,
    /// `Inventory` is here.
    pub inventory: bool,
    /// `Stage` is here.
    pub stage: bool,
    /// `StageBulk` is here.
    pub stage_bulk: bool,
}

/// How the last update ended.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct LastResult {
    /// What happened.
    pub outcome: Outcome,
    /// The release version the update was for, `None` when there was none.
    pub version: Option<ImageVersion>,
}

/// What happened to the last update.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    self_describing,
    ww_repr = nib
)]
pub enum Outcome {
    /// No update since the device was provisioned.
    Nothing,
    /// Updated and confirmed.
    Confirmed,
    /// The trial image did not confirm (or was reverted on purpose) and the previous one runs again.
    Reverted,
    /// Refused or abandoned before staging (bad signature, below the floor, a failed step, aborted).
    Rejected,
    /// A stored plan or a live in-place update could not finish and the updater took over.
    Recovery,
}
