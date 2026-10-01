#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use streams_api::Point;
    use tests_common::TestDevice;
    use wire_weaver::prelude::*;
    use wire_weaver::shrink_wrap::tail_bytes::TailBytesOwned;
    use wire_weaver_client::{Error, StreamError, TypedStreamEvent};
    use ww_client_server::{Event, EventKind, StreamSideband};

    #[derive(Default)]
    struct SharedTestData {
        /// (resource, sideband) received by the device, in order
        sideband: Vec<(String, StreamSideband)>,
        plain_sink: Vec<u8>,
        user_sink: Vec<Point>,
        bytes_sink: Vec<Vec<u8>>,
        /// Reply to a sideband command with the same command
        ack_sideband: bool,
    }

    mod no_std_sync_server {
        use super::*;
        use tests_common::TestProcessEvents;
        use wire_weaver::MessageSink;

        pub struct NoStdSyncServer {
            pub data: Arc<Mutex<SharedTestData>>,
        }

        impl NoStdSyncServer {
            fn record(&mut self, name: &str, sideband: StreamSideband) -> Option<StreamSideband> {
                let mut data = self.data.lock().unwrap();
                data.sideband.push((name.to_string(), sideband));
                data.ack_sideband.then_some(sideband)
            }

            fn sideband_plain_stream(
                &mut self,
                _msg_tx: &mut impl MessageSink,
                sideband: StreamSideband,
            ) -> Option<StreamSideband> {
                self.record("plain_stream", sideband)
            }

            fn sideband_plain_sink(
                &mut self,
                _msg_tx: &mut impl MessageSink,
                sideband: StreamSideband,
            ) -> Option<StreamSideband> {
                self.record("plain_sink", sideband)
            }

            fn write_plain_sink(&mut self, value: u8) {
                self.data.lock().unwrap().plain_sink.push(value);
            }

            fn sideband_vec_stream(
                &mut self,
                _msg_tx: &mut impl MessageSink,
                sideband: StreamSideband,
            ) -> Option<StreamSideband> {
                self.record("vec_stream", sideband)
            }

            fn valid_indices_root_array_of_streams(&mut self) -> ValidIndices<'_> {
                ValidIndices::range_u32(0..4)
            }

            fn sideband_array_of_streams(
                &mut self,
                _msg_tx: &mut impl MessageSink,
                index_chain: [UNib32; 1],
                sideband: StreamSideband,
            ) -> Option<StreamSideband> {
                self.record(&format!("array_of_streams[{}]", index_chain[0].0), sideband)
            }

            fn finish(&mut self, _msg_tx: &mut impl MessageSink) -> RpcResult<()> {
                Ready(())
            }

            fn sideband_user_stream(
                &mut self,
                _msg_tx: &mut impl MessageSink,
                sideband: StreamSideband,
            ) -> Option<StreamSideband> {
                self.record("user_stream", sideband)
            }

            fn sideband_user_sink(
                &mut self,
                _msg_tx: &mut impl MessageSink,
                sideband: StreamSideband,
            ) -> Option<StreamSideband> {
                self.record("user_sink", sideband)
            }

            fn write_user_sink(&mut self, value: Point) {
                self.data.lock().unwrap().user_sink.push(value);
            }

            fn sideband_bytes_sink(
                &mut self,
                _msg_tx: &mut impl MessageSink,
                sideband: StreamSideband,
            ) -> Option<StreamSideband> {
                self.record("bytes_sink", sideband)
            }

            fn write_bytes_sink(&mut self, data: &shrink_wrap::tail_bytes::TailBytes<'_>) {
                self.data.lock().unwrap().bytes_sink.push(data.0.to_vec());
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                streams_api :: Streams for super::NoStdSyncServer,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
                // debug_to_file = "../../target/tests_streams_server.rs"
            );
        }

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

    mod std_client {
        use wire_weaver_client::{ClientConfig, Commander, WwClient};

        pub struct StdClient {
            pub cmd: Commander,
        }

        impl WwClient for StdClient {
            fn default_config() -> ClientConfig {
                ClientConfig::default()
            }

            fn from_cmd(cmd: Commander) -> Self {
                Self { cmd }
            }
        }

        mod api_client {
            wire_weaver::ww_codegen!(
                streams_api :: Streams for super::StdClient,
                client = "std_client",
                // debug_to_file = "../../target/tests_streams_client.rs"
            );
        }
    }

    use no_std_sync_server::api_impl::stream_data_ser;
    use std_client::StdClient;

    fn start(path: &str) -> (TestDevice, Arc<Mutex<SharedTestData>>) {
        let _ = tracing_subscriber::fmt::try_init();
        let data = Arc::new(Mutex::new(SharedTestData::default()));
        let server = no_std_sync_server::NoStdSyncServer { data: data.clone() };
        let device = tests_common::start_device(
            path,
            server,
            streams_api::STREAMS_FULL_GID,
            no_std_sync_server::api_impl::api_hash(),
        );
        (device, data)
    }

    async fn connect(device: &TestDevice) -> StdClient {
        StdClient::config(|c| c.in_process_path(device.path()))
            .connect()
            .await
            .expect("connect")
    }

    fn connect_blocking(device: &TestDevice) -> StdClient {
        StdClient::config(|c| c.in_process_path(device.path()))
            .connect_blocking()
            .expect("connect")
    }

    /// Stream sideband event from the device, not in response to a request
    fn sideband_event(path: &[u32], sideband: StreamSideband) -> Vec<u8> {
        let path: Vec<UNib32> = path.iter().map(|p| UNib32(*p)).collect();
        let event = Event {
            seq: UVlq32(0),
            result: Ok(EventKind::StreamSideband {
                path: RefVec::Slice { slice: &path },
                sideband,
            }),
        };
        let mut scratch = [0u8; 64];
        event.to_ww_bytes(&mut scratch).unwrap().to_vec()
    }

    /// Wait for the device thread to process what was sent to it
    async fn eventually(data: &Arc<Mutex<SharedTestData>>, f: impl Fn(&SharedTestData) -> bool) {
        for _ in 0..500 {
            if f(&data.lock().unwrap()) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        panic!("condition not met within 500ms");
    }

    fn sideband(data: &Arc<Mutex<SharedTestData>>) -> Vec<(String, StreamSideband)> {
        data.lock().unwrap().sideband.clone()
    }

    fn sb(name: &str, sideband: StreamSideband) -> (String, StreamSideband) {
        (name.to_string(), sideband)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn basic_type_stream_and_open_close() {
        let (device, data) = start("streams/basic_type");
        let client = connect(&device).await;
        let mut stream = client.plain_stream().await.unwrap();

        stream.open().await.unwrap();
        eventually(&data, |d| !d.sideband.is_empty()).await;
        assert_eq!(
            sideband(&data),
            vec![sb("plain_stream", StreamSideband::Open)]
        );

        let mut scratch = [0u8; 64];
        for v in [1u8, 2, 0xAA] {
            device.send(stream_data_ser().plain_stream(&v, &mut scratch).unwrap());
        }
        assert_eq!(stream.recv().await.unwrap(), 1);
        assert_eq!(stream.recv().await.unwrap(), 2);
        assert_eq!(stream.recv().await.unwrap(), 0xAA);

        stream.close().await.unwrap();
        eventually(&data, |d| d.sideband.len() == 2).await;
        assert_eq!(
            sideband(&data)[1],
            sb("plain_stream", StreamSideband::Close)
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn sideband_reply_from_device() {
        let (device, data) = start("streams/sideband_reply");
        data.lock().unwrap().ack_sideband = true;
        let client = connect(&device).await;
        let mut stream = client.plain_stream().await.unwrap();
        assert_eq!(
            stream.recv_any().await.unwrap(),
            TypedStreamEvent::Connected
        );
        stream.open().await.unwrap();
        let ev = tokio::time::timeout(Duration::from_secs(1), stream.recv_any())
            .await
            .expect("sideband reply reaches the stream")
            .unwrap();
        assert_eq!(ev, TypedStreamEvent::Sideband(StreamSideband::Open));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn sideband_from_device() {
        let (device, _data) = start("streams/sideband_from_device");
        let client = connect(&device).await;
        let mut stream = client.plain_stream().await.unwrap();
        // locally generated when subscribed while connected
        assert_eq!(
            stream.recv_any().await.unwrap(),
            TypedStreamEvent::Connected
        );

        device.send(&sideband_event(&[0], StreamSideband::SizeHint(UNib32(3))));
        device.send(&sideband_event(&[0], StreamSideband::FrameSync));
        assert_eq!(
            stream.recv_any().await.unwrap(),
            TypedStreamEvent::Sideband(StreamSideband::SizeHint(UNib32(3)))
        );
        assert_eq!(
            stream.recv_any().await.unwrap(),
            TypedStreamEvent::Sideband(StreamSideband::FrameSync)
        );
        // recv() refuses sideband instead of data
        device.send(&sideband_event(&[0], StreamSideband::Close));
        assert!(matches!(
            stream.recv().await,
            Err(StreamError::UnexpectedEvent(_))
        ));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn byte_buffer_stream() {
        let (device, _data) = start("streams/byte_buffer");
        let client = connect(&device).await;
        let mut stream = client.vec_stream().await.unwrap();

        let mut scratch = [0u8; 512];
        device.send(
            stream_data_ser()
                .vec_stream(&[1, 2, 3], &mut scratch)
                .unwrap(),
        );
        // byte slices are sent as is, without a length
        let TailBytesOwned(bytes) = stream.recv().await.unwrap();
        assert_eq!(bytes, vec![1, 2, 3]);

        let big: Vec<u8> = (0..=255).collect();
        device.send(stream_data_ser().vec_stream(&big, &mut scratch).unwrap());
        device.send(stream_data_ser().vec_stream(&[], &mut scratch).unwrap());
        device.send(stream_data_ser().vec_stream(&[7], &mut scratch).unwrap());
        device.send(&sideband_event(&[2], StreamSideband::Close));
        let mut expected = big.clone();
        expected.push(7);
        assert_eq!(stream.recv_all_bytes().await.unwrap(), expected);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn user_type_stream() {
        let (device, _data) = start("streams/user_type");
        let client = connect(&device).await;
        let mut stream = client.user_stream().await.unwrap();
        let mut scratch = [0u8; 64];
        let points = [Point { x: -1, y: 2 }, Point { x: 300, y: -400 }];
        for p in &points {
            device.send(stream_data_ser().user_stream(p, &mut scratch).unwrap());
        }
        assert_eq!(stream.recv().await.unwrap(), Point { x: -1, y: 2 });
        assert_eq!(stream.recv().await.unwrap(), Point { x: 300, y: -400 });
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn array_of_streams() {
        let (device, data) = start("streams/array");
        let client = connect(&device).await;
        let mut s1 = client.array_of_streams(1).await.unwrap();
        let mut s3 = client.array_of_streams(3).await.unwrap();
        s3.open().await.unwrap();
        eventually(&data, |d| !d.sideband.is_empty()).await;
        assert_eq!(
            sideband(&data),
            vec![sb("array_of_streams[3]", StreamSideband::Open)]
        );

        let mut scratch = [0u8; 64];
        device.send(
            stream_data_ser()
                .array_of_streams(3, &[3], &mut scratch)
                .unwrap(),
        );
        device.send(
            stream_data_ser()
                .array_of_streams(1, &[1], &mut scratch)
                .unwrap(),
        );
        // each index only gets its own updates
        assert_eq!(s1.recv().await.unwrap().0, vec![1]);
        assert_eq!(s3.recv().await.unwrap().0, vec![3]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn sinks() {
        let (device, data) = start("streams/sinks");
        let client = connect(&device).await;

        let mut plain = client.plain_sink().await.unwrap();
        plain.open().await.unwrap();
        plain.send(1).await.unwrap();
        plain.send(2).await.unwrap();
        plain.close().await.unwrap();

        let mut user = client.user_sink().await.unwrap();
        user.send(Point { x: 5, y: -5 }).await.unwrap();

        let mut bytes = client.bytes_sink().await.unwrap();
        bytes.send_bytes(&[1, 2, 3]).await.unwrap();
        bytes.send_bytes(&[]).await.unwrap();

        // requests are processed in order, so all writes are done once this returns
        client.finish().call().await.unwrap();
        let data = data.lock().unwrap();
        assert_eq!(data.plain_sink, vec![1, 2]);
        assert_eq!(data.user_sink, vec![Point { x: 5, y: -5 }]);
        assert_eq!(data.bytes_sink, vec![vec![1, 2, 3], vec![]]);
        assert_eq!(
            data.sideband,
            vec![
                sb("plain_sink", StreamSideband::Open),
                sb("plain_sink", StreamSideband::Close)
            ]
        );
    }

    #[test]
    fn blocking() {
        let (device, data) = start("streams/blocking");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let client = connect_blocking(&device);

        let mut stream = client.plain_stream_blocking().unwrap();
        stream.open_blocking().unwrap();
        let mut scratch = [0u8; 64];
        device.send(stream_data_ser().plain_stream(&9, &mut scratch).unwrap());
        assert_eq!(stream.recv_blocking().unwrap(), 9);

        let mut sink = client.bytes_sink_blocking().unwrap();
        sink.send_bytes_blocking(&[4, 5]).unwrap();
        sink.close_blocking().unwrap();
        for _ in 0..500 {
            if data.lock().unwrap().sideband.len() == 2 {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        let data = data.lock().unwrap();
        assert_eq!(data.bytes_sink, vec![vec![4, 5]]);
        assert_eq!(
            data.sideband,
            vec![
                sb("plain_stream", StreamSideband::Open),
                sb("bytes_sink", StreamSideband::Close)
            ]
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn timeout_async() {
        let (device, _data) = start("streams/timeout_async");
        let client = connect(&device).await;
        let mut stream = client.vec_stream().await.unwrap();
        let mut scratch = [0u8; 64];
        device.send(stream_data_ser().vec_stream(&[1], &mut scratch).unwrap());
        // no Close from the device
        let r = stream
            .recv_all_bytes_timeout(Duration::from_millis(50))
            .await;
        assert!(
            matches!(r, Err(StreamError::Other(Error::Timeout))),
            "{r:?}"
        );
    }

    #[test]
    fn timeout_blocking() {
        let (device, _data) = start("streams/timeout_blocking");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let client = connect_blocking(&device);
        let mut stream = client.vec_stream_blocking().unwrap();
        let mut scratch = [0u8; 64];
        device.send(stream_data_ser().vec_stream(&[1], &mut scratch).unwrap());
        let r = stream.recv_all_bytes_timeout_blocking(Duration::from_millis(50));
        assert!(
            matches!(r, Err(StreamError::Other(Error::Timeout))),
            "{r:?}"
        );

        // with a Close in time, the bytes received so far are returned
        let mut stream = client.vec_stream_blocking().unwrap();
        device.send(stream_data_ser().vec_stream(&[2], &mut scratch).unwrap());
        device.send(&sideband_event(&[2], StreamSideband::Close));
        let r = stream.recv_all_bytes_timeout_blocking(Duration::from_millis(500));
        assert_eq!(r.unwrap(), vec![2]);
    }
}
