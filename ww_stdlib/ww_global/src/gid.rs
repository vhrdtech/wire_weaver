use shrink_wrap::prelude::*;

#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(PartialEq, Eq, Copy, Clone),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    cfg_attr_borrowed(feature = "serde", derive(serde::Deserialize, serde::Serialize)),
    final_structure
)]
pub struct GlobalTypeId {
    pub id: UNib32,
}

impl GlobalTypeId {
    pub const fn new(gid: u32) -> GlobalTypeId {
        GlobalTypeId { id: UNib32(gid) }
    }
}
