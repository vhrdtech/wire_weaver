#[cfg(test)]
mod tests {
    use wire_weaver::prelude::*;
    use wire_weaver::MessageSink;
    use ww_client_server::{Event, EventKind};
    use ww_client_server::{StreamSidebandCommand, StreamSidebandEvent};

    #[allow(dead_code)]
    pub struct NoStdSyncServer {}

    mod api_impl {
        use super::NoStdSyncServer;
        use tests_common::TestProcessEvents;
        use wire_weaver::MessageSink;

        wire_weaver::ww_codegen!(
            "../array_of_streams_api" :: ArrayOfStreams for NoStdSyncServer,
            server = true, no_alloc = true, use_async = false,
            method_model = "_=immediate",
            property_model = "_=get_set",
            debug_to_file = "../../target/tests_array_of_streams_server.rs"
        );

        impl TestProcessEvents for NoStdSyncServer {
            fn process_request_bytes<'a>(
                &mut self,
                bytes: &[u8],
                scratch_args: &'a mut [u8],
                scratch_event: &'a mut [u8],
                scratch_err: &'a mut [u8],
                msg_tx: &mut impl MessageSink,
            ) -> Result<&'a [u8], ShrinkWrapError> {
                self.process_request_bytes(bytes, scratch_args, scratch_event, scratch_err, msg_tx)
            }
        }
    }

    #[allow(dead_code)]
    impl NoStdSyncServer {
        fn root_stream_sideband(
            &mut self,
            _msg_tx: &mut impl MessageSink,
            _cmd: StreamSidebandCommand,
        ) -> Option<StreamSidebandEvent> {
            None
        }

        fn root_array_of_streams_sideband(
            &mut self,
            _msg_tx: &mut impl MessageSink,
            _index_chain: [UNib32; 1],
            _cmd: StreamSidebandCommand,
        ) -> Option<StreamSidebandEvent> {
            None
        }

        fn subgroup_stream_sideband(
            &mut self,
            _msg_tx: &mut impl MessageSink,
            _cmd: StreamSidebandCommand,
        ) -> Option<StreamSidebandEvent> {
            None
        }

        fn subgroup_array_of_streams_sideband(
            &mut self,
            _msg_tx: &mut impl MessageSink,
            _index_chain: [UNib32; 1],
            _cmd: StreamSidebandCommand,
        ) -> Option<StreamSidebandEvent> {
            None
        }

        fn gpio_stream_sideband(
            &mut self,
            _msg_tx: &mut impl MessageSink,
            _index_chain: [UNib32; 1],
            _cmd: StreamSidebandCommand,
        ) -> Option<StreamSidebandEvent> {
            None
        }

        fn gpio_array_of_streams_sideband(
            &mut self,
            _msg_tx: &mut impl MessageSink,
            _index_chain: [UNib32; 2],
            _cmd: StreamSidebandCommand,
        ) -> Option<StreamSidebandEvent> {
            None
        }

        fn channel_stream_sideband(
            &mut self,
            _msg_tx: &mut impl MessageSink,
            _index_chain: [UNib32; 2],
            _cmd: StreamSidebandCommand,
        ) -> Option<StreamSidebandEvent> {
            None
        }

        fn channel_array_of_streams_sideband(
            &mut self,
            _msg_tx: &mut impl MessageSink,
            _index_chain: [UNib32; 3],
            _cmd: StreamSidebandCommand,
        ) -> Option<StreamSidebandEvent> {
            None
        }

        fn valid_indices_root_root_array_of_streams(&mut self) -> ValidIndices<'_> {
            ValidIndices::Range(0..255)
        }

        fn valid_indices_root_gpio(&mut self) -> ValidIndices<'_> {
            ValidIndices::Range(0..255)
        }

        fn valid_indices_root_periph(&mut self) -> ValidIndices<'_> {
            ValidIndices::Range(0..255)
        }

        fn valid_indices_root_subgroup_subgroup_array_of_streams(&mut self) -> ValidIndices<'_> {
            ValidIndices::Range(0..255)
        }

        fn valid_indices_root_gpio_gpio_array_of_streams(
            &mut self,
            _index: [UNib32; 1],
        ) -> ValidIndices<'_> {
            ValidIndices::Range(0..255)
        }

        fn valid_indices_root_periph_channel(&mut self, _index: [UNib32; 1]) -> ValidIndices<'_> {
            ValidIndices::Range(0..255)
        }

        fn valid_indices_root_periph_channel_channel_array_of_streams(
            &mut self,
            _index: [UNib32; 2],
        ) -> ValidIndices<'_> {
            ValidIndices::Range(0..255)
        }
    }

    #[test]
    fn stream_paths_are_correct() {
        let v = &[1u8, 2, 3][..];
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
