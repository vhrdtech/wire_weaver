use crate::error::{Error, Result};
use crate::ser::flavors::{Flavor, Slice};
use serde::Serialize;

#[cfg(feature = "heapless")]
use crate::ser::flavors::HVec;

#[cfg(feature = "heapless")]
use heapless::Vec;

#[cfg(feature = "alloc")]
use crate::ser::flavors::AllocVec;

#[cfg(feature = "alloc")]
extern crate alloc;

use crate::ser::serializer::Serializer;

pub mod flavors;
pub(crate) mod serializer;

pub fn serialize_with_flavor<T, S, O>(value: &T, storage: S) -> Result<O>
where
    T: Serialize + ?Sized,
    S: Flavor<Output = O>,
{
    let mut serializer = Serializer { output: storage };
    value.serialize(&mut serializer)?;
    serializer
        .output
        .finalize()
        .map_err(|_| Error::SerializeBufferFull)
}