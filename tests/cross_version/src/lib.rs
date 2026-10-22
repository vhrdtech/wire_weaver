//! Cross-version calls (API-20, CLT-10): a device running an old version of its API (`cross_version_old_api`,
//! reported as `cross_version_api` 0.3.0) is rebooted by a host built against the new version 0.4.0. The new
//! generated client can't connect (the user API check refuses it), but a `traits_only` connection can, and its
//! `ww_dfu::BootControl` calls reach the trait through a global trait path, wherever the old API put it.
//!
//! The trait clients send `GlobalCompact` paths (the trait's `ww_global` id): served like `GlobalFull`, and a
//! device built before the trait had an id (it answers `TraitNotFound` to every compact path) is asked again with
//! `GlobalFull`, and only that way from then on.

#[cfg(test)]
mod tests {
    use std::sync::{Arc, RwLock};
    use wire_weaver::prelude::*;
    use wire_weaver::ww_version::{CompactVersion, FullVersion, GlobalTypeId, Version};
    use wire_weaver_client::{Attachment, CallError, Commander, DynClient, Error};
    use ww_client_server::ErrorKindOwned;
    use ww_dfu::{BootState, BootTarget};

    /// What the old device was asked to do.
    #[derive(Default)]
    struct Calls {
        rebooted_into: Option<BootTarget>,
        led_on: bool,
        /// Path kind of every request: `compact <id>`, `full <crate>::<trait>` or `absolute`
        paths: Vec<String>,
        /// Answer every `GlobalCompact` path with `TraitNotFound`, as firmware built before API-20 did
        pre_compact: bool,
    }

    /// Firmware already in the field: built from the 0.3.0 API.
    mod old_device {
        use super::*;

        pub struct OldDevice {
            pub calls: Arc<RwLock<Calls>>,
        }

