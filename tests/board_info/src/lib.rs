//! `ww_board_info` end to end: a mock server on `ww_device::Server` through `tests_common::start_device`, the generated
//! client through the real host event loop.

#[cfg(test)]
mod tests {
    use tests_common::TestDevice;
    use wire_weaver::prelude::*;
    use ww_board_info::{BoardSummary, RevisionSource};

    mod server {
        use super::*;
        use tests_common::TestProcessEvents;
        use wire_weaver::BlockingEventOut;

        /// A board with the given revision and where it came from.
        pub struct Server {
            pub revision_source: RevisionSource,
            pub known: bool,
        }

        /// The board record is read from OTP at boot: a `fn` (`const_model`), served without a handler.
        impl api_impl::BoardInfoTestConsts for Server {
            fn board_board(&self) -> BoardSummary<'_> {
                // what the device doesn't know is empty or None
                BoardSummary {
                    name: MaxLen::new_unchecked("B200"),
                    revision: MaxLen::new_unchecked("B"),
                    revision_source: self.revision_source,
                    variant: MaxLen::new_unchecked(if self.known { "usb-io" } else { "" }),
                    bom_variant: MaxLen::new_unchecked(""),
                    serial: MaxLen::new_unchecked(if self.known { "B200-0042" } else { "" }),
                    build_date: self.known.then(|| {
                        ww_board_info::DateTime::from_ymd_hms_utc_opt(2026, 9, 1, 0, 0, 0, 0)
                            .unwrap()
                    }),
                }
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                board_info_api :: BoardInfoTest for super::Server,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
                const_model = "BOARD=fn",
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
                board_info_api :: BoardInfoTest for super::StdClient,
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

    fn start(path: &str, revision_source: RevisionSource, known: bool) -> TestDevice {
        let _ = tracing_subscriber::fmt::try_init();
        tests_common::start_device(
            path,
            server::Server {
                revision_source,
                known,
            },
            board_info_api::BOARD_INFO_TEST_FULL_GID,
            server::api_impl::api_hash(),
        )
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn revision_from_record() {
        let device = start("board_info/record", RevisionSource::Record, true);
        let client = connect(&device).await;
        let b = client.board().read_BOARD().read().await.unwrap();
        assert_eq!(*b.name, "B200");
        assert_eq!(*b.revision, "B");
        assert_eq!(b.revision_source, RevisionSource::Record);
        assert_eq!(*b.variant, "usb-io");
        assert_eq!(*b.serial, "B200-0042");
        assert!(b.build_date.is_some());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn revision_from_strap_keeps_raw_value() {
        let device = start(
            "board_info/strap",
            RevisionSource::Strap { value: 0x2a5 },
            true,
        );
        let client = connect(&device).await;
        let b = client.board().read_BOARD().read().await.unwrap();
        assert_eq!(b.revision_source, RevisionSource::Strap { value: 0x2a5 });
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn assumed_revision_and_unknown_fields() {
        let device = start("board_info/assumed", RevisionSource::Assumed, false);
        let client = connect(&device).await;
        let b = client.board().read_BOARD().read().await.unwrap();
        assert_eq!(b.revision_source, RevisionSource::Assumed);
        assert!(b.variant.is_empty() && b.bom_variant.is_empty() && b.serial.is_empty());
        assert!(b.build_date.is_none());
    }

    #[test]
    fn revision_source_wire() {
        // Strap carries a u16 after a one nibble discriminant
        let mut buf = [0u8; 8];
        let mut wr = BufWriter::new(&mut buf);
        wr.write(&RevisionSource::Strap { value: 0x2a5 }).unwrap();
        let bytes = wr.finish_and_take().unwrap();
        let mut rd = BufReader::new(bytes);
        assert_eq!(
            rd.read::<RevisionSource>().unwrap(),
            RevisionSource::Strap { value: 0x2a5 }
        );
    }
}
