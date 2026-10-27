//! Bounded lists and strings (`MaxLen`, SW-19) through generated server and client code: arguments, return values,
//! properties, streams and sinks, checked on both sides; the bound in introspection data and the dynamic client; the
//! worst-case message lengths generated for each method.

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::{Arc, Mutex};
    use tests_common::TestDevice;
    use wire_weaver::prelude::*;
    use wire_weaver_client::ww_numeric::NumericValue;
    use wire_weaver_client::ww_self::{ApiBundleOwned, ApiItemKindOwned, TypeOwned, ValueOwned};
    use wire_weaver_client::{ClientConfig, DynClient, DynResource, DynResourceKind};

    #[derive(Default)]
    struct SharedTestData {
        pieces: Vec<Vec<u8>>,
        label: String,
        blocks: Vec<Vec<u8>>,
    }

    mod server {
        use super::*;
        use bounded_api::Piece;
        use tests_common::TestProcessEvents;
        use wire_weaver::{GetResult, SetResult};

        pub struct Server {
            pub data: Arc<Mutex<SharedTestData>>,
            pub label: [u8; 16],
        }

        impl Server {
            fn write(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _offset: u32,
                _last: bool,
                data: MaxLen<RefVec<'_, u8>, 8>,
            ) -> RpcResult<u8> {
                self.data
                    .lock()
                    .unwrap()
                    .pieces
                    .push(data.as_slice().to_vec());
                Ready(data.len() as u8)
            }

            fn name(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                len: u8,
            ) -> RpcResult<MaxLen<&'_ str, 4>> {
                // too long on purpose for len > 4: fails to serialize, the client gets an error
                Ready(MaxLen::new_unchecked(&"abcdefgh"[..len as usize]))
            }

            fn tags(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                tags: MaxLen<RefVec<'_, MaxLen<&'_ str, 3>>, 2>,
            ) -> RpcResult<u8> {
                Ready(tags.len() as u8)
            }

            fn piece(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                piece: Piece<'_>,
            ) -> RpcResult<u32> {
                Ready(piece.offset + piece.data.len() as u32)
            }

            fn unbounded(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _data: RefVec<'_, u8>,
            ) -> RpcResult<()> {
                Ready(())
            }

            fn set_label(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: MaxLen<&'_ str, 6>,
            ) -> SetResult<()> {
                self.data.lock().unwrap().label = value.to_string();
                SetResult::Set
            }

            fn get_label(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<MaxLen<&'_ str, 6>, ()> {
                let label = self.data.lock().unwrap().label.clone();
                self.label[..label.len()].copy_from_slice(label.as_bytes());
                let label = core::str::from_utf8(&self.label[..label.len()]).unwrap();
                GetResult::Value(MaxLen::new(label).unwrap())
            }

            fn chunks(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<()> {
                Ready(())
            }

            fn close_chunks(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {}

            fn sideband_chunks(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _sideband: ww_client_server::StreamSideband,
            ) -> Option<ww_client_server::StreamSideband> {
                None
            }

            fn sideband_blocks(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _sideband: ww_client_server::StreamSideband,
            ) -> Option<ww_client_server::StreamSideband> {
                None
            }

            fn write_blocks(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                data: &shrink_wrap::tail_bytes::TailBytes<'_>,
            ) {
                self.data.lock().unwrap().blocks.push(data.0.to_vec());
            }

            fn emit_chunk(
                &mut self,
                cx: &mut Context<'_, impl BlockingEventOut>,
                len: u8,
            ) -> RpcResult<bool> {
                let chunk = [0xC0u8; 8];
                let sent = api_impl::stream_data_ser()
                    .chunks_send_blocking(&chunk[..len as usize], cx)
                    .is_ok();
                Ready(sent)
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                bounded_api :: Bounded for super::Server,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
                // debug_to_file = "../../target/tests_bounded_server.rs"
            );
        }

        impl TestProcessEvents for Server {
            type Medium = ();

            fn process_request_bytes<'a>(
                &mut self,
                bytes: &[u8],
                scratch: &'a mut [u8],
                out: &mut impl BlockingEventOut,
                medium: (),
            ) -> Result<&'a [u8], ShrinkWrapError> {
                self.process_request_bytes(bytes, scratch, out, medium)
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
                bounded_api :: Bounded for super::StdClient,
                client = "std_client",
                // debug_to_file = "../../target/tests_bounded_client.rs"
            );
        }
    }

    use server::api_impl;
    use std_client::StdClient;

    // The largest `write` request fits the device's message buffer, checked at compile time.
    const _: () = assert!(matches!(
        api_impl::WRITE_REQUEST_MAX_LEN,
        Some(len) if len <= tests_common::MAX_MESSAGE_LEN
    ));

    fn new_server() -> (server::Server, Arc<Mutex<SharedTestData>>) {
        let data = Arc::new(Mutex::new(SharedTestData::default()));
        let server = server::Server {
            data: data.clone(),
            label: [0; 16],
        };
        (server, data)
    }

    fn start(path: &str) -> (TestDevice, Arc<Mutex<SharedTestData>>) {
        let _ = tracing_subscriber::fmt::try_init();
        let (server, data) = new_server();
        let device = tests_common::start_device(
            path,
            server,
            bounded_api::BOUNDED_FULL_GID,
            api_impl::api_hash(),
        );
        (device, data)
    }

    async fn connect(device: &TestDevice) -> StdClient {
        StdClient::config(|c| c.in_process_path(device.path()))
            .connect()
            .await
            .expect("connect")
    }

    fn bundle() -> Arc<ApiBundleOwned> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../bounded_api");
        Arc::new(wire_weaver_core::load(&path, Some("Bounded".into()), false).unwrap())
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn arguments_and_return_values() {
        let (device, data) = start("bounded/methods");
        let client = connect(&device).await;

        let at_bound = MaxLen::new(vec![1u8; 8]).unwrap();
        assert_eq!(client.write(0, true, at_bound).call().await.unwrap(), 8);
        assert_eq!(data.lock().unwrap().pieces, vec![vec![1u8; 8]]);

        // the client doesn't send a value that is too long
        let too_long = MaxLen::new_unchecked(vec![1u8; 9]);
        assert!(client.write(0, true, too_long).call().await.is_err());
        assert_eq!(data.lock().unwrap().pieces.len(), 1);

        assert_eq!(&*client.name(4).call().await.unwrap(), "abcd");
        // the server fails to serialize a return value that is too long
        assert!(client.name(5).call().await.is_err());

        let tags = MaxLen::new(vec![
            MaxLen::new("abc".to_string()).unwrap(),
            MaxLen::new("d".to_string()).unwrap(),
        ])
        .unwrap();
        assert_eq!(client.tags(tags).call().await.unwrap(), 2);
        let tags = MaxLen::new(vec![MaxLen::new_unchecked("abcd".to_string())]).unwrap();
        assert!(client.tags(tags).call().await.is_err());

        let piece = bounded_api::PieceOwned {
            offset: 10,
            data: MaxLen::new(vec![1, 2, 3]).unwrap(),
        };
        assert_eq!(client.piece(piece).call().await.unwrap(), 13);
    }

    #[test]
    fn server_rejects_a_value_that_is_too_long() {
        use ww_client_server::{ErrorKind, Event, PathKind, Request, RequestKind};

        // `write` arguments serialized without the bound, as an older or misbehaving client might
        #[derive_shrink_wrap(borrowed)]
        struct UnboundedArgs<'i> {
            offset: u32,
            last: bool,
            data: RefVec<'i, u8>,
        }
        let (mut server, data) = new_server();
        for (len, accepted) in [(8, true), (9, false)] {
            let bytes = vec![0xAB; len];
            let args = UnboundedArgs {
                offset: 0,
                last: true,
                data: RefVec::new_bytes(&bytes),
            };
            let mut args_buf = [0u8; 64];
            let args = args.to_ww_bytes(&mut args_buf).unwrap();
            let request = Request {
                seq: shrink_wrap::UVlq32Backfill(1),
                path_kind: PathKind::Absolute {
                    path: RefVec::Slice {
                        slice: &[UNib32(0)],
                    },
                },
                kind: RequestKind::Call {
                    args: shrink_wrap::tail_bytes::TailBytes(args),
                },
            };
            let mut request_buf = [0u8; 64];
            let request = request.to_ww_bytes(&mut request_buf).unwrap();

            let mut scratch = [0u8; 256];
            let mut queue_buf = [0u8; 256];
            let mut out = wire_weaver::EventQueue::new(&mut queue_buf);
            let event = server
                .process_request_bytes(request, &mut scratch, &mut out, ())
                .unwrap();
            let event = Event::from_ww_bytes(event).unwrap();
            match event.result {
                Ok(_) => assert!(accepted),
                Err(e) => {
                    assert!(!accepted);
                    assert!(matches!(e.kind, ErrorKind::ArgsDesFailed), "{e:?}");
                }
            }
        }
        assert_eq!(data.lock().unwrap().pieces, vec![vec![0xAB; 8]]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn property() {
        let (device, data) = start("bounded/property");
        let client = connect(&device).await;
        client
            .write_label(MaxLen::new("abcdef".to_string()).unwrap())
            .write()
            .await
            .unwrap();
        assert_eq!(data.lock().unwrap().label, "abcdef");
        assert_eq!(client.read_label().read().await.unwrap().as_str(), "abcdef");
        assert!(
            client
                .write_label(MaxLen::new_unchecked("abcdefg".to_string()))
                .write()
                .await
                .is_err()
        );
        assert_eq!(data.lock().unwrap().label, "abcdef");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn byte_stream_and_sink() {
        let (device, data) = start("bounded/streams");
        let client = connect(&device).await;
        let mut chunks = client.chunks().call().await.unwrap();
        assert!(client.emit_chunk(4).call().await.unwrap());
        // too long: not sent
        assert!(!client.emit_chunk(5).call().await.unwrap());
        assert_eq!(chunks.try_recv().unwrap().unwrap().0, vec![0xC0; 4]);
        assert!(chunks.try_recv().unwrap().is_none());

        // sent as is, like an unbounded byte stream, and received as such
        let mut scratch = [0u8; 64];
        let ser = api_impl::stream_data_ser();
        device.send(ser.chunks(&[1, 2], &mut scratch).unwrap());
        assert_eq!(chunks.recv().await.unwrap().0, vec![1, 2]);
        assert_eq!(
            ser.chunks(&[1, 2, 3, 4, 5], &mut scratch).err(),
            Some(ShrinkWrapError::MaxLenExceeded)
        );

        let mut blocks = client.blocks().await.unwrap();
        blocks.send_bytes(&[1, 2, 3, 4]).await.unwrap();
        // too long, rejected by the device
        blocks.send_bytes(&[1, 2, 3, 4, 5]).await.unwrap();
        blocks.send_bytes(&[]).await.unwrap();
        // requests are processed in order, so all writes are done once this returns
        client.emit_chunk(0).call().await.unwrap();
        assert_eq!(data.lock().unwrap().blocks, vec![vec![1, 2, 3, 4], vec![]]);
    }

    #[test]
    fn bound_in_introspection() {
        let bundle = bundle();
        let write = bundle.get_item(&[0]).unwrap();
        let ApiItemKindOwned::Method { args, .. } = &write.kind else {
            panic!("method");
        };
        // read from `MAX_PIECE`
        assert_eq!(args[2].ty.max_len(&bundle).unwrap(), Some(8));
        assert!(matches!(
            &args[2].ty,
            TypeOwned::MaxLen { ty, .. } if matches!(**ty, TypeOwned::Vec(_))
        ));
        assert!(args[2].ty.is_byte_slice(&bundle).unwrap());
        assert_eq!(
            args[2].ty.human_name(false, &bundle).unwrap(),
            "MaxLen<Vec<u8>, 8>"
        );
        let label = bundle.get_item(&[5]).unwrap();
        let ApiItemKindOwned::Property { ty, .. } = &label.kind else {
            panic!("property");
        };
        assert_eq!(ty.human_name(false, &bundle).unwrap(), "MaxLen<String, 6>");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dynamic_client_honours_the_bound() {
        let (device, data) = start("bounded/dynamic");
        let client = DynClient::from_config(ClientConfig::new().in_process_path(device.path()))
            .connect()
            .await
            .expect("connect");
        let cmd = client.cmd();
        let root = DynResource::root(bundle());
        let write = root.child("write").unwrap();
        let DynResourceKind::Method { .. } = write.kind().unwrap() else {
            panic!("method");
        };
        let num = |n| ValueOwned::Numeric(n);
        let bytes = |len: usize| {
            ValueOwned::Vec(
                (0..len)
                    .map(|_| num(NumericValue::U8(7)))
                    .collect::<Vec<_>>(),
            )
        };
        let args = |len| {
            [
                num(NumericValue::U32(0)),
                ValueOwned::Bool(true),
                bytes(len),
            ]
        };
        assert_eq!(
            write.call(cmd, &args(8)).await.unwrap(),
            Some(num(NumericValue::U8(8)))
        );
        let err = write.call(cmd, &args(9)).await.unwrap_err().to_string();
        assert!(err.contains("at most 8"), "{err}");
        assert_eq!(data.lock().unwrap().pieces, vec![vec![7u8; 8]]);

        let label = root.child("label").unwrap();
        let err = label
            .write(cmd, &ValueOwned::String("abcdefg".into()))
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("at most 6"), "{err}");
    }

    #[test]
    fn worst_case_message_lengths() {
        // offset (4 bytes), last, 8 bytes of data, the request around them
        let write = api_impl::WRITE_REQUEST_MAX_LEN.unwrap();
        assert!((14..32).contains(&write), "{write}");
        assert!(api_impl::WRITE_RESPONSE_MAX_LEN.is_some());
        assert!(api_impl::NAME_RESPONSE_MAX_LEN.is_some());
        assert!(api_impl::TAGS_REQUEST_MAX_LEN.is_some());
        assert!(api_impl::PIECE_REQUEST_MAX_LEN.is_some());
        assert_eq!(api_impl::UNBOUNDED_REQUEST_MAX_LEN, None);
        assert!(api_impl::UNBOUNDED_RESPONSE_MAX_LEN.is_some());
    }
}
