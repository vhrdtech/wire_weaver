use core::fmt::{Debug, Formatter};

use shrink_wrap::prelude::*;

/// Two hashes uniquely identifying client or device's API - with doc strings and without.
/// The idea behind this is that if something important has changed without changing API or data types,
/// documentation will be the only thing explaining the change.
///
/// Theoretically, API name + version must be unique, but it's hard to enfore version bump on every change.
/// Thus it's possible to introduce a subtle change that might be hard to find.
/// With API hash though, any change will be detected.
/// Client will then download actual ApiBundle from device or cache and use it to check for compatibility.
///
/// Doc strings can consume a lot of FLASH space, or there might not be a lot of it to begin with.
/// So it's beneficial to remove them, but still use full version when interacting with a device.
/// Second hash allows to load full version from cache, while retaining compatibility guarantees.
#[derive_shrink_wrap(
    final_structure,
    derive(Clone, Debug),
    derive_borrowed(Copy),
    owned(feature = "std")
)]
// #[serde = "serde"]
pub struct ApiHashPair<'i> {
    /// Hash of the [ApiBundle] with all the doc strings removed.
    pub no_docs: ApiHash<'i>,
    /// Hash of the [ApiBundle] with all the doc strings kept.
    /// If there are no doc strings, this array is empty.
    pub with_docs: ApiHash<'i>,
}

/// Hash of the [ApiBundle] used to compare if client and server API's are idential.
/// See also [ApiHashPair]
#[derive_shrink_wrap(
    final_structure,
    derive(Clone, PartialEq, Eq),
    derive_borrowed(Copy),
    owned(feature = "std")
)]
pub struct ApiHash<'i> {
    pub hash: RefVec<'i, u8>,
}

impl ApiHashPair<'static> {
    pub fn empty() -> ApiHashPair<'static> {
        const EMPTY: &'static [u8] = &[];
        Self {
            no_docs: ApiHash::new(EMPTY),
            with_docs: ApiHash::new(EMPTY),
        }
    }
}

impl ApiHash<'_> {
    pub fn new(hash: &'static [u8]) -> Self {
        Self {
            hash: RefVec::Slice { slice: hash },
        }
    }
}

impl Debug for ApiHash<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        for byte in self.hash.iter() {
            let Ok(byte) = byte else {
                continue;
            };
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

#[cfg(feature = "std")]
impl Debug for ApiHashOwned {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.to_string())
    }
}

#[cfg(feature = "std")]
impl ApiHash<'_> {
    pub fn make_owend(&self) -> ApiHashOwned {
        ApiHashOwned {
            hash: self.hash.iter().collect::<Result<Vec<u8>, _>>().unwrap(),
        }
    }
}

#[cfg(feature = "std")]
impl ApiHashOwned {
    pub fn to_string(&self) -> String {
        hex::encode(&self.hash)
    }
}

#[cfg(feature = "std")]
impl ApiHashPair<'_> {
    pub fn make_owned(&self) -> ApiHashPairOwned {
        ApiHashPairOwned {
            no_docs: self.no_docs.make_owend(),
            with_docs: self.with_docs.make_owend(),
        }
    }
}
