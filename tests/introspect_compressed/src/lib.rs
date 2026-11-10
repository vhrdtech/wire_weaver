//! Compressed introspection data (INT-16): `ww_codegen!(.., introspect_compression = "lz4")` keeps `WW_SELF_BYTES`
//! as LZ4 blocks (`ww_self::compressed`) and sends them like a plain bundle. Checked against a plain
//! server of the same API: the same bundle comes out of the client (download, promise, through the hub), the API
//! hashes are the same, and the bytes take several introspection chunks.

#[cfg(test)]
mod tests {
    use introspect_compressed_api::{
        ChannelId, INTROSPECT_COMPRESSED_FULL_GID, INTROSPECT_COMPRESSED_WIRE_VERSION,
    };
    use std::path::PathBuf;
    use std::time::Duration;
    use wire_weaver::shrink_wrap::{DeserializeShrinkWrap, SerializeShrinkWrapOwned};
    use wire_weaver_client::{ClientConfig, Commander, WwClient};
    use ww_client_server::{Event, EventKind, StreamSideband};
    use ww_hub::{Bound, Exit, HubConfig};
    use ww_self::ApiBundleOwned;

    /// The handlers, the same for every server here.
    macro_rules! sync_handlers {
        ($server:ident) => {
            impl $server {
                fn toggle(
                    &mut self,
                    _cx: &mut Context<'_, impl BlockingEventOut>,
                    channel: ChannelId,
                ) -> RpcResult<bool> {
                    let on = self.on.get_mut(channel.0 as usize);
                    Ready(
                        on.map(|on| {
                            *on = !*on;
                            *on
                        }) == Some(true),
                    )
                }

                fn all_off(
                    &mut self,
                    _cx: &mut Context<'_, impl BlockingEventOut>,
                ) -> RpcResult<()> {
                    self.on = [false; 4];
                    Ready(())
                }

                fn set_enabled(
                    &mut self,
                    _cx: &mut Context<'_, impl BlockingEventOut>,
                    value: bool,
                ) -> SetResult<()> {
                    self.enabled = value;
                    SetResult::Set
                }

                fn get_enabled(
                    &mut self,
                    _cx: &mut Context<'_, impl BlockingEventOut>,
                ) -> GetResult<bool, ()> {
                    Value(self.enabled)
                }
            }

            impl tests_common::TestProcessEvents for $server {
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
        };
    }

    mod plain {
        use super::*;
        use wire_weaver::prelude::*;

        #[derive(Default)]
        pub struct Server {
            on: [bool; 4],
            enabled: bool,
        }

        sync_handlers!(Server);

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                introspect_compressed_api :: IntrospectCompressed for super::Server,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
                introspect = "with_docs",
            );
        }
    }

    mod lz4 {
        use super::*;
        use wire_weaver::prelude::*;

        #[derive(Default)]
        pub struct Server {
            on: [bool; 4],
            enabled: bool,
        }

        sync_handlers!(Server);

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                introspect_compressed_api :: IntrospectCompressed for super::Server,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
                introspect = "with_docs",
                introspect_compression = "lz4",
            );
        }
    }

    mod lz4_async {
        use super::*;
        use wire_weaver::prelude::*;

        #[derive(Default)]
        pub struct Server {
            on: [bool; 4],
            enabled: bool,
        }

        impl Server {
            async fn toggle(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                channel: ChannelId,
            ) -> RpcResult<bool> {
                let on = self.on.get_mut(channel.0 as usize);
                Ready(
                    on.map(|on| {
                        *on = !*on;
                        *on
                    }) == Some(true),
                )
            }

            async fn all_off(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
                self.on = [false; 4];
                Ready(())
            }

            async fn set_enabled(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                value: bool,
            ) -> SetResult<()> {
                self.enabled = value;
                SetResult::Set
            }

            async fn get_enabled(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
            ) -> GetResult<bool, ()> {
                Value(self.enabled)
            }
        }

        impl tests_common::TestProcessEventsAsync for Server {
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

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                introspect_compressed_api :: IntrospectCompressed for super::Server,
                server = true, no_alloc = true, use_async = true,
                method_model = "_=immediate",
                property_model = "_=get_set",
                introspect = "with_docs",
                introspect_compression = "lz4",
            );
        }
    }

    pub struct Host {
        cmd: Commander,
    }

    impl WwClient for Host {
        fn default_config() -> ClientConfig {
            ClientConfig::default()
        }

        fn try_from_cmd(cmd: Commander) -> Result<Self, wire_weaver_client::Error> {
            Ok(Self { cmd })
        }
    }

    mod api_client {
        wire_weaver::ww_codegen!(
            introspect_compressed_api :: IntrospectCompressed for super::Host,
            client = "std_client",
        );
    }

    /// The bundle a plain server sends, with the known traits and types put back as the client does.
    fn plain_bundle() -> ApiBundleOwned {
        wire_weaver_client::decode_introspection(&plain::api_impl::WW_SELF_BYTES)
            .unwrap()
            .unwrap()
    }

    fn bytes(bundle: &ApiBundleOwned) -> Vec<u8> {
        bundle.to_ww_bytes_owned().unwrap()
    }

    /// The device keeps the compressed bytes, they decompress to the plain server's, and the API hashes (the API's
    /// identity) are those of the uncompressed bytes.
    #[test]
    fn bytes_compressed_hashes_unchanged() {
        let packed = &lz4::api_impl::WW_SELF_BYTES[..];
        let plain = &plain::api_impl::WW_SELF_BYTES[..];
        assert!(ww_self::compressed::is_compressed(packed));
        assert!(!ww_self::compressed::is_compressed(plain));
        assert_eq!(ww_self::compressed::decompress(packed).unwrap(), plain);
        assert!(
            packed.len() < plain.len(),
            "{} vs {}",
            packed.len(),
            plain.len()
        );
        // several introspection chunks, so reassembly is checked too
        assert!(packed.len() > 2 * 128, "{}", packed.len());
        assert_eq!(&lz4_async::api_impl::WW_SELF_BYTES[..], packed);
        let hashes = |h: wire_weaver::ww_version::ApiHashPair<'static>| {
            (
                h.no_docs.hash.as_slice().to_vec(),
                h.with_docs.hash.as_slice().to_vec(),
            )
        };
        assert_eq!(
            hashes(lz4::api_impl::api_hash()),
            hashes(plain::api_impl::api_hash())
        );
        assert_eq!(lz4::api_impl::API_ID, plain::api_impl::API_ID);
        let with_docs = ww_self::signature::api_hash(plain);
        assert_eq!(
            lz4::api_impl::api_hash().with_docs.hash.as_slice(),
            &with_docs[..]
        );
    }

    /// What goes over the wire: the compressed bytes in chunks, then `Close`, as for a plain bundle: the data says
    /// it is compressed by its magic.
    #[test]
    fn chunks_then_close() {
        use tests_common::TestProcessEvents;
        let mut server = lz4::Server::default();
        let request = ww_client_server::RequestOwned {
            seq: wire_weaver::shrink_wrap::UVlq32Backfill(0),
            priority: wire_weaver::shrink_wrap::Nibble::new(4).unwrap(),
            path_kind: ww_client_server::PathKindOwned::Absolute { path: vec![] },
            kind: ww_client_server::RequestKindOwned::Introspect,
        }
        .to_ww_bytes_owned()
        .unwrap();
        let mut out = Collect(vec![]);
        let mut scratch = [0u8; 512];
        let _ = TestProcessEvents::process_request_bytes(
            &mut server,
            &request,
            &mut scratch,
            &mut out,
            (),
        );
        let events: Vec<_> = out
            .0
            .iter()
            .map(|e| Event::from_ww_bytes(e).unwrap().result.unwrap())
            .collect();
        assert!(events.len() > 3, "{events:?}");
        let mut data = vec![];
        for e in &events[..events.len() - 1] {
            let EventKind::StreamData { data: d, .. } = e else {
                panic!("{e:?}")
            };
            data.extend_from_slice(d.as_slice());
        }
        assert!(matches!(
            events.last().unwrap(),
            EventKind::StreamSideband {
                sideband: StreamSideband::Close,
                ..
            }
        ));
        assert_eq!(data, lz4::api_impl::WW_SELF_BYTES);
    }

    struct Collect(Vec<Vec<u8>>);

    impl wire_weaver::BlockingEventOut for Collect {
        fn send_with_blocking<F>(&mut self, ser: F) -> Result<(), wire_weaver::SendError>
        where
            F: FnOnce(&mut [u8]) -> Result<usize, wire_weaver::shrink_wrap::Error>,
        {
            let mut buf = vec![0u8; 512];
            let len = ser(&mut buf).map_err(wire_weaver::SendError::Serialize)?;
            self.0.push(buf[..len].to_vec());
            Ok(())
        }
    }

    async fn connect_to(device: &tests_common::TestDevice) -> Host {
        Host::config(|c| c.in_process_path(device.path()))
            .connect()
            .await
            .unwrap()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn sync_device_download_and_calls() {
        let _ = tracing_subscriber::fmt::try_init();
        let device = tests_common::start_device(
            "introspect_compressed/sync",
            lz4::Server::default(),
            INTROSPECT_COMPRESSED_FULL_GID,
            INTROSPECT_COMPRESSED_WIRE_VERSION,
            lz4::api_impl::api_hash(),
        );
        let host = connect_to(&device).await;
        let downloaded = host.cmd.introspect().download().await.unwrap().unwrap();
        assert_eq!(bytes(&downloaded), bytes(&plain_bundle()));
        // and from the local cache, by the device's hashes
        let cached = host.cmd.introspect().get().await.unwrap().unwrap();
        assert_eq!(bytes(&cached), bytes(&plain_bundle()));
        assert!(host.toggle(ChannelId(1)).call().await.unwrap());
        assert!(!host.toggle(ChannelId(1)).call().await.unwrap());
    }

    /// The reference: a plain device's download is the same bundle.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn plain_device_download() {
        let _ = tracing_subscriber::fmt::try_init();
        let device = tests_common::start_device(
            "introspect_compressed/plain",
            plain::Server::default(),
            INTROSPECT_COMPRESSED_FULL_GID,
            INTROSPECT_COMPRESSED_WIRE_VERSION,
            plain::api_impl::api_hash(),
        );
        let host = connect_to(&device).await;
        let downloaded = host.cmd.introspect().download().await.unwrap().unwrap();
        assert_eq!(bytes(&downloaded), bytes(&plain_bundle()));
        assert!(host.toggle(ChannelId(2)).call().await.unwrap());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn async_device_download() {
        let _ = tracing_subscriber::fmt::try_init();
        let device = tests_common::start_device_async(
            "introspect_compressed/async",
            lz4_async::Server::default(),
            INTROSPECT_COMPRESSED_FULL_GID,
            INTROSPECT_COMPRESSED_WIRE_VERSION,
            lz4_async::api_impl::api_hash(),
        );
        let host = connect_to(&device).await;
        let downloaded = host.cmd.introspect().download().await.unwrap().unwrap();
        assert_eq!(bytes(&downloaded), bytes(&plain_bundle()));
    }

    /// The dynamic client's connection downloads it at link setup too.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dynamic_client_at_link_setup() {
        let _ = tracing_subscriber::fmt::try_init();
        let device = tests_common::start_device(
            "introspect_compressed/dynamic",
            lz4::Server::default(),
            INTROSPECT_COMPRESSED_FULL_GID,
            INTROSPECT_COMPRESSED_WIRE_VERSION,
            lz4::api_impl::api_hash(),
        );
        let client = wire_weaver_client::DynClient::from_config(
            ClientConfig::new().in_process_path(device.path()),
        )
        .connect()
        .await
        .unwrap();
        let introspect = client.device_introspect().unwrap();
        assert_eq!(bytes(&introspect.api_bundle), bytes(&plain_bundle()));
    }

    /// A promise, polled from sync code, as GUIs do.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn promise() {
        let _ = tracing_subscriber::fmt::try_init();
        let device = tests_common::start_device(
            "introspect_compressed/promise",
            lz4::Server::default(),
            INTROSPECT_COMPRESSED_FULL_GID,
            INTROSPECT_COMPRESSED_WIRE_VERSION,
            lz4::api_impl::api_hash(),
        );
        let host = connect_to(&device).await;
        let cmd = host.cmd.clone();
        let bundle = tokio::task::spawn_blocking(move || {
            let mut promise = cmd.introspect().download_promise();
            loop {
                promise.sync_poll();
                match promise.take() {
                    Ok(Some(bundle)) => return bundle,
                    Ok(None) => std::thread::sleep(Duration::from_millis(1)),
                    Err(e) => panic!("{e}"),
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(bytes(&bundle), bytes(&plain_bundle()));
    }

    fn runtime_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ww-introspect-compressed-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Through the hub: it fetches the device's introspection once (the answer is the same as a plain device's), later
    /// clients get it from its cache, and the hub decodes it for itself.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn through_the_hub() {
        let _ = tracing_subscriber::fmt::try_init();
        let path = "introspect_compressed/hub";
        let device = tests_common::start_device(
            path,
            lz4::Server::default(),
            INTROSPECT_COMPRESSED_FULL_GID,
            INTROSPECT_COMPRESSED_WIRE_VERSION,
            lz4::api_impl::api_hash(),
        );
        let dir = runtime_dir("hub");
        let bound = Bound::bind(HubConfig {
            runtime_dir: dir.clone(),
            idle_exit: Duration::from_secs(60),
            linger: Duration::from_secs(30),
            version: ww_hub::HUB_VERSION.into(),
            sync: false,
            sync_linger: None,
            sync_options: wire_weaver_client::clock_sync::Options::default(),
            state_dir: None,
            adopt_attach: Duration::from_secs(5),
        })
        .unwrap()
        .expect("no other hub in this dir");
        let shutdown = bound.shutdown_handle();
        let hub = tokio::spawn(bound.run());
        let socket = dir
            .join(ww_local_socket::files::HUB_SOCKET)
            .display()
            .to_string();
        let key = format!("inproc:{path}");
        let client = || Host::config(|c| c.hub_device_at(socket.clone(), key.clone())).connect();
        let a = client().await.unwrap();
        let b = client().await.unwrap();
        let first = a.cmd.introspect().download().await.unwrap().unwrap();
        let second = b.cmd.introspect().download().await.unwrap().unwrap();
        assert_eq!(bytes(&first), bytes(&plain_bundle()));
        assert_eq!(bytes(&second), bytes(&plain_bundle()));
        assert!(a.toggle(ChannelId(0)).call().await.unwrap());
        shutdown.shutdown();
        assert_eq!(hub.await.unwrap(), Exit::Shutdown);
        drop(device);
    }
}
