//! One sub-level client for a trait two lines share (CLT-68). `shared_sub_level_api` has line 1 (w1.1) and line 2
//! (w2.1) in one crate; both reach `common::Channel` and `ww_indication::Indication`, at other places: the channel on
//! line 1's root, under `probe` and as the `aux[]` array on line 2. `ChannelClient` and `IndicationClient` are
//! generated once (`client = "sub_level"`), and each line's client returns them (`sub_levels = [..]`), so code over
//! a channel or the indication is written once for both lines. The calls still go by path, through the line's own
//! `Commander`, so each line's `#[since]` still decides what its device's revision has.

#[cfg(test)]
mod tests {
    use shared_sub_level_api::common::Code;
    use std::path::Path;
    use wire_weaver::prelude::*;
    use wire_weaver::ww_version::{ApiHash, ApiHashPair, WireVersion};
    use wire_weaver_client::{ClientConfig, Commander, Error, WwClient};
    use ww_indication::Mode;

    /// The shared clients, generated once: one `ww_codegen!` per module, as for any client.
    mod channel_client {
        wire_weaver::ww_codegen!(
            shared_sub_level_api :: common :: Channel for ChannelClient,
            client = "sub_level",
        );
    }
    mod indication_client {
        wire_weaver::ww_codegen!(
            ww_indication :: Indication for IndicationClient,
            client = "sub_level",
        );
    }
    use channel_client::ChannelClient;
    use indication_client::IndicationClient;

    /// Firmware of line 1.
    mod device_v1 {
        use super::*;

        pub struct Device {
            pub mode: Mode,
        }

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

            pub(super) fn set_indication_mode(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: Mode,
            ) -> SetResult<()> {
                self.mode = value;
                Set
            }

