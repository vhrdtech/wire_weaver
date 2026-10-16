//! `#[cfg]` enum variants (DER-8) across builds: a device built from `cfg_variants_narrow_api` (without the `wide`
//! and `float` features, as firmware) and a host built from `cfg_variants_api` with every variant connect, the
//! compatibility check passes, and the variants both have are exchanged. Introspection lists every variant whatever
//! features are on, so type signatures don't depend on them either.

#[cfg(test)]
mod tests {
    use wire_weaver_client::{ClientConfig, Commander, WwClient};

    /// Firmware: `Reading` without `Wide` and `Float`.
    mod narrow_device {
        use cfg_variants_narrow_api::Reading;
        use tests_common::TestProcessEvents;
        use wire_weaver::BlockingEventOut;
        use wire_weaver::prelude::*;

        pub struct Device {
            pub reading: Reading,
        }

        impl Device {
            fn read(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<Reading> {
                RpcResult::Ready(self.reading)
            }

            fn write(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                reading: Reading,
            ) -> RpcResult<()> {
                self.reading = reading;
                RpcResult::Ready(())
            }

            fn supply(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<ww_si::Volt> {
                RpcResult::Ready(ww_si::Volt {
                    prefix: ww_si::Prefix::Milli,
                    value: ww_si::ww_numeric::NumericValue::I32(3300),
                })
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                cfg_variants_narrow_api :: CfgVariants for super::Device,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
            );
        }

        impl TestProcessEvents for Device {
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

    /// Host: every variant.
    pub struct Host {
        cmd: Commander,
    }

    impl WwClient for Host {
        fn default_config() -> ClientConfig {
            ClientConfig::default()
        }

        fn from_cmd(cmd: Commander) -> Self {
            Self { cmd }
        }
    }

    mod api_client {
        wire_weaver::ww_codegen!(
            cfg_variants_api :: CfgVariants for super::Host,
            client = "std_client",
        );
    }

    async fn connect(path: &str) -> (tests_common::TestDevice, Host) {
        let _ = tracing_subscriber::fmt::try_init();
        let device = tests_common::start_device(
            path,
            narrow_device::Device {
                reading: cfg_variants_narrow_api::Reading::Small(0),
            },
            // what the firmware reports: the same API crate as the host's, the same version
            cfg_variants_api::CFG_VARIANTS_FULL_GID,
            narrow_device::api_impl::api_hash(),
        );
        let host = Host::config(|c| c.in_process_path(device.path()))
            .connect()
            .await
            .expect("a build with fewer variants must connect");
        (device, host)
    }

    #[test]
    fn both_builds_have_the_same_source() {
        assert_eq!(
            include_str!("../../cfg_variants_api/src/lib.rs"),
            include_str!("../../cfg_variants_narrow_api/src/lib.rs"),
        );
    }

    #[test]
    fn builds_have_different_variants() {
        // the narrow build really is narrow: no u64 and f32 variants
        assert!(
            size_of::<cfg_variants_narrow_api::Reading>() < size_of::<cfg_variants_api::Reading>()
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shared_variants_and_skipped_types_are_exchanged() {
        use cfg_variants_api::Reading;
        let (_device, mut host) = connect("cfg_variants/shared").await;
        assert_eq!(host.read().call().await.unwrap(), Reading::Small(0));
        host.write(Reading::Small(7)).call().await.unwrap();
        assert_eq!(host.read().call().await.unwrap(), Reading::Small(7));
        host.write(Reading::Missing).call().await.unwrap();
        assert_eq!(host.read().call().await.unwrap(), Reading::Missing);
        let volt = host.supply().call().await.unwrap();
        assert_eq!(volt.prefix, ww_si::Prefix::Milli);
        assert_eq!(volt.value, ww_si::ww_numeric::NumericValue::I32(3300));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn introspection_lists_every_variant() {
        use ww_self::{TypeLocationOwned, TypeOwned};
        let (_device, host) = connect("cfg_variants/introspect").await;
        let bundle = host.cmd.introspect().get().await.unwrap().unwrap();
        let mut reading = None;
        for location in &bundle.types {
            match location {
                TypeLocationOwned::InLine {
                    ty: TypeOwned::Enum(e),
                    ..
                } if e.ident == "Reading" => reading = Some(e),
                // put back only when the device's signature matches the host's snapshot
                TypeLocationOwned::SkippedFullVersion { type_name, .. } => {
                    panic!("{type_name} is not put back")
                }
                _ => {}
            }
        }
        let variants: Vec<_> = reading
            .expect("Reading in the device's API")
            .variants
            .iter()
            .map(|v| (v.ident.as_str(), v.discriminant.0))
            .collect();
        // the device doesn't have Wide and Float, its introspection does
        assert_eq!(
            variants,
            [("Small", 0), ("Wide", 1), ("Float", 2), ("Missing", 3)]
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn variant_the_device_lacks_is_refused_by_it() {
        use cfg_variants_api::Reading;
        let (_device, mut host) = connect("cfg_variants/lacking").await;
        let e = host.write(Reading::Wide(1 << 40)).call().await.err();
        // the device reads it as `EnumVariantDisabled` and answers so
        assert!(format!("{e:?}").contains("ArgsDesFailed"), "{e:?}");
        // the device is still fine and kept the old value
        assert_eq!(host.read().call().await.unwrap(), Reading::Small(0));
    }
}
