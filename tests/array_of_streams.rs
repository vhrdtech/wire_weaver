use wire_weaver::prelude::*;
use ww_client_server::{StreamSidebandCommand, StreamSidebandEvent};

#[ww_trait]
trait ArrayOfStreams {
    stream!(root_stream: [u8]);
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

pub struct NoStdSyncServer {}

mod api_impl {
    use wire_weaver::ww_api;

    ww_api!(
        "array_of_streams.rs" as super::ArrayOfStreams for NoStdSyncServer,
        server = true, no_alloc = true, use_async = false,
        method_model = "_=immediate",
        property_model = "_=get_set",
        debug_to_file = "../target/tests_array_of_streams_server.rs"
    );
}

impl NoStdSyncServer {
    fn root_stream_sideband(&mut self, _cmd: StreamSidebandCommand) -> Option<StreamSidebandEvent> {
        None
    }

    fn root_array_of_streams_sideband(
        &mut self,
        _index_chain: [UNib32; 1],
        _cmd: StreamSidebandCommand,
    ) -> Option<StreamSidebandEvent> {
        None
    }

    fn subgroup_stream_sideband(
        &mut self,
        _cmd: StreamSidebandCommand,
    ) -> Option<StreamSidebandEvent> {
        None
    }

    fn subgroup_array_of_streams_sideband(
        &mut self,
        _index_chain: [UNib32; 1],
        _cmd: StreamSidebandCommand,
    ) -> Option<StreamSidebandEvent> {
        None
    }

    fn gpio_stream_sideband(
        &mut self,
        _index_chain: [UNib32; 1],
        _cmd: StreamSidebandCommand,
    ) -> Option<StreamSidebandEvent> {
        None
    }

    fn gpio_array_of_streams_sideband(
        &mut self,
        _index_chain: [UNib32; 2],
        _cmd: StreamSidebandCommand,
    ) -> Option<StreamSidebandEvent> {
        None
    }

    fn channel_stream_sideband(
        &mut self,
        _index_chain: [UNib32; 2],
        _cmd: StreamSidebandCommand,
    ) -> Option<StreamSidebandEvent> {
        None
    }

    fn channel_array_of_streams_sideband(
        &mut self,
        _index_chain: [UNib32; 3],
        _cmd: StreamSidebandCommand,
    ) -> Option<StreamSidebandEvent> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ww_client_server::{Event, EventKind};

    #[test]
    fn stream_paths_are_correct() {
        let v = RefVec::Slice { slice: &[1, 2, 3] };
        let mut s1 = [0u8; 512];
        let mut s2 = [0u8; 512];

        let root = api_impl::stream_data_ser();
        let update = root.root_stream(&v, &mut s1, &mut s2).unwrap();
        check_path(update, &[0]);
        let update = root
            .root_array_of_streams(10, &v, &mut s1, &mut s2)
            .unwrap();
        check_path(update, &[1, 10]);

        let subgroup = root.subgroup();
        let update = subgroup.subgroup_stream(&v, &mut s1, &mut s2).unwrap();
        check_path(update, &[2, 0]);
        let update = subgroup
            .subgroup_array_of_streams(11, &v, &mut s1, &mut s2)
            .unwrap();
        check_path(update, &[2, 1, 11]);

        let gpio = root.gpio(123);
        let update = gpio.gpio_stream(&v, &mut s1, &mut s2).unwrap();
        check_path(update, &[3, 123, 0]);
        let update = gpio
            .gpio_array_of_streams(12, &v, &mut s1, &mut s2)
            .unwrap();
        check_path(update, &[3, 123, 1, 12]);

        let periph = root.periph(255);
        let channel = periph.channel(1023);
        let update = channel.channel_stream(&v, &mut s1, &mut s2).unwrap();
        check_path(update, &[4, 255, 0, 1023, 0]);
        let update = channel
            .channel_array_of_streams(13, &v, &mut s1, &mut s2)
            .unwrap();
        check_path(update, &[4, 255, 0, 1023, 1, 13]);
    }

    fn check_path(event: &[u8], expected: &[u32]) {
        let event = Event::from_ww_bytes(event).unwrap();
        let EventKind::StreamData { path, .. } = event.result.unwrap() else {
            panic!("wrong event");
        };
        assert_eq!(
            path.iter()
                .map(|p| p.unwrap().0)
                .collect::<Vec<_>>()
                .as_slice(),
            expected
        );
    }
}
