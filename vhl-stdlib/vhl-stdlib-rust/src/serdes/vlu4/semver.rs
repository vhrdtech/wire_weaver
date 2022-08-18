use core::fmt::{Display, Formatter};

/// Globally unique identifier of any type or trait. Created when publishing to Registry from:
/// username + project name + file name + module name + identifier
#[derive(Copy, Clone, Debug)]
pub struct GlobalTypeId {
    pub id: u64,
}

#[derive(Copy, Clone, Debug)]
pub struct SemVer {

}

/// Semver requirement
#[derive(Copy, Clone, Debug)]
pub struct SemVerReq<'i> {
    pub data: &'i [u8],
    pub len: usize,
    pub pos: usize,
}

/// Unique identifier compatibility checker of a type inside the Registry.
#[derive(Copy, Clone, Debug)]
pub struct GlobalTypeIdBound<'i> {
    /// Global type id from the Registry
    pub unique_id: GlobalTypeId,
    /// Which version to choose from
    // pub semver_req: VersionReq, // need to avoid Vec
    pub semver_req: SemVerReq<'i>,
}

/// Set of GlobalTypeIdBound
#[derive(Copy, Clone, Debug)]
pub struct TraitSet<'i> {
    pub data: &'i [u8],
    pub len: usize,
    pub pos: usize,
}

impl<'i> Display for TraitSet<'i> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "impl")
    }
}