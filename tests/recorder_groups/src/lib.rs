//! A small firmware leaves every optional group of `ww_recorder::Recorder` out with `omit` (STD-34): `format`,
//! `recordings`, `markers`, `track_settings`, `counters` (both levels) and `usb` stay uncompiled, only the minimum
//! (`status`, `card`, `sync`, `settings`, `track[].info` and `enabled`) is served. See
//! `docs/std_library/ww_recorder.md`.

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use tests_common::TestDevice;
    use wire_weaver::prelude::*;
    use ww_recorder::{
        CardFacts, FullPolicy, ItemType, RecorderError, RecordingMode, Settings, State, Status,
        TrackInfo, UsbView,
    };

    #[derive(Clone)]
    struct SharedTestData {
        settings: Settings,
        enabled: [bool; 2],
    }

    impl SharedTestData {
        fn new() -> Self {
            SharedTestData {
                settings: Settings {
                    flush_bytes: 64 << 10,
                    flush_interval_ms: 1000,
                    full: FullPolicy::DropOldest,
                    mode: RecordingMode::Continuous,
                    usb_view: UsbView {
                        by_time: false,
                        by_track: false,
                        csv: false,
                    },
                },
                enabled: [true, true],
            }
        }
    }

    mod server {
        use super::*;
        use tests_common::TestProcessEvents;
        use wire_weaver::BlockingEventOut;

        pub struct Server {
            pub data: Arc<Mutex<SharedTestData>>,
        }

        impl Server {
            fn valid_indices_root_recorder_track(&mut self) -> ValidIndices<'_> {
                ValidIndices::range_u32(0..2)
            }

            fn recorder_status(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<Status<'_>> {
                Ready(Status {
                    state: State::Logging,
                    session: 1,
                    recording: None,
                    device_time: None,
                    ring_used_bytes: 0,
                    ring_capacity_bytes: 1 << 20,
                })
            }

            fn recorder_card(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<Option<CardFacts<'_>>> {
                Ready(None)
            }

            fn recorder_sync(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<Result<(), RecorderError<'_>>> {
                Ready(Ok(()))
            }

            fn set_recorder_settings(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: Settings,
            ) -> SetResult<RecorderError<'_>> {
                self.data.lock().unwrap().settings = value;
                Set
            }

            fn get_recorder_settings(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<Settings, RecorderError<'_>> {
                Value(self.data.lock().unwrap().settings)
            }

            fn recorder_track_info(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index_chain: [UNib32; 1],
            ) -> RpcResult<TrackInfo<'_>> {
                let n = index_chain[0].0;
                let path: &'static [UNib32] = if n == 0 { &[] } else { &[UNib32(0)] };
                Ready(TrackInfo {
                    path: MaxLen::new(RefVec::Slice { slice: path }).unwrap(),
                    item_type: ItemType {
                        signature: None,
                        global_id: None,
                    },
                    key: n as u64,
                    rate: 0,
                })
            }

            fn set_recorder_track_enabled(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index_chain: [UNib32; 1],
                value: bool,
            ) -> SetResult<RecorderError<'_>> {
                let n = index_chain[0].0;
                if n == 0 && !value {
                    // the system track can't be disabled
                    return SetError(RecorderError::unsupported());
                }
                self.data.lock().unwrap().enabled[n as usize] = value;
                Set
            }

            fn get_recorder_track_enabled(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index_chain: [UNib32; 1],
            ) -> GetResult<bool, RecorderError<'_>> {
                Value(self.data.lock().unwrap().enabled[index_chain[0].0 as usize])
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                recorder_groups_api :: RecorderGroupsTest for super::Server,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
                omit = [
                    "recorder.formatting",
                    "recorder.recording",
                    "recorder.markers",
                    "recorder.track.track_settings",
                    "recorder.counters",
                    "recorder.track.counters",
                    "recorder.usb",
                    "recorder.track.compression",
                ],
                // debug_to_file = "../../target/tests_recorder_groups_server.rs"
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
                recorder_groups_api :: RecorderGroupsTest for super::StdClient,
                client = "std_client",
                omit = [
                    "recorder.formatting",
                    "recorder.recording",
                    "recorder.markers",
                    "recorder.track.track_settings",
                    "recorder.counters",
                    "recorder.track.counters",
                    "recorder.usb",
                    "recorder.track.compression",
                ],
                // debug_to_file = "../../target/tests_recorder_groups_client.rs"
            );
        }
    }

    use server::api_impl;
    use std_client::StdClient;

    fn start(path: &str) -> (TestDevice, Arc<Mutex<SharedTestData>>) {
        let _ = tracing_subscriber::fmt::try_init();
        let data = Arc::new(Mutex::new(SharedTestData::new()));
        let server = server::Server { data: data.clone() };
        let device = tests_common::start_device(
            path,
            server,
            recorder_groups_api::RECORDER_GROUPS_TEST_FULL_GID,
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

    /// With every optional group omitted, only the minimum is served: a small firmware still answers status, card,
    /// sync, settings and the track array.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn minimum_is_served() {
        let (device, data) = start("recorder_groups/minimum");
        let client = connect(&device).await;

        let status = client.recorder().status().call().await.unwrap();
        assert_eq!(status.session, 1);
        assert!(client.recorder().card().call().await.unwrap().is_none());
        client.recorder().sync().call().await.unwrap();

        let settings = client.recorder().read_settings().read().await.unwrap();
        assert_eq!(settings.flush_bytes, 64 << 10);
        client
            .recorder()
            .write_settings(Settings {
                flush_bytes: 128 << 10,
                ..settings
            })
            .write()
            .await
            .unwrap();
        assert_eq!(data.lock().unwrap().settings.flush_bytes, 128 << 10);

        let info = client.recorder().track(1).info().call().await.unwrap();
        assert_eq!(info.key, 1);

        client
            .recorder()
            .track(1)
            .write_enabled(false)
            .write()
            .await
            .unwrap();
        assert!(!data.lock().unwrap().enabled[1]);

        // the system track can't be disabled: the handler's own RecorderError::Common(Unsupported)
        let err = client
            .recorder()
            .track(0)
            .write_enabled(false)
            .write()
            .await;
        assert!(err.is_err());
    }
}
