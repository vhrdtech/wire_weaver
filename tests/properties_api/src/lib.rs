use wire_weaver::prelude::*;

#[ww_trait]
trait Properties {
    property!(rw x: u8);
    property!(rw y: u8);

    // changes pub sub
    // const ro wo
    // () [u8]
    // user-defined
    // arrays
}
