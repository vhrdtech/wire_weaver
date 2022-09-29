#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SerialUri {
    pub segments: Vec<u32>,
}

// #[derive(Clone, Debug, Eq, PartialEq)]
// pub enum SerialUriSegment {
//     Serial { serial: u32 },
//     SerialIndex { serial: u32, by: u32 },
// }

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SerialMultiUri {}