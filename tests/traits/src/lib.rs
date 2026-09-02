#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, RwLock};
    use std::time::Duration;
    use tokio::sync::mpsc;
    use wire_weaver::MessageSink;
    use wire_weaver::prelude::*;
    use wire_weaver_client::Commander;
    use wire_weaver_client::internal::ConnectionInfo;
    use wire_weaver_client::{DeviceApiInfo, internal::Command};
    use ww_client_server::{Event, EventKind, Request};

    #[derive(Default)]
    struct SharedTestData {
        subgroup_m1_called: bool,
        gpio_used_indices: Vec<u32>,
        set_gain: HashMap<[UNib32; 2], f32>,
    }

    mod no_std_sync_server {
        use super::*;
        use std::sync::{Arc, RwLock};

        pub struct NoStdSyncServer {
            pub data: Arc<RwLock<SharedTestData>>,
        }

        impl NoStdSyncServer {
            fn g1_m1(&mut self, _msg_tx: &mut impl MessageSink) -> RpcResult<()> {
                self.data.write().unwrap().subgroup_m1_called = true;
                Ready(())
            }

            fn gpio_set_high(
                &mut self,
                _msg_tx: &mut impl MessageSink,
                index: [UNib32; 1],
            ) -> RpcResult<()> {
                self.data
                    .write()
                    .unwrap()
                    .gpio_used_indices
                    .push(index[0].0);
                Ready(())
            }

            fn set_periph_channel_gain(&mut self, index: [UNib32; 2], gain: f32) -> SetResult<()> {
                self.data.write().unwrap().set_gain.insert(index, gain);
                Set
            }

            fn get_periph_channel_gain(&self, index: [UNib32; 2]) -> GetResult<f32, ()> {
                let value = self
                    .data
                    .read()
                    .unwrap()
                    .set_gain
                    .get(&index)
                    .copied()
                    .unwrap_or(0.0);
                Value(value)
            }

            fn periph_channel_run(
                &mut self,
                _msg_tx: &mut impl MessageSink,
                _index: [UNib32; 2],
            ) -> RpcResult<()> {
                Ready(())
            }

            fn valid_indices_root_gpio(&mut self) -> ValidIndices<'_> {
                ValidIndices::range_u32(0..255)
            }

            fn valid_indices_root_periph(&mut self) -> ValidIndices<'_> {
                ValidIndices::range_u32(0..255)
            }

            fn valid_indices_root_periph_channel(
                &mut self,
                _index: [UNib32; 1],
            ) -> ValidIndices<'_> {
                ValidIndices::range_u32(0..255)
            }
        }

        mod api_impl {
            wire_weaver::ww_codegen!(
                traits_api :: Traits for super::NoStdSyncServer,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
                // debug_to_file = "../../target/tests_traits_server.rs"
            );
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
                traits_api :: Traits for super::StdClient,
                client = "std_client",
                // debug_to_file = "../../target/tests_traits_client.rs"
            );
        }
    }

    // mod no_std_raw_client {
    //     use super::*;
    //
    //     pub struct RawClient {}
    //
    //     ww_api!(
    //         "properties.rs" as tests::Properties for RawClient,
    //         client = "raw",
    //         no_alloc = true,
    //         use_async = false,
    //     );
    // }

    struct DummyTx;
    impl MessageSink for DummyTx {
        fn send(&mut self, _message: &[u8]) -> impl Future<Output = Result<(), ()>> {
            core::future::ready(Ok(()))
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn std_async_client_driving_no_std_sync_server() {
        tracing_subscriber::fmt::init();
        let (cmd_tx, mut cmd_rx) = mpsc::channel(128);
        let data = Arc::new(RwLock::new(SharedTestData::default()));

        let mut dummy_msg_tx = DummyTx {};
        let data_clone = data.clone();
        tokio::spawn(async move {
            let mut server = no_std_sync_server::NoStdSyncServer { data: data_clone };
            let mut scratch = [0u8; 512];

            let mut seq = 1;
            while let Some(cmd) = cmd_rx.recv().await {
                match cmd {
                    Command::Connect { connected_tx, .. } => {
                        if let Some(tx) = connected_tx {
                            tx.send(ConnectionInfo {
                                result: Ok(DeviceApiInfo::empty()),
                            })
                            .unwrap();
                        }
                        continue;
                    }
                    Command::SendMessage { mut bytes, done_tx } => {
                        Request::set_seq(&mut bytes, seq);
                        seq += 1;
                        let r = server
                            .process_request_bytes(&bytes, &mut scratch, &mut dummy_msg_tx)
                            .expect("process_request");
                        if r.is_empty() {
                            continue;
                        }
                        let event = Event::from_ww_bytes(r).unwrap();
                        let r = match event.result {
                            Ok(event_kind) => {
                                let data = match event_kind {
                                    EventKind::Value { data } => data.as_slice().to_vec(),
                                    _ => vec![],
                                };
                                Ok(data)
                            }
                            Err(e) => Err(wire_weaver_client::Error::RemoteError(e.make_owned())),
                        };
                        if let Some((done_tx, _timeout)) = done_tx {
                            done_tx.send(r).unwrap();
                        }
                    }
                    _ => panic!("not supported command"),
                }
            }
        });

        let cmd = Commander::new(cmd_tx);
        let client = std_client::StdClient { cmd };
        tokio::time::sleep(Duration::from_millis(10)).await;

        client.g1().m1().call().await.unwrap();
        assert!(data.read().unwrap().subgroup_m1_called);

        client.gpio(0).set_high().call().await.unwrap();
        assert!(data.read().unwrap().gpio_used_indices.contains(&0));

        client.gpio(123).set_high().call().await.unwrap();
        assert!(data.read().unwrap().gpio_used_indices.contains(&123));

        client
            .periph(3)
            .channel(7)
            .write_gain(10.0)
            .write()
            .await
            .unwrap();
        assert_eq!(
            data.read().unwrap().set_gain.get(&[UNib32(3), UNib32(7)]),
            Some(&10.0)
        );
        let value = client
            .periph(3)
            .channel(7)
            .read_gain()
            .read()
            .await
            .unwrap();
        assert!(value == 10.0);
    }
}