            pub(super) fn get_indication_mode(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<Mode, ()> {
                Value(self.mode)
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                shared_sub_level_api :: wire_v1 :: Thermo for super::Device,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
                omit = ["indication.brightness", "indication.identify", "indication.NAMES"],
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

        pub struct Device {
            pub mode: Mode,
        }

        impl Device {
            pub(super) fn read_once(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                average: u8,
            ) -> RpcResult<Code> {
                RpcResult::Ready(Code(200 + average as u16))
            }

            pub(super) fn probe_channel_read(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<Code> {
                RpcResult::Ready(Code(2))
            }

            pub(super) fn probe_channel_reset(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<()> {
                RpcResult::Ready(())
            }

            pub(super) fn valid_indices_root_aux(&mut self) -> ValidIndices<'_> {
                ValidIndices::range_u32(0..4)
            }

            pub(super) fn aux_read(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index: [UNib32; 1],
            ) -> RpcResult<Code> {
                RpcResult::Ready(Code(20 + index[0].0 as u16))
            }

            pub(super) fn aux_reset(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _index: [UNib32; 1],
            ) -> RpcResult<()> {
                RpcResult::Ready(())
            }

            pub(super) fn set_indication_mode(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: Mode,
            ) -> SetResult<()> {
                self.mode = value;
                Set
            }

            pub(super) fn get_indication_mode(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<Mode, ()> {
                Value(self.mode)
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                shared_sub_level_api :: wire_v2 :: Thermo for super::Device,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
                omit = ["indication.brightness", "indication.identify", "indication.NAMES"],
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

    /// A host of line 1: its `channel()` and `indication()` return the shared clients.
    mod client_v1 {
        use super::*;

        pub struct Client {
            pub cmd: Commander,
        }

        impl WwClient for Client {
            fn default_config() -> ClientConfig {
                ClientConfig::default()
            }

            fn from_cmd(cmd: Commander) -> Self {
                Self { cmd }
            }
        }

        mod api_client {
            wire_weaver::ww_codegen!(
                shared_sub_level_api :: wire_v1 :: Thermo for super::Client,
                client = "std_client",
                sub_levels = [
                    shared_sub_level_api::common::Channel => crate::tests::ChannelClient,
                    ww_indication::Indication => crate::tests::IndicationClient,
                ],
            );
        }
    }

    /// A host of line 2: `probe().channel()`, `aux(i)` and `indication()` return the same shared clients.
    mod client_v2 {
        use super::*;

        pub struct Client {
            pub cmd: Commander,
        }

        impl WwClient for Client {
            fn default_config() -> ClientConfig {
                ClientConfig::default()
            }

            fn from_cmd(cmd: Commander) -> Self {
                Self { cmd }
            }
        }

        mod api_client {
            wire_weaver::ww_codegen!(
                shared_sub_level_api :: wire_v2 :: Thermo for super::Client,
                client = "std_client",
                sub_levels = [
                    shared_sub_level_api::common::Channel => crate::tests::ChannelClient,
                    ww_indication::Indication => crate::tests::IndicationClient,
                ],
            );
        }
    }

    /// Code over a channel, written once for both lines.
    async fn read_and_reset(channel: ChannelClient<'_>) -> Result<Code, Error> {
        let code = channel.read().call().await?;
        channel.reset().call().await?;
        Ok(code)
    }

    /// Code over the indication, written once for both lines.
    async fn toggle(indication: IndicationClient<'_>) -> Mode {
        indication.write_mode(Mode::AllOff).write().await.unwrap();
        indication.read_mode().read().await.unwrap()
    }

    /// A device of `wire` built from this crate; a device of an older revision gets another API hash, as it would
    /// have, so the host checks `#[since]` instead of taking the identical fast path.
    fn start(path: &str, line: u8, wire: WireVersion) -> tests_common::TestDevice {
        let _ = tracing_subscriber::fmt::try_init();
        const OTHER: [u8; 8] = [0xA5; 8];
        let other = ApiHashPair {
            no_docs: ApiHash::new(&OTHER),
            with_docs: ApiHash::new(&OTHER),
        };
        let mode = Mode::Normal;
        match line {
            1 => tests_common::start_device(
                path,
                device_v1::Device { mode },
                shared_sub_level_api::wire_v1::THERMO_FULL_GID,
                wire,
                if wire == shared_sub_level_api::wire_v1::THERMO_WIRE_VERSION {
                    device_v1::api_impl::api_hash()
                } else {
                    other
                },
            ),
            _ => tests_common::start_device(
                path,
                device_v2::Device { mode },
                shared_sub_level_api::wire_v2::THERMO_FULL_GID,
                wire,
                if wire == shared_sub_level_api::wire_v2::THERMO_WIRE_VERSION {
                    device_v2::api_impl::api_hash()
                } else {
                    other
                },
            ),
        }
    }

    /// The same shared clients work on both lines, at every place each line has the trait.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn one_client_per_trait_for_both_lines() {
        let d1 = start("shared_sub_level/v1", 1, WireVersion::new(1, 1));
        let d2 = start("shared_sub_level/v2", 2, WireVersion::new(2, 1));
        let c1 = client_v1::Client::config(|c| c.in_process_path(d1.path()))
            .connect()
            .await
            .unwrap();
        let c2 = client_v2::Client::config(|c| c.in_process_path(d2.path()))
            .connect()
            .await
            .unwrap();

        assert_eq!(read_and_reset(c1.channel()).await.unwrap(), Code(1));
        assert_eq!(read_and_reset(c2.probe().channel()).await.unwrap(), Code(2));
        assert_eq!(read_and_reset(c2.aux(3)).await.unwrap(), Code(23));

        assert_eq!(toggle(c1.indication()).await, Mode::AllOff);
        assert_eq!(toggle(c2.indication()).await, Mode::AllOff);

        // the line's own items are still its own
        assert_eq!(c1.read_once().call().await.unwrap(), Code(100));
        assert_eq!(c2.read_once(3).call().await.unwrap(), Code(203));
    }

    /// `#[since = "w1.1, w2.1"]` of the shared trait: the shared client has no revision, each line's `Commander`
    /// checks its own, so `reset` is refused on a w1.0 and on a w2.0 device and `read` still works.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn each_line_checks_its_own_since() {
        let d1 = start("shared_sub_level/since_v1", 1, WireVersion::new(1, 0));
        let d2 = start("shared_sub_level/since_v2", 2, WireVersion::new(2, 0));
        let c1 = client_v1::Client::config(|c| c.in_process_path(d1.path()))
            .connect()
            .await
            .unwrap();
        let c2 = client_v2::Client::config(|c| c.in_process_path(d2.path()))
            .connect()
            .await
            .unwrap();

        for (channel, required) in [
            (c1.channel(), WireVersion::new(1, 1)),
            (c2.probe().channel(), WireVersion::new(2, 1)),
            (c2.aux(0), WireVersion::new(2, 1)),
        ] {
            channel.read().call().await.unwrap();
            match channel.reset().call().await {
                Err(Error::OlderProtocol { required: r, .. }) => assert_eq!(r, required),
                other => panic!("expected OlderProtocol needing {required}, got {other:?}"),
            }
        }
    }

    fn api_crate() -> &'static Path {
        Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../shared_sub_level_api"
        ))
    }

    /// The signature each line's client checks: the trait as the shared client was generated from it, the same as
    /// each line has it although each line's `#[since]` names another revision; another trait has another.
    #[test]
    fn signature_leaves_since_out() {
        use wire_weaver_core::codegen::api_client::shared_signature;
        use wire_weaver_core::internal::ww_self::ApiLevelLocationOwned;
        let alone =
            wire_weaver_core::load(api_crate(), Some("common::Channel".into()), false).unwrap();
        assert_eq!(alone.root.items[1].since, None, "no line: no revision");
        let alone = shared_signature(&alone, None).unwrap();
        assert_eq!(alone, ChannelClient::WW_SIGNATURE);
        for root in ["wire_v1::Thermo", "wire_v2::Thermo"] {
            let bundle = wire_weaver_core::load(api_crate(), Some(root.into()), false).unwrap();
            let idx = bundle
                .traits
                .iter()
                .position(|t| matches!(t, ApiLevelLocationOwned::InLine { level, .. } if level.trait_name == "Channel"))
                .expect("Channel in the bundle");
            let ApiLevelLocationOwned::InLine { level, .. } = &bundle.traits[idx] else {
                unreachable!()
            };
            assert!(
                level.items[1].since.is_some(),
                "{root}: the line's revision"
            );
            assert_eq!(
                shared_signature(&bundle, Some(idx as u32)).unwrap(),
                alone,
                "{root}"
            );
        }
        assert_ne!(IndicationClient::WW_SIGNATURE, ChannelClient::WW_SIGNATURE);
    }
}
