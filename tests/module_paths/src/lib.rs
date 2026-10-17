//! Types in modules (API-46), through generated server and client code: each is named by the path other crates see
//! (`module_paths_api::geometry::shapes::Label`, a root re-export, a re-export under another name), equal definitions
//! in different modules stay apart.

#[cfg(test)]
mod tests {
    use module_paths_api::geometry::Point;
    use module_paths_api::geometry::shapes::{Label, LabelOwned};
    use module_paths_api::{Hidden, Renamed, other};
    use std::sync::{Arc, Mutex};
    use tests_common::TestDevice;
    use wire_weaver::prelude::*;

    #[derive(Default)]
    struct SharedTestData {
        hidden: Option<Hidden>,
        renamed: Option<Renamed>,
    }

    mod server {
        use super::*;
        use tests_common::TestProcessEvents;
        use wire_weaver::{GetResult, SetResult};

        pub struct Server {
            pub data: Arc<Mutex<SharedTestData>>,
        }

        impl Server {
            fn mirror(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                p: Point,
            ) -> RpcResult<Point> {
                Ready(Point { x: -p.x, y: -p.y })
            }

            fn lift(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                p: other::Point,
            ) -> RpcResult<other::Point> {
                Ready(other::Point { x: p.x, y: p.y + 1 })
            }

            fn measure(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                label: Label<'_>,
            ) -> RpcResult<u32> {
                Ready(label.text.len() as u32)
            }

            fn name(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<Label<'_>> {
                Ready(Label { text: "probe" })
            }

            fn set_hidden(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: Hidden,
            ) -> SetResult<()> {
                self.data.lock().unwrap().hidden = Some(value);
                SetResult::Set
            }

            fn get_hidden(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<Hidden, ()> {
                match self.data.lock().unwrap().hidden {
                    Some(v) => GetResult::Value(v),
                    None => GetResult::GetError(()),
                }
            }

            fn set_renamed(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: Renamed,
            ) -> SetResult<()> {
                self.data.lock().unwrap().renamed = Some(value);
                SetResult::Set
            }

            fn get_renamed(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<Renamed, ()> {
                match self.data.lock().unwrap().renamed {
                    Some(v) => GetResult::Value(v),
                    None => GetResult::GetError(()),
                }
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                module_paths_api :: ModulePaths for super::Server,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
                // debug_to_file = "../../target/tests_module_paths_server.rs"
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
                module_paths_api :: ModulePaths for super::StdClient,
                client = "std_client",
                // debug_to_file = "../../target/tests_module_paths_client.rs"
            );
        }
    }

    use server::api_impl;
    use std_client::StdClient;

    fn start(path: &str) -> (TestDevice, Arc<Mutex<SharedTestData>>) {
        let _ = tracing_subscriber::fmt::try_init();
        let data = Arc::new(Mutex::new(SharedTestData::default()));
        let server = server::Server { data: data.clone() };
        let device = tests_common::start_device(
            path,
            server,
            module_paths_api::MODULE_PATHS_FULL_GID,
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

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn types_in_modules() {
        let (device, _data) = start("module_paths/methods");
        let mut client = connect(&device).await;
        let mirrored = client.mirror(Point { x: 3, y: -4 }).call().await.unwrap();
        assert_eq!(mirrored, Point { x: -3, y: 4 });
        let lifted = client
            .lift(other::Point { x: 3, y: -4 })
            .call()
            .await
            .unwrap();
        assert_eq!(lifted, other::Point { x: 3, y: -3 });
        let label = LabelOwned {
            text: "hello".into(),
        };
        assert_eq!(client.measure(label).call().await.unwrap(), 5);
        assert_eq!(
            client.name().call().await.unwrap(),
            LabelOwned {
                text: "probe".into()
            }
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn re_exports() {
        let (device, data) = start("module_paths/properties");
        let client = connect(&device).await;
        client.write_hidden(Hidden { id: 7 }).write().await.unwrap();
        assert_eq!(client.read_hidden().read().await.unwrap(), Hidden { id: 7 });
        client
            .write_renamed(Renamed { id: 70_000 })
            .write()
            .await
            .unwrap();
        assert_eq!(
            client.read_renamed().read().await.unwrap(),
            Renamed { id: 70_000 }
        );
        assert_eq!(data.lock().unwrap().renamed, Some(Renamed { id: 70_000 }));
    }
}
