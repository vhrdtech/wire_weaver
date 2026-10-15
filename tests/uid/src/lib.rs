//! `ww_uid` end to end: a mock server on `ww_device::Server` through `tests_common::start_device`, the generated
//! client through the real host event loop.

#[cfg(test)]
mod tests {
    use tests_common::TestDevice;
    use wire_weaver::prelude::*;
    use ww_uid::{UidKind, UidKindOwned, UidSource, UidSourceOwned, UniqueId};

    const CHIP: [u8; 8] = [0xe6, 0x61, 0xa4, 0x10, 0x43, 0x1b, 0x6d, 0x2f];
    const MAC: [u8; 6] = [0x02, 0x00, 0x5e, 0x10, 0x00, 0x42];
    const OTHER: [u8; 3] = [1, 2, 3];

    mod server {
        use super::*;
        use tests_common::TestProcessEvents;
        use wire_weaver::BlockingEventOut;

        pub struct Server {
            pub ids: Vec<UniqueId<'static>>,
        }

        /// Ids are read from the chip at boot: a `fn` (`const_model`), served without a handler.
        impl api_impl::UidTestConsts for Server {
            fn uid_ids(&self) -> MaxLen<RefVec<'_, UniqueId<'_>>, 4> {
                MaxLen::new_unchecked(RefVec::Slice { slice: &self.ids })
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                uid_api :: UidTest for super::Server,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
                const_model = "IDS=fn",
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
                uid_api :: UidTest for super::StdClient,
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

    fn all_ids() -> Vec<UniqueId<'static>> {
        vec![
            // the first is the primary id: the USB serial
            UniqueId {
                kind: UidKind::ChipUnique,
                source: UidSource::InternalIc,
                value: MaxLen::new_unchecked(RefVec::new_bytes(&CHIP)),
            },
            UniqueId {
                kind: UidKind::Mac48,
                source: UidSource::ExternalIc,
                value: MaxLen::new_unchecked(RefVec::new_bytes(&MAC)),
            },
            UniqueId {
                kind: UidKind::Other(MaxLen::new_unchecked("fab-lot")),
                source: UidSource::Other(MaxLen::new_unchecked("label")),
                value: MaxLen::new_unchecked(RefVec::new_bytes(&OTHER)),
            },
        ]
    }

    fn start(path: &str, ids: Vec<UniqueId<'static>>) -> TestDevice {
        let _ = tracing_subscriber::fmt::try_init();
        tests_common::start_device(
            path,
            server::Server { ids },
            uid_api::UID_TEST_FULL_GID,
            server::api_impl::api_hash(),
        )
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ids_in_order_primary_first() {
        let device = start("uid/all", all_ids());
        let client = connect(&device).await;
        let ids = client.uid().read_IDS().read().await.unwrap();
        assert_eq!(ids.len(), 3);
        assert_eq!(ids[0].kind, UidKindOwned::ChipUnique);
        assert_eq!(ids[0].source, UidSourceOwned::InternalIc);
        assert_eq!(ids[0].value.as_slice(), CHIP);
        assert_eq!(ids[1].kind, UidKindOwned::Mac48);
        assert_eq!(ids[1].value.as_slice(), MAC);
        assert_eq!(&*ids[2].value, &OTHER);
        assert!(matches!(&ids[2].kind, UidKindOwned::Other(n) if &**n == "fab-lot"));
        assert!(matches!(&ids[2].source, UidSourceOwned::Other(n) if &**n == "label"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn device_without_ids_answers_empty_list() {
        let device = start("uid/none", Vec::new());
        let client = connect(&device).await;
        assert!(client.uid().read_IDS().read().await.unwrap().is_empty());
    }

    /// The primary id's hex is the USB serial string, so the host can find the unit after a reboot to a ROM
    /// bootloader that uses the same id.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn primary_id_hex_is_the_usb_serial() {
        let device = start("uid/serial", all_ids());
        let client = connect(&device).await;
        let ids = client.uid().read_IDS().read().await.unwrap();
        let serial: String = ids[0].value.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(serial, "e661a410431b6d2f");
    }
}
