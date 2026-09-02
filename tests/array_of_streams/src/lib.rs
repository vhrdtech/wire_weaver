#[cfg(test)]
mod tests {
    use wire_weaver::MessageSink;
    use wire_weaver::prelude::*;
    use ww_client_server::StreamSideband;
    use ww_client_server::{Event, EventKind};

    #[allow(dead_code)]
    pub struct NoStdSyncServer {}

    mod api_impl {
        use super::NoStdSyncServer;
        use tests_common::TestProcessEvents;
        use wire_weaver::MessageSink;

        wire_weaver::ww_codegen!(
            array_of_streams_api :: ArrayOfStreams for NoStdSyncServer,
            server = true, no_alloc = true, use_async = false,
            method_model = "_=immediate",
            property_model = "_=get_set",
            debug_to_file = "../../target/tests_array_of_streams_server.rs"
        );

        impl TestProcessEvents for NoStdSyncServer {
            fn process_request_bytes<'a>(
                &mut self,
                bytes: &[u8],
                scratch: &'a mut [u8],
                msg_tx: &mut impl MessageSink,
            ) -> Result<&'a [u8], ShrinkWrapError> {
                self.process_request_bytes(bytes, scratch, msg_tx)
            }
        }
    }

    #[allow(dead_code)]
    impl NoStdSyncServer {
        fn sideband_stream(
            &mut self,
            _msg_tx: &mut impl MessageSink,
            _cmd: StreamSideband,
        ) -> Option<StreamSideband> {
            None
        }

        fn sideband_array_of_streams(
            &mut self,
            _msg_tx: &mut impl MessageSink,
            _index_chain: [UNib32; 1],
            _cmd: StreamSideband,
        ) -> Option<StreamSideband> {
            None
        }

        fn sideband_subgroup_stream(
            &mut self,
            _msg_tx: &mut impl MessageSink,
            _cmd: StreamSideband,
        ) -> Option<StreamSideband> {
            None
        }

        fn sideband_subgroup_array_of_streams(
            &mut self,
            _msg_tx: &mut impl MessageSink,
            _index_chain: [UNib32; 1],
            _cmd: StreamSideband,
        ) -> Option<StreamSideband> {
            None
        }

        fn sideband_gpio_stream(
            &mut self,
            _msg_tx: &mut impl MessageSink,
            _index_chain: [UNib32; 1],
            _cmd: StreamSideband,
        ) -> Option<StreamSideband> {
            None
        }

        fn sideband_gpio_array_of_streams(
            &mut self,
            _msg_tx: &mut impl MessageSink,
            _index_chain: [UNib32; 2],
            _cmd: StreamSideband,
        ) -> Option<StreamSideband> {
            None
        }

        fn sideband_periph_channel_stream(
            &mut self,
            _msg_tx: &mut impl MessageSink,
            _index_chain: [UNib32; 2],
            _cmd: StreamSideband,
        ) -> Option<StreamSideband> {
            None
        }

        fn sideband_periph_channel_array_of_streams(
            &mut self,
            _msg_tx: &mut impl MessageSink,
            _index_chain: [UNib32; 3],
            _cmd: StreamSideband,
        ) -> Option<StreamSideband> {
            None
        }

        fn valid_indices_root_array_of_streams(&mut self) -> ValidIndices<'_> {
            ValidIndices::range_u32(0..255)
        }

        fn valid_indices_root_gpio(&mut self) -> ValidIndices<'_> {
            ValidIndices::range_u32(0..255)
        }

        fn valid_indices_root_periph(&mut self) -> ValidIndices<'_> {
            ValidIndices::range_u32(0..255)
        }

        fn valid_indices_root_subgroup_array_of_streams(&mut self) -> ValidIndices<'_> {
            ValidIndices::range_u32(0..255)
        }

        fn valid_indices_root_gpio_array_of_streams(
            &mut self,
            _index: [UNib32; 1],
        ) -> ValidIndices<'_> {
            ValidIndices::range_u32(0..255)
        }

        fn valid_indices_root_periph_channel(&mut self, _index: [UNib32; 1]) -> ValidIndices<'_> {
            ValidIndices::range_u32(0..255)
        }

        fn valid_indices_root_periph_channel_array_of_streams(
            &mut self,
            _index: [UNib32; 2],
        ) -> ValidIndices<'_> {
            ValidIndices::range_u32(0..255)
        }
    }

    #[test]
    fn stream_paths_are_correct() {
        let v = &[1u8, 2, 3][..];
        let mut scratch = [0u8; 512];

        let root = api_impl::stream_data_ser();
        let update = root.stream(&v, &mut scratch).unwrap();
        check_path(update, &[0]);
        let update = root.array_of_streams(10, &v, &mut scratch).unwrap();
        check_path(update, &[1, 10]);

        let subgroup = root.subgroup();
        let update = subgroup.stream(&v, &mut scratch).unwrap();
        check_path(update, &[2, 0]);
        let update = subgroup.array_of_streams(11, &v, &mut scratch).unwrap();
        check_path(update, &[2, 1, 11]);

        let gpio = root.gpio(123);
        let update = gpio.stream(&v, &mut scratch).unwrap();
        check_path(update, &[3, 123, 0]);
        let update = gpio.array_of_streams(12, &v, &mut scratch).unwrap();
        check_path(update, &[3, 123, 1, 12]);

        let periph = root.periph(255);
        let channel = periph.channel(1023);
        let update = channel.stream(&v, &mut scratch).unwrap();
        check_path(update, &[4, 255, 0, 1023, 0]);
        let update = channel.array_of_streams(13, &v, &mut scratch).unwrap();
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
