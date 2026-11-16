use shrink_wrap::prelude::*;

/// Id of a crate or trait in the `ww_global` ledger, a `UNib32` on the wire (ids below 8 take one nibble).
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    cfg_attr_borrowed(feature = "serde", derive(serde::Deserialize, serde::Serialize)),
    derive(PartialEq, Eq, Copy, Clone),
    final_structure
)]
pub struct GlobalTypeId {
    /// The id as assigned in the ledger.
    pub id: UNib32,
}

impl GlobalTypeId {
    /// The id `gid`, as assigned in the ledger.
    pub const fn new(gid: u32) -> GlobalTypeId {
        GlobalTypeId { id: UNib32(gid) }
    }
}
