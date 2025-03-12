#[derive(Debug)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
}

impl Version {
    // pub(crate) fn invalid() -> Self {
    //     Version { major: 0, minor: 0 }
    // }

    pub fn new(major: u32, minor: u32) -> Self {
        Self { major, minor }
    }
}