        impl OldDevice {
            fn led_on(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<()> {
                self.calls.write().unwrap().led_on = true;
                RpcResult::Ready(())
            }

            fn led_off(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<()> {
                Unimplemented.into()
            }

            fn device_uptime_s(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<u32> {
                RpcResult::Ready(42)
            }

            fn device_boot_boot_state(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<BootState> {
                RpcResult::Ready(BootState::Confirmed)
            }

            fn device_boot_confirm(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<Result<(), ww_dfu::Error>> {
                RpcResult::Ready(Ok(()))
            }

            fn device_boot_revert(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<Result<(), ww_dfu::Error>> {
                RpcResult::Ready(Err(ww_dfu::Error::Unsupported))
            }

            fn device_boot_reboot(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                into: BootTarget,
            ) -> RpcResult<Result<(), ww_dfu::Error>> {
                self.calls.write().unwrap().rebooted_into = Some(into);
                RpcResult::Ready(Ok(()))
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                cross_version_old_api :: CrossVersion for super::OldDevice,
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
                use ww_client_server::{PathKind, PathKindOwned, Request};
                let request = Request::from_ww_bytes(bytes)?;
                let mut calls = self.calls.write().unwrap();
                calls.paths.push(match &request.path_kind {
                    PathKind::Absolute { .. } => "absolute".into(),
                    PathKind::GlobalCompact { gid, .. } => format!("compact {}", gid.gid.id.0),
                    PathKind::GlobalFull {
                        gid, trait_name, ..
                    } => {
                        format!("full {}::{trait_name}", gid.crate_id)
                    }
                });
                if calls.pre_compact && matches!(request.path_kind, PathKind::GlobalCompact { .. })
                {
                    drop(calls);
                    // an id no server knows: the generated code answers TraitNotFound, as an old one did to any id
                    let mut old = request.make_owned()?;
                    if let PathKindOwned::GlobalCompact { gid, .. } = &mut old.path_kind {
                        gid.gid = ww_global::GlobalTypeId::new(4000);
                    }
                    let mut old = old.to_ww_bytes_owned()?;
                    let seq = Request::peek_seq(bytes)?;
                    let len = Request::set_seq(&mut old, seq)?.len();
                    let old = old.split_off(old.len() - len);
                    return self.process_request_bytes(&old, scratch, out, medium);
                }
                drop(calls);
                self.process_request_bytes(bytes, scratch, out, medium)
            }
        }
    }

    /// Host built against the new 0.4.0 API.
    mod new_client {
        use wire_weaver_client::{ClientConfig, Commander, WwClient};

        pub struct NewClient {
            #[allow(dead_code)] // never gets connected
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
                cross_version_api :: CrossVersion for super::NewClient,
                client = "std_client",
            );
        }
    }

    /// Generic `ww_dfu::BootControl` client, knows nothing about the device's API.
    struct BootControl {
        cmd: Commander,
    }

    impl BootControl {
        fn new(attachment: Attachment) -> Self {
            assert_eq!(attachment.trait_name(), "BootControl");
            Self {
                cmd: attachment.cmd_tx_take(),
            }
        }
    }

    mod boot_control_client {
        use super::*;
        wire_weaver::ww_codegen!(
            ww_dfu :: BootControl for BootControl,
            client = "trait_client",
            no_alloc = false,
            use_async = true,
        );
    }

    /// `ww_dfu::Inventory` client: the old device doesn't implement it.
    struct Inventory {
        cmd: Commander,
    }

    mod inventory_client {
        use super::*;
        wire_weaver::ww_codegen!(
            ww_dfu :: Inventory for Inventory,
            client = "trait_client",
            no_alloc = false,
            use_async = true,
        );
    }

    fn start_old_device(path: &str) -> (Arc<RwLock<Calls>>, tests_common::TestDevice) {
        start_device(path, false)
    }

    fn start_device(
        path: &str,
        pre_compact: bool,
    ) -> (Arc<RwLock<Calls>>, tests_common::TestDevice) {
        let _ = tracing_subscriber::fmt::try_init();
        let calls = Arc::new(RwLock::new(Calls {
            pre_compact,
            ..Calls::default()
        }));
        let server = old_device::OldDevice {
            calls: calls.clone(),
        };
        let device = tests_common::start_device(
            path,
            server,
            // what the old firmware reports: the same API crate as the host's, an incompatible version
            FullVersion::new("cross_version_api", Version::new(0, 3, 0)),
            old_device::api_impl::api_hash(),
        );
        (calls, device)
    }

    /// Without the connection's own clock steering (CLT-46): its `Clock::now()` try would be in the recorded calls.
    async fn connect_traits_only(device: &tests_common::TestDevice) -> DynClient {
        DynClient::traits_only(|c| c.in_process_path(device.path()).clock_sync(false))
            .connect()
            .await
            .expect("connect")
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn new_api_client_is_refused() {
        let (_calls, device) = start_old_device("cross_version/refused");
        let r = new_client::NewClient::config(|c| c.in_process_path(device.path()))
            .connect()
            .await;
        let e = r
            .err()
            .expect("an incompatible API version must not connect");
        assert!(
            matches!(e.downcast_ref::<Error>(), Some(Error::ConnectFailed { reason, .. }) if reason.contains("incompatible protocol")),
            "{e:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn old_device_rebooted_by_new_client() {
        let (calls, device) = start_old_device("cross_version/reboot");
        let client = connect_traits_only(&device).await;
        assert_eq!(
            client.device_api_info().user_api_version.crate_id,
            "cross_version_api"
        );

        let mut boot = BootControl::new(
            client.global_attachment(ww_dfu::BOOT_CONTROL_FULL_GID, "BootControl"),
        );
        let state = boot.boot_state().call().await.unwrap();
        assert_eq!(state, BootState::Confirmed);
        boot.reboot(BootTarget::Updater).call().await.unwrap();
        assert_eq!(
            calls.read().unwrap().rebooted_into,
            Some(BootTarget::Updater)
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn trait_not_implemented() {
        let (_calls, device) = start_old_device("cross_version/not_found");
        let client = connect_traits_only(&device).await;
        let mut inventory = Inventory {
            cmd: client.cmd().clone(),
        };
        let r = inventory.manifest_digest(ww_dfu::Slot::Active).call().await;
        assert!(
            matches!(&r, Err(CallError::Client(Error::RemoteError(e))) if matches!(e.kind, ErrorKindOwned::TraitNotFound)),
            "{r:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn incompatible_trait_version_not_found() {
        let (_calls, device) = start_old_device("cross_version/trait_version");
        let client = connect_traits_only(&device).await;
        // a ww_dfu 0.2 host: same trait, a version the device doesn't have
        let gid = FullVersion::new("ww_dfu", Version::new(0, 2, 0));
        let path = [UNib32(0)]; // boot_state
        let r = client
            .cmd()
            .prepare_call::<BootState, Error>(
                ww_client_server::PathKind::global(gid, None, "BootControl", &path),
                Ok(vec![]),
                "BootControl::boot_state",
            )
            .call()
            .await;
        assert!(
            matches!(&r, Err(Error::RemoteError(e)) if matches!(e.kind, ErrorKindOwned::TraitNotFound)),
            "{r:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn absolute_paths_refused_when_traits_only() {
        let (calls, device) = start_old_device("cross_version/absolute");
        let client = connect_traits_only(&device).await;
        // led_on of the old API, at [0]: the host can't know that is still what [0] means
        let path = [UNib32(0)];
        let r = client
            .cmd()
            .prepare_call::<(), Error>(
                ww_client_server::PathKind::absolute(&path),
                Ok(vec![]),
                "Old::led_on",
            )
            .call()
            .await;
        assert!(
            matches!(r, Err(Error::AbsolutePathInTraitsOnly(_))),
            "{r:?}"
        );
        assert!(!calls.read().unwrap().led_on);
    }

    fn paths(calls: &RwLock<Calls>) -> Vec<String> {
        calls.read().unwrap().paths.clone()
    }

    fn is_trait_not_found<T: std::fmt::Debug>(r: &Result<T, Error>) -> bool {
        matches!(r, Err(Error::RemoteError(e)) if matches!(e.kind, ErrorKindOwned::TraitNotFound))
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn compact_trait_path_served() {
        let (calls, device) = start_old_device("cross_version/compact");
        let client = connect_traits_only(&device).await;
        let id = ww_dfu::BOOT_CONTROL_COMPACT_GID
            .expect("BootControl has a ww_global id")
            .gid
            .id
            .0;
        let mut boot = BootControl::new(
            client.global_attachment(ww_dfu::BOOT_CONTROL_FULL_GID, "BootControl"),
        );
        assert_eq!(
            boot.boot_state().call().await.unwrap(),
            BootState::Confirmed
        );
        boot.reboot(BootTarget::Updater).call().await.unwrap();
        assert_eq!(
            calls.read().unwrap().rebooted_into,
            Some(BootTarget::Updater)
        );
        assert_eq!(
            paths(&calls),
            [format!("compact {id}"), format!("compact {id}")]
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn compact_trait_path_not_found() {
        let (calls, device) = start_old_device("cross_version/compact_not_found");
        let client = connect_traits_only(&device).await;
        let boot = ww_dfu::BOOT_CONTROL_COMPACT_GID.unwrap();
        let call = |gid: CompactVersion| {
            let path = [UNib32(0)]; // boot_state
            client
                .cmd()
                .prepare_call::<BootState, Error>(
                    ww_client_server::PathKind::GlobalCompact {
                        gid,
                        path_from_trait: RefVec::Slice { slice: &path },
                    },
                    Ok(vec![]),
                    "BootControl::boot_state",
                )
                .call()
        };
        // an id the registry doesn't have, a crate's id (ww_log), BootControl of an incompatible ww_dfu version
        let unknown = CompactVersion::new(GlobalTypeId::new(4000), 0, 1, 0);
        let crate_id = CompactVersion::new(ww_global::WW_LOG, 0, 1, 0);
        let old_version = CompactVersion::new(boot.gid, 0, 2, 0);
        for gid in [unknown, crate_id, old_version] {
            let r = call(gid).await;
            assert!(is_trait_not_found(&r), "{gid:?}: {r:?}");
        }
        assert_eq!(call(boot).await.unwrap(), BootState::Confirmed);
        // the incompatible version is asked again with GlobalFull (it could be a device from before the id), the
        // others have no trait to name; the compatible version still goes compact
        let id = boot.gid.id.0;
        assert_eq!(
            paths(&calls),
            [
                "compact 4000".to_string(),
                "compact 0".into(),
                format!("compact {id}"),
                "full ww_dfu::BootControl".into(),
                format!("compact {id}"),
            ]
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn device_without_compact_ids_asked_again_with_full_path() {
        let (calls, device) = start_device("cross_version/pre_compact", true);
        let client = connect_traits_only(&device).await;
        let id = ww_dfu::BOOT_CONTROL_COMPACT_GID.unwrap().gid.id.0;
        let mut boot = BootControl::new(
            client.global_attachment(ww_dfu::BOOT_CONTROL_FULL_GID, "BootControl"),
        );
        assert_eq!(
            boot.boot_state().call().await.unwrap(),
            BootState::Confirmed
        );
        boot.reboot(BootTarget::Updater).call().await.unwrap();
        assert_eq!(
            calls.read().unwrap().rebooted_into,
            Some(BootTarget::Updater)
        );
        // one TraitNotFound, then the connection remembers the trait goes by its full path
        assert_eq!(
            paths(&calls),
            [
                format!("compact {id}"),
                "full ww_dfu::BootControl".into(),
                "full ww_dfu::BootControl".into(),
            ]
        );
        // a new connection tries the compact path again
        client.cmd().disconnect().asynch().await.unwrap();
        let client = connect_traits_only(&device).await;
        let boot = BootControl::new(
            client.global_attachment(ww_dfu::BOOT_CONTROL_FULL_GID, "BootControl"),
        );
        boot.boot_state().call().await.unwrap();
        assert_eq!(
            paths(&calls)[3..],
            [format!("compact {id}"), "full ww_dfu::BootControl".into()]
        );
    }
}
