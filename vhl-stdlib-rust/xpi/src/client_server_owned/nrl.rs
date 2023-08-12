use core::{fmt::Display, ops::Deref};
use core::slice::Iter;

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

impl PartialEq<[u32]> for Nrl {
    fn eq(&self, other: &[u32]) -> bool {
        self.0.deref() == other
    }
}

macro_rules! impl_partial_eq {
    ($len:literal) => {
        impl PartialEq<[u32; $len]> for Nrl {
            fn eq(&self, other: &[u32; $len]) -> bool {
                self.0.deref() == other
            }
        }
    };
}
impl_partial_eq!(1);
impl_partial_eq!(2);
impl_partial_eq!(3);
impl_partial_eq!(4);

impl From<Iter<'_, u32>> for Nrl {
    fn from(value: Iter<u32>) -> Self {
        Nrl(value.copied().collect())
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
