use shrink_wrap::prelude::*;

#[derive_shrink_wrap]
#[derive(PartialEq, Eq, Copy, Clone)]
#[defmt = "defmt"]
#[serde = "serde"]
#[final_structure]
pub struct GlobalTypeId {
    pub id: UNib32,
}

impl GlobalTypeId {
    pub const fn new(gid: u32) -> GlobalTypeId {
        GlobalTypeId { id: UNib32(gid) }
    }
}
