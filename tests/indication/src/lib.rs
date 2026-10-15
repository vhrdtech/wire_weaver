//! `ww_indication` end to end: two mock servers on `ww_device::Server` through `tests_common::start_device` (one
//! with only the required `mode`, one with every optional item implemented), the generated client through the
//! real host event loop.

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use tests_common::TestDevice;
    use wire_weaver::prelude::*;
    use wire_weaver_client::CallError;
    use ww_error::CommonErrorOwned;
    use ww_indication::{Brightness, Mode};
    use ww_si::Second;

    /// State shared between the test and the full server.
    struct Storage {
        mode: Mode,
        brightness: Brightness,
        busy: bool,
        identify_calls: u32,
        last_identify: Option<Second>,
    }

    impl Default for Storage {
        fn default() -> Self {
            Storage {
                mode: Mode::Normal,
                brightness: Brightness::Normal,
                busy: false,
                identify_calls: 0,
                last_identify: None,
            }
        }
    }

    /// Every item implemented: `mode`, `brightness`, `identify` and `NAMES`.
    mod full_server {
        use super::*;
        use tests_common::TestProcessEvents;
        use wire_weaver::BlockingEventOut;

        pub struct Server {
            pub storage: Arc<Mutex<Storage>>,
        }

        impl Server {
            fn set_indication_mode(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: Mode,
            ) -> SetResult<()> {
                self.storage.lock().unwrap().mode = value;
                Set
            }

            fn get_indication_mode(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<Mode, ()> {
                Value(self.storage.lock().unwrap().mode)
            }

            fn set_indication_brightness(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: Brightness,
            ) -> SetResult<()> {
                self.storage.lock().unwrap().brightness = value;
                Set
            }

            fn get_indication_brightness(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<Brightness, ()> {
                Value(self.storage.lock().unwrap().brightness)
            }

            fn indication_identify(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                duration: Second,
            ) -> RpcResult<Result<(), ww_error::CommonError<'_>>> {
                let mut st = self.storage.lock().unwrap();
                if st.busy {
                    return Ready(Err(ww_error::CommonError::Busy));
                }
                st.identify_calls += 1;
                st.last_identify = Some(duration);
                Ready(Ok(()))
            }
        }

        /// A firmware has these as flash-saved names; the mock serves a fixed pair.
        impl api_impl::IndicationTestConsts for Server {
            const INDICATION_NAMES: RefVec<'static, &'static str> = RefVec::Slice {
                slice: &["status", "power"],
            };
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                indication_api :: IndicationTest for super::Server,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
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

    /// Only `mode`: `brightness`, `identify` and `NAMES` left out with `omit`.
    mod lite_server {
        use super::*;
        use tests_common::TestProcessEvents;
        use wire_weaver::BlockingEventOut;

        pub struct Server {
            pub mode: Mode,
        }

        impl Server {
            fn set_indication_mode(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: Mode,
            ) -> SetResult<()> {
                self.mode = value;
                Set
            }

            fn get_indication_mode(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<Mode, ()> {
                Value(self.mode)
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                indication_api :: IndicationTest for super::Server,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
                omit = ["indication.brightness", "indication.identify", "indication.NAMES"],
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
                indication_api :: IndicationTest for super::StdClient,
                client = "std_client",
            );
        }
    }

    use std_client::StdClient;

    async fn connect(device: &TestDevice) -> StdClient {
        StdClient::config(|c| c.in_process_path(device.path()))
            .connect()
            .await
            .expect("connect")
    }

    fn start_full(path: &str) -> (TestDevice, Arc<Mutex<Storage>>) {
        let _ = tracing_subscriber::fmt::try_init();
        let storage = Arc::new(Mutex::new(Storage::default()));
        let device = tests_common::start_device(
            path,
            full_server::Server {
                storage: storage.clone(),
            },
            indication_api::INDICATION_TEST_FULL_GID,
            full_server::api_impl::api_hash(),
        );
        (device, storage)
    }

    fn start_lite(path: &str) -> TestDevice {
        let _ = tracing_subscriber::fmt::try_init();
        tests_common::start_device(
            path,
            lite_server::Server { mode: Mode::Normal },
            indication_api::INDICATION_TEST_FULL_GID,
            lite_server::api_impl::api_hash(),
        )
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn mode_and_brightness_roundtrip() {
        let (device, _storage) = start_full("indication/roundtrip");
        let client = connect(&device).await;

        client
            .indication()
            .write_mode(Mode::OnlyEssential)
            .write()
            .await
            .unwrap();
        assert_eq!(
            client.indication().read_mode().read().await.unwrap(),
            Mode::OnlyEssential
        );

        client
            .indication()
            .write_brightness(Brightness::Highest)
            .write()
            .await
            .unwrap();
        assert_eq!(
            client.indication().read_brightness().read().await.unwrap(),
            Brightness::Highest
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn names_are_served_without_a_handler() {
        let (device, _storage) = start_full("indication/names");
        let client = connect(&device).await;
        let names = client.indication().read_NAMES().read().await.unwrap();
        assert_eq!(names, vec!["status".to_string(), "power".to_string()]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn identify_blinks_then_refuses_while_busy() {
        let (device, storage) = start_full("indication/identify");
        let client = connect(&device).await;

        client
            .indication()
            .identify(Second::seconds_f32(2.0))
            .call()
            .await
            .unwrap();
        assert_eq!(storage.lock().unwrap().identify_calls, 1);
        assert_eq!(
            storage.lock().unwrap().last_identify,
            Some(Second::seconds_f32(2.0))
        );

        storage.lock().unwrap().busy = true;
        let r = client
            .indication()
            .identify(Second::seconds_f32(1.0))
            .call()
            .await;
        assert!(
            matches!(r, Err(CallError::Device(CommonErrorOwned::Busy))),
            "{r:?}"
        );
    }

    /// A device that leaves optional items out still serves `mode`; the rest fails locally, known from the
    /// device's introspection data (see `docs/api/optional_items.md`).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn lite_server_serves_only_mode() {
        let device = start_lite("indication/lite");
        let client = connect(&device).await;

        client
            .indication()
            .write_mode(Mode::AllOff)
            .write()
            .await
            .unwrap();
        assert_eq!(
            client.indication().read_mode().read().await.unwrap(),
            Mode::AllOff
        );

        assert!(matches!(
            client.indication().read_brightness().read().await,
            Err(wire_weaver_client::Error::NotImplementedByDevice { .. })
        ));
        assert!(matches!(
            client.indication().read_NAMES().read().await,
            Err(wire_weaver_client::Error::NotImplementedByDevice { .. })
        ));
        assert!(matches!(
            client
                .indication()
                .identify(Second::seconds_f32(1.0))
                .call()
                .await,
            Err(CallError::Unimplemented { .. })
        ));
    }
}
