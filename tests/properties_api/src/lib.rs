use wire_weaver::prelude::*;

#[ww_trait]
trait Properties {
    property!(rw x: u8);
    property!(rw y: u8);

    property!(rw absent: u8);

    // changes pub sub
    // const ro wo
    // () [u8]
    property!(rw custom: Custom<'i>);
    // property!(rw custom: RefVec<'i, u8>);
    // arrays
}

#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone))]
struct Custom<'i> {
    z: u8,
    inner: RefVec<'i, Inner<'i>>,
}

#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq, Eq))]
struct Inner<'i> {
    u: u8,
    v: &'i str,
}

impl Custom<'_> {
    pub fn make_owned(&self) -> CustomOwned {
        CustomOwned {
            z: self.z,
            inner: self.inner.iter().map(|v| v.unwrap().make_owned()).collect(),
        }
    }
}

impl Inner<'_> {
    pub fn make_owned(&self) -> InnerOwned {
        InnerOwned {
            u: self.u,
            v: self.v.to_string(),
        }
    }
}

impl Default for CustomOwned {
    fn default() -> Self {
        Self {
            z: 0,
            inner: Default::default(),
        }
    }
}
