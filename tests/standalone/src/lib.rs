//! Types of a crate that isn't a wire_weaver API crate (API-60): `standalone_units` has `shrink_wrap` as a plain
//! dependency and no `#[ww_trait]`; `standalone_api` uses its types and re-exports it. This crate doesn't depend
//! on `standalone_units`, so the generated server and client name its types through `standalone_api`'s re-export.

#[cfg(test)]
mod tests {
    use standalone_api::Supply;
    use standalone_api::standalone_units::units::{Ampere, Volt};
    use standalone_api::standalone_units::{Milli, Source};
    use wire_weaver_client::{ClientConfig, Commander, WwClient};

    mod device {
        use super::*;
        use tests_common::TestProcessEvents;
        use wire_weaver::BlockingEventOut;
        use wire_weaver::prelude::*;

        pub struct Device {
            pub limit: Milli<Ampere>,
        }

        impl Device {
            fn supply(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<Supply> {
                RpcResult::Ready(Supply {
                    voltage: Milli::new(3300),
                    source: Source::Estimated { confidence: 90 },
                })
            }

            fn set_limit(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                current: Milli<Ampere>,
            ) -> RpcResult<()> {
                self.limit = current;
                RpcResult::Ready(())
            }

            fn get_limit(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<Milli<Ampere>, ()> {
                Value(self.limit)
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                standalone_api :: Standalone for super::Device,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
                introspect = "no_docs",
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
            standalone_api :: Standalone for super::Host,
            client = "std_client",
        );
    }

    async fn connect(path: &str) -> (tests_common::TestDevice, Host) {
        let _ = tracing_subscriber::fmt::try_init();
        let device = tests_common::start_device(
            path,
            device::Device {
                limit: Milli::new(500),
            },
            standalone_api::STANDALONE_FULL_GID,
            device::api_impl::api_hash(),
        );
        let host = Host::config(|c| c.in_process_path(device.path()))
            .connect()
            .await
            .unwrap();
        (device, host)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn standalone_types_are_exchanged() {
        let (_device, host) = connect("standalone/exchange").await;
        let supply = host.supply().call().await.unwrap();
        assert_eq!(supply.voltage, Milli::<Volt>::new(3300));
        assert_eq!(supply.source, Source::Estimated { confidence: 90 });
        assert_eq!(host.read_limit().read().await.unwrap(), Milli::new(500));
        host.set_limit(Milli::new(750)).call().await.unwrap();
        assert_eq!(host.read_limit().read().await.unwrap(), Milli::new(750));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn introspection_names_the_defining_crate() {
        use ww_self::{TypeLocationOwned, TypeOwned};
        let (_device, host) = connect("standalone/introspect").await;
        let bundle = host.cmd.introspect().get().await.unwrap().unwrap();
        let crates: Vec<_> = bundle
            .ext_crates
            .iter()
            .map(|c| c.crate_id.as_str())
            .collect();
        assert!(crates.contains(&"standalone_units"), "{crates:?}");
        let milli = bundle.types.iter().find_map(|l| match l {
            TypeLocationOwned::InLine {
                ty: TypeOwned::Struct(s),
                crate_idx,
            } if s.ident == "Milli" => Some(crate_idx.0),
            _ => None,
        });
        let crate_idx = milli.expect("Milli in the device's API");
        assert_eq!(
            bundle.ext_crates[crate_idx as usize].crate_id,
            "standalone_units"
        );
    }
}
