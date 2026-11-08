//! Link setup decides on the wire line, not the crate version (EVO-25): firmware of `wire_line_api` 0.3.0 at wire
//! w2.3 (`wire_line_old_api`, built without introspection) and a host built against 0.4.0 at w2.4, a crate breaking
//! bump on the same line. They connect; the host learns the device's w2.3 from link setup and refuses the w2.4 method
//! with `OlderProtocol` before sending anything. The same firmware on another line is refused, naming both lines.

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};
    use wire_weaver::prelude::*;
    use wire_weaver::ww_version::{FullVersion, Version, WireVersion};
    use wire_weaver_client::Error;

    /// Firmware already in the field: built from the 0.3.0 API at w2.3.
    mod old_device {
        use super::*;

        pub struct OldDevice {
            /// Requests that reached the device
            pub requests: Arc<AtomicU32>,
        }

        impl OldDevice {
            pub(super) fn led_on(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<()> {
                RpcResult::Ready(())
            }

            pub(super) fn led_off(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<()> {
                RpcResult::Ready(())
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                wire_line_old_api :: WireLine for super::OldDevice,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
            );
        }

        impl tests_common::TestProcessEvents for OldDevice {
            type Medium = ();

            fn process_request_bytes<'a>(
                &mut self,
                bytes: &[u8],
                scratch: &'a mut [u8],
                out: &mut impl BlockingEventOut,
                medium: (),
            ) -> Result<&'a [u8], ShrinkWrapError> {
                self.requests.fetch_add(1, Ordering::Relaxed);
                self.process_request_bytes(bytes, scratch, out, medium)
            }
        }
    }

    /// Host built against the new 0.4.0 API at w2.4.
    mod new_client {
        use wire_weaver_client::{ClientConfig, Commander, WwClient};

        pub struct NewClient {
            pub cmd: Commander,
        }

        impl WwClient for NewClient {
            fn default_config() -> ClientConfig {
                ClientConfig::default()
            }

            fn from_cmd(cmd: Commander) -> Self {
                Self { cmd }
            }
        }

        mod api_client {
            wire_weaver::ww_codegen!(
                wire_line_api :: WireLine for super::NewClient,
                client = "std_client",
            );
        }
    }

    /// The old firmware, reporting itself as `wire_line_api` 0.3.0 at `wire`.
    fn start_old_device(
        path: &str,
        wire: WireVersion,
    ) -> (Arc<AtomicU32>, tests_common::TestDevice) {
        let _ = tracing_subscriber::fmt::try_init();
        let requests = Arc::new(AtomicU32::new(0));
        let device = tests_common::start_device(
            path,
            old_device::OldDevice {
                requests: requests.clone(),
            },
            FullVersion::new("wire_line_api", Version::new(0, 3, 0)),
            wire,
            old_device::api_impl::api_hash(),
        );
        (requests, device)
    }

    #[test]
    fn the_two_apis_are_on_one_line() {
        let (old, new) = (
            wire_line_old_api::WIRE_LINE_WIRE_VERSION,
            wire_line_api::WIRE_LINE_WIRE_VERSION,
        );
        assert_eq!((old, new), (WireVersion::new(2, 3), WireVersion::new(2, 4)));
        assert!(old.same_line(&new));
    }

    /// A host on w2.4 talks to a device on w2.3: it connects although its crate had a breaking bump, calls what both
    /// have, and gets `OlderProtocol` for the w2.4 method only, without a request reaching the device.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn newer_revision_connects_and_refuses_newer_items() {
        let (requests, device) = start_old_device(
            "wire_line/newer_revision",
            wire_line_old_api::WIRE_LINE_WIRE_VERSION,
        );
        let client = new_client::NewClient::config(|c| c.in_process_path(device.path()))
            .connect()
            .await
            .expect("a host on the device's wire line connects whatever its crate version");
        let info = client.cmd.device_api_info();
        assert_eq!(info.user_api_wire, Some(WireVersion::new(2, 3)));
        assert_eq!(info.user_api_version.version.minor.0, 3);

        client.led_on().call().await.unwrap();
        client.led_off().call().await.unwrap();
        let before = requests.load(Ordering::Relaxed);
        let e = client.led_toggle().call().await.unwrap_err();
        assert!(
            matches!(
                e,
                Error::OlderProtocol { device_wire, required, .. }
                    if device_wire == WireVersion::new(2, 3) && required == WireVersion::new(2, 4)
            ),
            "{e:?}"
        );
        assert!(e.to_string().contains("w2.4"), "{e}");
        assert_eq!(
            requests.load(Ordering::Relaxed),
            before,
            "nothing sent for led_toggle"
        );
    }

    /// The same firmware on another wire line is refused in link setup, the error naming both lines and who should
    /// update.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn another_line_is_refused() {
        for (wire, advice) in [
            (WireVersion::new(3, 0), "update the host"),
            (WireVersion::new(1, 7), "update the device firmware"),
        ] {
            let (requests, device) =
                start_old_device(&format!("wire_line/line_{}", wire.line.0), wire);
            let e = new_client::NewClient::config(|c| c.in_process_path(device.path()))
                .connect()
                .await
                .err()
                .expect("a host on another wire line must not connect");
            let Some(Error::ConnectFailed { reason, .. }) = e.downcast_ref::<Error>() else {
                panic!("{e:?}");
            };
            assert!(
                reason.contains(&format!("at wire {wire}"))
                    && reason.contains("at wire w2.4")
                    && reason.contains(advice),
                "{reason}"
            );
            assert_eq!(requests.load(Ordering::Relaxed), 0);
        }
    }
}
