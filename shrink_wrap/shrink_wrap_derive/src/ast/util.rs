#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) struct Version {
    pub(crate) major: u32,
    pub(crate) minor: u32,
    pub(crate) patch: u32,
}
