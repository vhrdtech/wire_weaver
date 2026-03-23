use wire_weaver::prelude::*;

#[ww_trait]
trait ArrayOfStreams {
    stream!(root_stream: Vec<u8>);
    stream!(root_array_of_streams[]: [u8]);

    ww_impl!(subgroup: Subgroup);
    ww_impl!(gpio[]: Gpio);
    ww_impl!(periph[]: Peripheral);
}

#[ww_trait]
trait Subgroup {
    stream!(subgroup_stream: [u8]);
    stream!(subgroup_array_of_streams[]: [u8]);
}

#[ww_trait]
trait Gpio {
    stream!(gpio_stream: [u8]);
    stream!(gpio_array_of_streams[]: [u8]);
}

#[ww_trait]
trait Peripheral {
    ww_impl!(channel[]: Channel);
}

#[ww_trait]
trait Channel {
    stream!(channel_stream: [u8]);
    stream!(channel_array_of_streams[]: [u8]);
}
