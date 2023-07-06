use core::fmt::Display;

use smallvec::SmallVec;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Nrl(pub SmallVec<[u32; 3]>);
impl Default for Nrl {
    fn default() -> Self {
        Nrl(SmallVec::new())
    }
}

impl Nrl {
    pub fn new(parts: &[u32]) -> Self {
        Nrl(parts.into())
    }

    pub fn iter(&self) -> core::slice::Iter<u32> {
        self.0.iter()
    }
}

impl Display for Nrl {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "/")?;
        let mut it = self.0.iter().peekable();
        while let Some(p) = it.next() {
            write!(f, "{p}")?;
            if it.peek().is_some() {
                write!(f, "/")?;
            }
        }
        Ok(())
    }
}
