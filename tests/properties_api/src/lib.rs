use wire_weaver::prelude::*;

#[ww_trait]
trait Properties {
    property!(rw x: u8);
    property!(rw y: u8);

    // changes pub sub
    // const ro wo
    // () [u8]
    // property!(rw custom: Custom<'i>);
    // property!(rw custom: RefVec<'i, u8>);
    // arrays
}

#[derive_shrink_wrap]
#[owned = "std"]
struct Custom<'i> {
    z: u8,
    inner: RefVec<'i, Inner<'i>>,
}

#[derive_shrink_wrap]
#[owned = "std"]
#[derive(Clone)]
struct Inner<'i> {
    u: u8,
    v: &'i str,
}
