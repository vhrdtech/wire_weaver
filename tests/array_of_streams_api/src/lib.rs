//! Test API of `tests/array_of_streams`: nested arrays of sub-traits (`gpio[]`, `periph[].channel[]`) each
//! carrying a plain stream and an array of streams, to check stream dispatch through array indices.
use ww_api::prelude::*;

#[ww_trait]
trait ArrayOfStreams {
    stream!(stream: Vec<u8>);
    stream!(array_of_streams[]: [u8]);

    ww_impl!(subgroup: Subgroup);
    ww_impl!(gpio[]: Gpio);
    ww_impl!(periph[]: Peripheral);
}

#[ww_trait]
trait Subgroup {
    stream!(stream: [u8]);
    stream!(array_of_streams[]: [u8]);
}

#[ww_trait]
trait Gpio {
    stream!(stream: [u8]);
    stream!(array_of_streams[]: [u8]);
}

#[ww_trait]
trait Peripheral {
    ww_impl!(channel[]: Channel);
}

#[ww_trait]
trait Channel {
    stream!(stream: [u8]);
    stream!(array_of_streams[]: [u8]);
}
