use wire_weaver::prelude::*;
use wire_weaver_client::Commander;

/// Generated client of `ww_i2c::I2c`, on the commander of one attachment.
pub(crate) struct I2cClient {
    pub(crate) cmd: Commander,
}

mod i2c_client {
    use super::*;
    ww_impl!(
        ww_i2c :: I2c for I2cClient,
        client = "trait_client",
        no_alloc = false,
        use_async = true,
        // debug_to_file = "../../target/ww_i2c_hl_i2c.rs"
    );
}
