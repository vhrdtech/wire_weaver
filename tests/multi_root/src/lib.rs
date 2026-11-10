//! Two wire lines of one API in one crate (EVO-37): `multi_root_api::wire_v1::Thermo` at w1.2 and
//! `multi_root_api::wire_v2::Thermo` at w2.1. A server and a client of each line are generated in this one crate,
//! each from its root's module path; each device reports its own line in link setup and each client connects only to
//! its own. The sub-trait both lines share is one trait of `common`, and its `#[since = "w1.2, w2.1"]` takes the
//! revision of the line it is loaded for.

#[cfg(test)]
mod tests {
    use multi_root_api::common::Code;
    use std::path::Path;
    use wire_weaver::prelude::*;
    use wire_weaver::ww_version::WireVersion;
    use wire_weaver_client::{ClientConfig, Commander, WwClient};

    /// Firmware of line 1.
    mod device_v1 {
        use super::*;

        pub struct Device;

        impl Device {
            pub(super) fn read_once(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<Code> {
                RpcResult::Ready(Code(100))
            }

            pub(super) fn channel_read(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<Code> {
                RpcResult::Ready(Code(1))
            }

            pub(super) fn channel_reset(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<()> {
                RpcResult::Ready(())
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                multi_root_api :: wire_v1 :: Thermo for super::Device,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
            );
        }

        impl tests_common::TestProcessEvents for Device {
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

    /// Firmware of line 2.
    mod device_v2 {
        use super::*;

        pub struct Device;

        impl Device {
            pub(super) fn read_once(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                average: u8,
            ) -> RpcResult<Code> {
                RpcResult::Ready(Code(200 + average as u16))
            }

            pub(super) fn channel_read(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<Code> {
                RpcResult::Ready(Code(2))
            }

            pub(super) fn channel_reset(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<()> {
                RpcResult::Ready(())
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                multi_root_api :: wire_v2 :: Thermo for super::Device,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
            );
        }

        impl tests_common::TestProcessEvents for Device {
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

    /// A host of line 1.
    mod client_v1 {
        use super::*;

        pub struct Client {
            pub cmd: Commander,
        }

        impl WwClient for Client {
            fn default_config() -> ClientConfig {
                ClientConfig::default()
            }

            fn try_from_cmd(cmd: Commander) -> Result<Self, wire_weaver_client::Error> {
                Ok(Self { cmd })
            }
        }

        mod api_client {
            wire_weaver::ww_codegen!(
                multi_root_api :: wire_v1 :: Thermo for super::Client,
                client = "std_client",
            );
        }
    }

    /// A host of line 2, in the same crate as the one of line 1.
    mod client_v2 {
        use super::*;

        pub struct Client {
            pub cmd: Commander,
        }

        impl WwClient for Client {
            fn default_config() -> ClientConfig {
                ClientConfig::default()
            }

            fn try_from_cmd(cmd: Commander) -> Result<Self, wire_weaver_client::Error> {
                Ok(Self { cmd })
            }
        }

        mod api_client {
            wire_weaver::ww_codegen!(
                multi_root_api :: wire_v2 :: Thermo for super::Client,
                client = "std_client",
            );
        }
    }

    fn start_v1(path: &str) -> tests_common::TestDevice {
        let _ = tracing_subscriber::fmt::try_init();
        tests_common::start_device(
            path,
            device_v1::Device,
            multi_root_api::wire_v1::THERMO_FULL_GID,
            multi_root_api::wire_v1::THERMO_WIRE_VERSION,
            device_v1::api_impl::api_hash(),
        )
    }

    fn start_v2(path: &str) -> tests_common::TestDevice {
        let _ = tracing_subscriber::fmt::try_init();
        tests_common::start_device(
            path,
            device_v2::Device,
            multi_root_api::wire_v2::THERMO_FULL_GID,
            multi_root_api::wire_v2::THERMO_WIRE_VERSION,
            device_v2::api_impl::api_hash(),
        )
    }

    fn api_crate() -> &'static Path {
        Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../multi_root_api"))
    }

    /// Each root's macro takes its own entry of the `roots` table, though both traits are named `Thermo`.
    #[test]
    fn each_root_has_its_wire_version() {
        assert_eq!(
            multi_root_api::wire_v1::THERMO_WIRE_VERSION,
            WireVersion::new(1, 2)
        );
        assert_eq!(
            multi_root_api::wire_v2::THERMO_WIRE_VERSION,
            WireVersion::new(2, 1)
        );
    }

    /// The walker loads a root by its module path, with its wire version; `#[since]` of the shared sub-trait takes
    /// the revision of that root's line.
    #[test]
    fn walker_loads_each_root_by_module_path() {
        for (root, wire) in [
            ("wire_v1::Thermo", WireVersion::new(1, 2)),
            ("wire_v2::Thermo", WireVersion::new(2, 1)),
        ] {
            let bundle = wire_weaver_core::load(api_crate(), Some(root.into()), false).unwrap();
            assert_eq!(bundle.wire, Some(wire), "{root}");
            let channel = bundle
                .traits
                .iter()
                .find_map(|t| match t {
                    wire_weaver_core::internal::ww_self::ApiLevelLocationOwned::InLine {
                        level,
                        ..
                    } if level.trait_name == "Channel" => Some(level),
                    _ => None,
                })
                .expect("Channel in the bundle");
            let reset = channel.items.iter().find(|i| i.ident == "reset").unwrap();
            assert_eq!(reset.since, Some(wire), "{root}");
        }
    }

    /// Two roots and no module path: the error names both.
    #[test]
    fn several_roots_need_a_module_path() {
        let e = wire_weaver_core::load(api_crate(), None, false).unwrap_err();
        let e = format!("{e:?}");
        assert!(
            e.contains("`wire_v1::Thermo`") && e.contains("`wire_v2::Thermo`"),
            "{e}"
        );
        let e = wire_weaver_core::load(api_crate(), Some("Thermo".into()), false).unwrap_err();
        let e = format!("{e:?}");
        assert!(e.contains("name one by its module"), "{e}");
    }

    /// Each device reports its own line in link setup, each host talks to its own, the shared sub-level included.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn each_line_talks_to_its_own() {
        let d1 = start_v1("multi_root/v1");
        let d2 = start_v2("multi_root/v2");
        let c1 = client_v1::Client::config(|c| c.in_process_path(d1.path()))
            .connect()
            .await
            .expect("a line 1 host connects to a line 1 device");
        assert_eq!(
            c1.cmd.device_api_info().user_api_wire,
            Some(WireVersion::new(1, 2))
        );
        assert_eq!(c1.read_once().call().await.unwrap(), Code(100));
        assert_eq!(c1.channel().read().call().await.unwrap(), Code(1));
        c1.channel().reset().call().await.unwrap();

        let c2 = client_v2::Client::config(|c| c.in_process_path(d2.path()))
            .connect()
            .await
            .expect("a line 2 host connects to a line 2 device");
        assert_eq!(
            c2.cmd.device_api_info().user_api_wire,
            Some(WireVersion::new(2, 1))
        );
        assert_eq!(c2.read_once(3).call().await.unwrap(), Code(203));
        assert_eq!(c2.channel().read().call().await.unwrap(), Code(2));
        c2.channel().reset().call().await.unwrap();
    }

    /// A host of line 2 is refused by a device of line 1, naming both lines.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_other_line_is_refused() {
        let d1 = start_v1("multi_root/refused");
        let e = client_v2::Client::config(|c| c.in_process_path(d1.path()))
            .connect()
            .await
            .err()
            .expect("a line 2 host must not connect to a line 1 device");
        let e = e.to_string();
        assert!(e.contains("w1.2") && e.contains("w2.1"), "{e}");
    }
}
