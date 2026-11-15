//! Server of a keyed array (SRV-62): the Dissector of `keyed_api` answers `index_of`, `key[i]`, a multi read of all
//! keys and the `keys` stream, from a blocking `no_alloc` server and an async one, through the host event loop.
//! Requests are built by hand: the generated client of a keyed array is CLT-72.

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use keyed_api::{Endpoint, FlowKey, Port, Proto};
    use tests_common::TestDevice;
    use tokio::sync::oneshot;
    use wire_weaver::prelude::*;
    use wire_weaver::shrink_wrap::either_any_vec::EitherAnyVec;
    use wire_weaver::shrink_wrap::tail_bytes::TailBytesOwned;
    use wire_weaver::shrink_wrap::{BufReader, UVlq32Backfill};
    use wire_weaver_client::internal::Command;
    use wire_weaver_client::{ClientConfig, Commander, DynClient, Error};
    use ww_client_server::{
        ErrorKindOwned, ErrorOwned, MultiIndexOwned, PathKind, PathKindOwned, RequestKindOwned,
        RequestOwned, ResourceIndex,
    };
    use ww_keyed::KeyEvent;
    use ww_si::origins::DeviceMicros;

    /// Flows by index; `None` is a free index.
    #[derive(Default)]
    struct Table {
        flows: Vec<Option<FlowKey>>,
    }

    impl Table {
        fn indices(&self) -> Vec<UNib32> {
            (0u32..)
                .zip(&self.flows)
                .filter(|(_, flow)| flow.is_some())
                .map(|(i, _)| UNib32(i))
                .collect()
        }
    }

    /// Same handlers for the blocking and async servers: `$($async)?`, the event out trait and how a stream event is
    /// sent differ.
    macro_rules! handlers {
        ($($async:ident)?; $out:path; $send:ident; $($await:tt)*) => {
            fn valid_indices_root_flow(&mut self) -> ValidIndices<'_> {
                self.valid = self.table.lock().unwrap().indices();
                ValidIndices::List(RefVec::Slice { slice: &self.valid })
            }

            $($async)? fn flow_index_of(
                &mut self,
                _cx: &mut Context<'_, impl $out>,
                key: FlowKey,
            ) -> RpcResult<Option<UNib32>> {
                let table = self.table.lock().unwrap();
                let found = (0u32..).zip(&table.flows).find(|(_, flow)| flow.as_ref() == Some(&key));
                Ready(found.map(|(i, _)| UNib32(i)))
            }

            $($async)? fn get_flow_key(
                &mut self,
                _cx: &mut Context<'_, impl $out>,
                index_chain: [ResourceIndex; 1],
            ) -> GetResult<FlowKey, ()> {
                let table = self.table.lock().unwrap();
                let key = table.flows[index_chain[0].get().0 as usize].clone();
                Value(key.expect("valid indices are the occupied ones"))
            }

            /// Drops every flow and announces each on the `keys` stream.
            $($async)? fn clear(&mut self, cx: &mut Context<'_, impl $out>) -> RpcResult<()> {
                let indices = self.table.lock().unwrap().indices();
                self.table.lock().unwrap().flows.clear();
                for index in indices {
                    api_impl::stream_data_ser()
                        .$send(&KeyEvent::Removed { index }, cx)
                        $($await)*
                        .expect("event sent");
                }
                Ready(())
            }

            $($async)? fn get_flow_bytes(
                &mut self,
                _cx: &mut Context<'_, impl $out>,
                _index_chain: [ResourceIndex; 1],
            ) -> GetResult<ww_si::Bytes<u64>, ()> {
                Unimplemented.into()
            }

            $($async)? fn flow_packets(
                &mut self,
                _cx: &mut Context<'_, impl $out>,
                _index_chain: [ResourceIndex; 1],
            ) -> RpcResult<()> {
                Ready(())
            }

            $($async)? fn close_flow_packets(
                &mut self,
                _cx: &mut Context<'_, impl $out>,
                _index_chain: [ResourceIndex; 1],
            ) {
            }

            $($async)? fn sideband_flow_packets(
                &mut self,
                _cx: &mut Context<'_, impl $out>,
                _index_chain: [ResourceIndex; 1],
                _sideband: ww_client_server::StreamSideband,
            ) -> Option<ww_client_server::StreamSideband> {
                None
            }
        };
    }

    mod blocking_server {
        use super::*;
        use tests_common::TestProcessEvents;

        pub struct Server {
            pub table: Arc<Mutex<Table>>,
            pub valid: Vec<UNib32>,
        }

        impl Server {
            handlers!(; BlockingEventOut; flow_keys_send_blocking;);
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                keyed_api :: Dissector for super::Server,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
                multi_req = true,
                debug_to_file = "../../target/tests_keyed_server.rs"
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

    mod async_server {
        use super::*;
        use tests_common::TestProcessEventsAsync;

        pub struct Server {
            pub table: Arc<Mutex<Table>>,
            pub valid: Vec<UNib32>,
        }

        impl Server {
            handlers!(async; EventOut; flow_keys_send; .await);
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                keyed_api :: Dissector for super::Server,
                server = true, no_alloc = false, use_async = true,
                method_model = "_=immediate",
                property_model = "_=get_set",
                multi_req = true
            );
        }

        impl TestProcessEventsAsync for Server {
            type Medium = ();

            async fn process_request_bytes<'a>(
                &mut self,
                bytes: &[u8],
                scratch: &'a mut [u8],
                out: &mut impl EventOut,
                medium: (),
            ) -> Result<&'a [u8], ShrinkWrapError> {
                self.process_request_bytes(bytes, scratch, out, medium)
                    .await
            }
        }
    }

    /// Same wire format as the generated arguments of `index_of`.
    #[derive_shrink_wrap(owned)]
    struct IndexOfArgs {
        key: FlowKey,
    }

    // ids in the API: `flow[]` 0, `flow_keys` 1, `clear` 2; the companion's level: `index_of` 0, `key[]` 1, `keys` 2
    const FLOW_KEYS: u32 = 1;
    const CLEAR: u32 = 2;
    const INDEX_OF: u32 = 0;
    const KEY: u32 = 1;
    const KEYS: u32 = 2;

    fn key(port: u16, first_seen: u64) -> FlowKey {
        let endpoint = |addr: [u8; 4], port: u16| Endpoint {
            addr,
            port: Port(port),
        };
        FlowKey {
            proto: if port.is_multiple_of(2) {
                Proto::Tcp
            } else {
                Proto::Udp
            },
            src: endpoint([10, 0, 0, 1], port),
            dst: endpoint([10, 0, 0, 2], 443),
            first_seen: DeviceMicros::from_raw(first_seen),
        }
    }

    /// Flows met in an order that left index 1 free: 0, 2 and 3 are in use.
    fn table() -> Arc<Mutex<Table>> {
        Arc::new(Mutex::new(Table {
            flows: vec![
                Some(key(5000, 10)),
                None,
                Some(key(5001, 20)),
                Some(key(5002, 30)),
            ],
        }))
    }

    fn start_blocking(path: &str) -> TestDevice {
        let _ = tracing_subscriber::fmt::try_init();
        let server = blocking_server::Server {
            table: table(),
            valid: vec![],
        };
        tests_common::start_device(
            path,
            server,
            keyed_api::DISSECTOR_FULL_GID,
            keyed_api::DISSECTOR_WIRE_VERSION,
            blocking_server::api_impl::api_hash(),
        )
    }

    fn start_async(path: &str) -> TestDevice {
        let _ = tracing_subscriber::fmt::try_init();
        let server = async_server::Server {
            table: table(),
            valid: vec![],
        };
        tests_common::start_device_async(
            path,
            server,
            keyed_api::DISSECTOR_FULL_GID,
            keyed_api::DISSECTOR_WIRE_VERSION,
            async_server::api_impl::api_hash(),
        )
    }

    async fn connect(device: &TestDevice) -> DynClient {
        DynClient::from_config(ClientConfig::new().in_process_path(device.path()))
            .connect()
            .await
            .expect("connect")
    }

    fn resource_path(path: &[u32]) -> Vec<ResourceIndex> {
        path.iter()
            .copied()
            .map(|i| ResourceIndex::new(UNib32(i)))
            .collect()
    }

    /// Send a request as is and wait for the answer: the value bytes, or the device's error.
    async fn raw(cmd: &Commander, path: &[u32], kind: RequestKindOwned) -> Result<Vec<u8>, Error> {
        let req = RequestOwned {
            seq: UVlq32Backfill(0),
            priority: ww_client_server::Priority::NORMAL.to_wire(),
            reserved: false,
            path_kind: PathKindOwned::Absolute {
                path: resource_path(path),
            },
            kind,
        };
        let bytes = req.to_ww_bytes_owned().unwrap();
        let (tx, rx) = oneshot::channel();
        cmd.send(Command::SendMessage {
            bytes,
            done_tx: Some((tx, Duration::from_secs(2))),
        })
        .await?;
        rx.await.expect("event loop answers")
    }

    async fn index_of(cmd: &Commander, key: &FlowKey) -> Option<UNib32> {
        let args = IndexOfArgs { key: key.clone() }
            .to_ww_bytes_owned()
            .unwrap();
        let kind = RequestKindOwned::Call {
            args: TailBytesOwned(args),
        };
        let answer = raw(cmd, &[FLOW_KEYS, INDEX_OF], kind).await.unwrap();
        Option::<UNib32>::from_ww_bytes(&answer).unwrap()
    }

    /// `index_of`, `key[i]` and the key array's valid indices, `key` of a free index refused.
    async fn index_of_and_key(cmd: &Commander) {
        assert_eq!(index_of(cmd, &key(5001, 20)).await, Some(UNib32(2)));
        assert_eq!(index_of(cmd, &key(5000, 10)).await, Some(UNib32(0)));
        assert_eq!(index_of(cmd, &key(5001, 21)).await, None);

        let answer = raw(cmd, &[FLOW_KEYS, KEY, 3], RequestKindOwned::Read)
            .await
            .unwrap();
        assert_eq!(
            FlowKey::from_ww_bytes_owned(&answer).unwrap(),
            key(5002, 30)
        );

        let refused = raw(cmd, &[FLOW_KEYS, KEY, 1], RequestKindOwned::Read).await;
        assert!(
            matches!(
                refused,
                Err(Error::RemoteError(ErrorOwned {
                    kind: ErrorKindOwned::BadIndex,
                    ..
                }))
            ),
            "{refused:?}"
        );

        let answer = raw(cmd, &[FLOW_KEYS, KEY], RequestKindOwned::Read)
            .await
            .unwrap();
        let indices = ValidIndicesOwned::from_ww_bytes_owned(&answer).unwrap();
        assert_eq!(indices.iter().collect::<Vec<_>>(), [0, 2, 3]);
    }

    /// Every key in one multi read over the valid indices.
    async fn multi_read_keys(cmd: &Commander) {
        let kind = RequestKindOwned::MultiRead {
            multi_idx: MultiIndexOwned::List(resource_path(&[0, 2, 3])),
            in_each_array_id: None,
        };
        let answer = raw(cmd, &[FLOW_KEYS, KEY], kind).await.unwrap();
        let mut rd = BufReader::new(&answer);
        let vec = EitherAnyVec::des_shrink_wrap(&mut rd).unwrap();
        let mut iter = vec.iter();
        let keys: Vec<FlowKey> = (0..3)
            .map(|_| {
                let item: Result<FlowKey, ErrorOwned> =
                    iter.next_owned::<ErrorOwned, FlowKey>().unwrap().into();
                item.unwrap()
            })
            .collect();
        assert_eq!(keys, [key(5000, 10), key(5001, 20), key(5002, 30)]);
    }

    /// The `keys` stream: opened without a handler, an `Added` sent from outside a handler, `Removed` for each flow
    /// from `clear`'s handler.
    async fn keys_stream(device: &TestDevice, cmd: &Commander, added: &[u8]) {
        let path = resource_path(&[FLOW_KEYS, KEYS]);
        let mut stream = cmd
            .prepare_stream::<KeyEvent<UNib32, FlowKey>>(PathKind::Absolute {
                path: RefVec::Slice { slice: &path },
            })
            .await
            .unwrap();
        let open = RequestKindOwned::StreamOpen {
            resume: None,
            max_age: None,
            raw: false,
            args: TailBytesOwned(vec![]),
        };
        raw(cmd, &[FLOW_KEYS, KEYS], open).await.unwrap();

        device.send(added);
        assert_eq!(
            stream.recv().await.unwrap(),
            KeyEvent::Added {
                index: UNib32(4),
                key: key(5003, 40)
            }
        );

        let clear = RequestKindOwned::Call {
            args: TailBytesOwned(vec![]),
        };
        raw(cmd, &[CLEAR], clear).await.unwrap();
        for index in [0, 2, 3] {
            assert_eq!(
                stream.recv().await.unwrap(),
                KeyEvent::Removed {
                    index: UNib32(index)
                }
            );
        }
        let refused = raw(cmd, &[FLOW_KEYS, KEY, 0], RequestKindOwned::Read).await;
        assert!(refused.is_err(), "no flow after clear: {refused:?}");
    }

    /// `KeyEvent::Added { 4, key(5003, 40) }` as the device sends it on the `keys` stream.
    fn added_event(ser: impl FnOnce(&KeyEvent<UNib32, FlowKey>, &mut [u8]) -> Vec<u8>) -> Vec<u8> {
        let event = KeyEvent::Added {
            index: UNib32(4),
            key: key(5003, 40),
        };
        let mut scratch = [0u8; 128];
        ser(&event, &mut scratch)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn blocking_no_alloc_server() {
        let device = start_blocking("keyed/blocking");
        let client = connect(&device).await;
        index_of_and_key(client.cmd()).await;
        multi_read_keys(client.cmd()).await;
        let added = added_event(|event, scratch| {
            blocking_server::api_impl::stream_data_ser()
                .flow_keys(event, scratch)
                .unwrap()
                .to_vec()
        });
        keys_stream(&device, client.cmd(), &added).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn async_alloc_server() {
        let device = start_async("keyed/async");
        let client = connect(&device).await;
        index_of_and_key(client.cmd()).await;
        multi_read_keys(client.cmd()).await;
        let added = added_event(|event, scratch| {
            async_server::api_impl::stream_data_ser()
                .flow_keys(event, scratch)
                .unwrap()
                .to_vec()
        });
        keys_stream(&device, client.cmd(), &added).await;
    }

    /// The `keys` stream's events carry the companion's path: `flow_keys` 1, `keys` 2.
    #[test]
    fn keys_stream_path() {
        let added = added_event(|event, scratch| {
            blocking_server::api_impl::stream_data_ser()
                .flow_keys(event, scratch)
                .unwrap()
                .to_vec()
        });
        let event = ww_client_server::Event::from_ww_bytes(&added).unwrap();
        let ww_client_server::EventKind::StreamData { path, .. } = event.result.unwrap() else {
            panic!("not stream data");
        };
        let path: Vec<u32> = path.iter().map(|p| p.unwrap().get().0).collect();
        assert_eq!(path, [FLOW_KEYS, KEYS]);
    }
}
