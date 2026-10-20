//! A whole `#[optional]` `ww_impl!` left out of a server (API-54): `traits_api::Board` with its `diag` trait and the
//! `probes` group (a trait array and a method), served in full, left out with `omit`, and with `omit_if`.

use super::*;

/// Every handler, nothing left out.
mod full_server {
    use super::*;

    pub struct FullServer;

    impl FullServer {
        fn ping(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<u8> {
            Ready(1)
        }

        fn valid_indices_root_probe(&mut self) -> ValidIndices<'_> {
            ValidIndices::range_u32(0..4)
        }

        fn probe_count(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<u8> {
            Ready(4)
        }

        fn diag_read(
            &mut self,
            _cx: &mut Context<'_, impl BlockingEventOut>,
            point: u16,
        ) -> RpcResult<u8> {
            Ready(point as u8 * 2)
        }

        fn diag_release_all(
            &mut self,
            _cx: &mut Context<'_, impl BlockingEventOut>,
        ) -> RpcResult<()> {
            Ready(())
        }

        fn diag_sub_m1(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<()> {
            Ready(())
        }

        fn probe_set_high(
            &mut self,
            _cx: &mut Context<'_, impl BlockingEventOut>,
            _index: [UNib32; 1],
        ) -> RpcResult<()> {
            Ready(())
        }
    }

    impl api_impl::BoardConsts for FullServer {
        const DIAG_POINTS: u16 = 64;
    }

    pub mod api_impl {
        wire_weaver::ww_codegen!(
            traits_api :: Board for super::FullServer,
            server = true, no_alloc = true, use_async = false,
            introspect = "no_docs",
        );
    }

    impl tests_common::TestProcessEvents for FullServer {
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

/// `diag` and the `probes` group left out: no handlers for them, no consts trait (its only const is in `diag`).
mod lean_server {
    use super::*;

    pub struct LeanServer;

    impl LeanServer {
        fn ping(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<u8> {
            Ready(1)
        }
    }

    pub mod api_impl {
        wire_weaver::ww_codegen!(
            traits_api :: Board for super::LeanServer,
            server = true, no_alloc = true, use_async = false,
            introspect = "no_docs",
            omit = ["diag", "probes"],
        );
    }

    impl tests_common::TestProcessEvents for LeanServer {
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

/// `omit_if(any())` keeps `diag` (the condition is false), `omit_if(all())` leaves the `probes` group out. The
/// handlers carry the same conditions, as a firmware ties them to its Cargo features.
mod cfg_server {
    use super::*;

    pub struct CfgServer;

    impl CfgServer {
        fn ping(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<u8> {
            Ready(1)
        }

        #[cfg(not(all()))]
        fn valid_indices_root_probe(&mut self) -> ValidIndices<'_> {
            ValidIndices::range_u32(0..4)
        }

        #[cfg(not(all()))]
        fn probe_count(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<u8> {
            Ready(4)
        }

        #[cfg(not(all()))]
        fn probe_set_high(
            &mut self,
            _cx: &mut Context<'_, impl BlockingEventOut>,
            _index: [UNib32; 1],
        ) -> RpcResult<()> {
            Ready(())
        }

        #[cfg(not(any()))]
        fn diag_read(
            &mut self,
            _cx: &mut Context<'_, impl BlockingEventOut>,
            point: u16,
        ) -> RpcResult<u8> {
            Ready(point as u8 * 2)
        }

        #[cfg(not(any()))]
        fn diag_release_all(
            &mut self,
            _cx: &mut Context<'_, impl BlockingEventOut>,
        ) -> RpcResult<()> {
            Ready(())
        }

        #[cfg(not(any()))]
        fn diag_sub_m1(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<()> {
            Ready(())
        }
    }

    impl api_impl::BoardConsts for CfgServer {
        #[cfg(not(any()))]
        const DIAG_POINTS: u16 = 64;
    }

    pub mod api_impl {
        wire_weaver::ww_codegen!(
            traits_api :: Board for super::CfgServer,
            server = true, no_alloc = true, use_async = false,
            introspect = "no_docs",
            omit_if(any()) = ["diag"],
            omit_if(all()) = ["probes"],
        );
    }

    impl tests_common::TestProcessEvents for CfgServer {
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

mod board_client {
    use wire_weaver_client::{ClientConfig, Commander, WwClient};

    pub struct BoardClient {
        pub cmd: Commander,
    }

    impl WwClient for BoardClient {
        fn default_config() -> ClientConfig {
            ClientConfig::default()
        }

        fn from_cmd(cmd: Commander) -> Self {
            Self { cmd }
        }
    }

    mod api_client {
        wire_weaver::ww_codegen!(
            traits_api :: Board for super::BoardClient,
            client = "std_client",
        );
    }
}

use board_client::BoardClient;

/// With the optional traits in, every level is served as usual.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn optional_traits_served_when_present() {
    let _ = tracing_subscriber::fmt::try_init();
    let device = tests_common::start_device(
        "traits/optional_traits_full",
        full_server::FullServer,
        traits_api::TRAITS_FULL_GID,
        full_server::api_impl::api_hash(),
    );
    let client = BoardClient::config(|c| c.in_process_path(device.path()))
        .connect()
        .await
        .expect("connect");
    assert_eq!(client.ping().call().await.unwrap(), 1);
    assert_eq!(client.diag().read(3).call().await.unwrap(), 6);
    client.diag().release_all().call().await.unwrap();
    client.diag().sub().m1().call().await.unwrap();
    assert_eq!(client.diag().read_POINTS().read().await.unwrap(), 64);
    client.probe(2).set_high().call().await.unwrap();
    assert_eq!(client.probe_count().call().await.unwrap(), 4);
}

/// A left-out trait fails locally, down to its nested levels, its consts and the items of its group.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn omitted_traits_fail_locally() {
    let _ = tracing_subscriber::fmt::try_init();
    let device = tests_common::start_device(
        "traits/optional_traits_lean",
        lean_server::LeanServer,
        traits_api::TRAITS_FULL_GID,
        lean_server::api_impl::api_hash(),
    );
    let client = BoardClient::config(|c| c.in_process_path(device.path()))
        .connect()
        .await
        .expect("connect");
    assert_eq!(client.ping().call().await.unwrap(), 1);
    assert_not_implemented(client.diag().read(3).call().await, "diag.read");
    assert_not_implemented(client.diag().sub().m1().call().await, "diag.sub.m1");
    assert_not_implemented(client.diag().read_POINTS().read().await, "diag.POINTS");
    assert_not_implemented(client.probe(1).set_high().call().await, "probe[].set_high");
    assert_not_implemented(client.probe_count().call().await, "probe_count");
}

/// `omit_if` on traits follows its condition, in dispatch and introspection.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn omit_if_on_traits_follows_cfg() {
    let _ = tracing_subscriber::fmt::try_init();
    let device = tests_common::start_device(
        "traits/optional_traits_cfg",
        cfg_server::CfgServer,
        traits_api::TRAITS_FULL_GID,
        cfg_server::api_impl::api_hash(),
    );
    let client = BoardClient::config(|c| c.in_process_path(device.path()))
        .connect()
        .await
        .expect("connect");
    assert_eq!(client.diag().read(3).call().await.unwrap(), 6);
    assert_eq!(client.diag().read_POINTS().read().await.unwrap(), 64);
    assert_not_implemented(client.probe(1).set_high().call().await, "probe[].set_high");
    assert_not_implemented(client.probe_count().call().await, "probe_count");
}

/// Called anyway (a client without the device's model), anything under a left-out trait answers `Unimplemented`
/// from the fallback arm of the level holding it.
#[test]
fn omitted_traits_answer_unimplemented() {
    // ids: root ping 0, diag 1, probe 2, probe_count 3; diag: read 0, POINTS 1, sub 2, release_all 3
    let mut server = lean_server::LeanServer;
    let mut call = |path: &[u32]| {
        call_raw(
            |request, scratch, out| {
                server
                    .process_request_bytes(request, scratch, out, ())
                    .unwrap()
                    .to_vec()
            },
            path,
            &[],
        )
    };
    assert_eq!(call(&[0]), None);
    assert_eq!(call(&[1, 3]).as_deref(), Some("Unimplemented@41"));
    assert_eq!(call(&[1, 2, 0]).as_deref(), Some("Unimplemented@41"));
    assert_eq!(call(&[2, 0, 0]).as_deref(), Some("Unimplemented@41"));
    assert_eq!(call(&[3]).as_deref(), Some("Unimplemented@41"));
    assert_eq!(call(&[4]).as_deref(), Some("BadPath@40"));

    let mut server = cfg_server::CfgServer;
    let mut call = |path: &[u32]| {
        call_raw(
            |request, scratch, out| {
                server
                    .process_request_bytes(request, scratch, out, ())
                    .unwrap()
                    .to_vec()
            },
            path,
            &[],
        )
    };
    assert_eq!(call(&[1, 3]), None);
    assert_eq!(call(&[2, 0, 0]).as_deref(), Some("Unimplemented@41"));
}

/// The device's model lists the left-out traits in the level holding them, the dynamic client (and with it Python)
/// fails everything below them locally.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dynamic_client_fails_omitted_traits_locally() {
    use wire_weaver_client::{ClientConfig, DynClient, DynResource};
    let _ = tracing_subscriber::fmt::try_init();
    let device = tests_common::start_device(
        "traits/optional_traits_dynamic",
        lean_server::LeanServer,
        traits_api::TRAITS_FULL_GID,
        lean_server::api_impl::api_hash(),
    );
    let client = DynClient::from_config(ClientConfig::new().in_process_path(device.path()))
        .connect()
        .await
        .expect("connect");
    let bundle = client.device_introspect().unwrap().api_bundle.clone();
    assert_eq!(
        bundle.root_omitted,
        Some(vec![UNib32(1), UNib32(2), UNib32(3)])
    );
    let root = DynResource::root(bundle);
    assert!(!root.child("ping").unwrap().is_omitted());
    let diag = root.child("diag").unwrap();
    assert!(diag.is_omitted());
    let m1 = diag.child("sub").unwrap().child("m1").unwrap();
    assert!(m1.is_omitted());
    assert_not_implemented(m1.call(client.cmd(), &[]).await, "diag.sub.m1");
}

#[test]
fn scaffold_leaves_out_omitted_traits() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../traits_api");
    let bundle = wire_weaver_core::load(&path, Some("Board".into()), false).unwrap();
    let config = wire_weaver_core::ServerScaffoldConfig {
        server_struct: "Server".into(),
        no_alloc: true,
        use_async: false,
        handler_model: String::new(),
        method_model: String::new(),
        property_model: String::new(),
        const_model: String::new(),
        medium: None,
        omit: vec![],
        observe_cells: vec![],
    };
    let code = wire_weaver_core::gen_server_scaffold(&bundle, &config).unwrap();
    for expected in [
        "// optional, all handlers of diag can be left out with omit = [\"diag\"]",
        "// optional, all handlers of probe[] can be left out with omit = [\"probes\"]",
        "fn diag_sub_m1(",
        "const DIAG_POINTS: u16",
    ] {
        assert!(code.contains(expected), "{expected}\n{code}");
    }

    let config = wire_weaver_core::ServerScaffoldConfig {
        omit: vec!["diag".into()],
        ..config
    };
    let code = wire_weaver_core::gen_server_scaffold(&bundle, &config).unwrap();
    for omitted in ["diag_", "DIAG_POINTS", "BoardConsts"] {
        assert!(!code.contains(omitted), "{omitted}\n{code}");
    }
    assert!(code.contains("fn probe_set_high("), "{code}");
}
