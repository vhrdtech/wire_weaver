#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, RwLock};
    use wire_weaver::MessageSink;
    use wire_weaver::prelude::*;

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

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                traits_api :: Traits for super::NoStdSyncServer,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
                // debug_to_file = "../../target/tests_traits_server.rs"
            );
        }

        impl tests_common::TestProcessEvents for NoStdSyncServer {
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

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn std_async_client_driving_no_std_sync_server() {
        let _ = tracing_subscriber::fmt::try_init();
        let data = Arc::new(RwLock::new(SharedTestData::default()));
        let server = no_std_sync_server::NoStdSyncServer { data: data.clone() };
        let device = tests_common::start_device(
            "traits",
            server,
            traits_api::TRAITS_FULL_GID,
            no_std_sync_server::api_impl::api_hash(),
        );
        let client = std_client::StdClient::config(|c| c.in_process_path(device.path()))
            .connect()
            .await
            .expect("connect");

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
