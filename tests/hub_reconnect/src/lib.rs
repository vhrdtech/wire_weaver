//! Clients of a device behind the ww hub when the hub goes (CLT-40) or hands over to a newer one (SRV-30). The device
//! is served in the tests' own process: as `inproc:` for a hub in that process too (shut down and bound again), on a
//! local socket (`ipc:`) for `hub_dev`, a hub in another process that is killed and started again by the client.

use std::sync::{Arc, Mutex};
use tests_common::locked::Locked;

use tests_common::TestDevice;
use wire_weaver::prelude::*;
use ww_stream::tx::Counted;

/// What the device saw.
#[derive(Default, Debug)]
pub struct Seen {
    /// Every `add` call's arguments, in order: a call sent again would show up twice
    pub adds: Vec<(u32, u32)>,
}

pub mod server {
    use super::*;
    use tests_common::TestProcessEventsAsync;

    pub struct Server {
        pub seen: Arc<Mutex<Seen>>,
        pub ticks_stream: Counted,
        pub next_tick: u32,
        pub stream_sessions: u32,
    }

    impl Server {
        async fn add(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            a: u32,
            b: u32,
        ) -> RpcResult<u32> {
            self.seen.locked().adds.push((a, b));
            RpcResult::Ready(a.wrapping_add(b))
        }

        async fn ticks(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
            RpcResult::Ready(())
        }

        async fn close_ticks(&mut self, _cx: &mut Context<'_, impl EventOut>) {}

        async fn stream_ticks(&mut self) -> &mut Counted {
            &mut self.ticks_stream
        }

        async fn emit(&mut self, cx: &mut Context<'_, impl EventOut>, count: u32) -> RpcResult<()> {
            let ser = api_impl::stream_data_ser();
            for _ in 0..count {
                let tick = self.next_tick;
                self.next_tick += 1;
                ser.ticks_send(&mut self.ticks_stream, &tick, 1, None, cx)
                    .await
                    .unwrap();
            }
            RpcResult::Ready(())
        }

        fn new_stream_session(&mut self) -> u32 {
            self.stream_sessions = self.stream_sessions.wrapping_add(1);
            self.stream_sessions
        }
    }

    pub mod api_impl {
        wire_weaver::ww_codegen!(
            hub_reconnect_api :: HubReconnect for super::Server,
            server = true, no_alloc = true, use_async = true,
            method_model = "_=immediate",
            property_model = "_=get_set",
        );
    }

    impl TestProcessEventsAsync for Server {
        type Medium = ();

        async fn process_request_bytes<'a>(
            &mut self,
            bytes: &[u8],
            scratch: &'a mut [u8],
            out: &mut impl EventOut,
            medium: (),
        ) -> Result<&'a [u8], ShrinkWrapError> {
            self.process_request_bytes(bytes, scratch, out, medium)
                .await
        }
    }
}

pub mod client {
    use wire_weaver_client::{ClientConfig, Commander, WwClient};

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
            hub_reconnect_api :: HubReconnect for super::Client,
            client = "std_client",
        );
    }
}

fn new_server(seen: &Arc<Mutex<Seen>>) -> server::Server {
    server::Server {
        seen: seen.clone(),
        ticks_stream: Counted::new(),
        next_tick: 0,
        stream_sessions: 0,
    }
}

/// The device, served in this process at `inproc:<path>`: a hub in this process reaches it.
pub fn start_device(path: &str) -> (TestDevice, Arc<Mutex<Seen>>) {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let device = tests_common::start_device_async(
        path,
        new_server(&seen),
        hub_reconnect_api::HUB_RECONNECT_FULL_GID,
        hub_reconnect_api::HUB_RECONNECT_WIRE_VERSION,
        tests_common::api!(server::api_impl),
    );
    (device, seen)
}

/// The device, served by this process on a local socket: a hub in another process reaches it as `ipc:<socket>`,
/// and it outlives that hub, as a real device does.
pub fn start_device_ipc(socket: &std::path::Path) -> (TestDevice, Arc<Mutex<Seen>>) {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let device = tests_common::start_device_async_ipc(
        socket,
        new_server(&seen),
        hub_reconnect_api::HUB_RECONNECT_FULL_GID,
        hub_reconnect_api::HUB_RECONNECT_WIRE_VERSION,
        tests_common::api!(server::api_impl),
    );
    (device, seen)
}

/// The device as a simulated USB device on a local socket (64 B packets, frame counter): programs in other processes
/// reach it as `usbsim:<socket>`, one holding it at a time, and it outlives them (a live link handover, CLT-44).
pub fn start_device_usb_sim(socket: &std::path::Path) -> (TestDevice, Arc<Mutex<Seen>>) {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let device = tests_common::start_device_async_usb_sim(
        socket,
        64,
        new_server(&seen),
        hub_reconnect_api::HUB_RECONNECT_FULL_GID,
        hub_reconnect_api::HUB_RECONNECT_WIRE_VERSION,
        tests_common::api!(server::api_impl),
    );
    (device, seen)
}

/// Set in the child process [`in_child_with_env`] starts: the test's runtime dir.
const CHILD_DIR_ENV: &str = "HUB_RECONNECT_CHILD_DIR";

/// The file the child process writes beside its runtime dir (which the test may remove) when the test it was started
/// for runs, so a misspelt test name fails.
fn child_ran_file(dir: &std::path::Path) -> std::path::PathBuf {
    dir.with_extension("ran")
}

/// Runs test `test` of this test binary in a child process whose environment has `vars` and
/// [`RUNTIME_DIR_ENV`](ww_local_socket::RUNTIME_DIR_ENV) set, so no test sets a variable of its own process
/// (`std::env::set_var` is unsafe code, which the workspace forbids).
///
/// In the test's own process it makes the fresh runtime dir `<temp>/<prefix>-<pid>`, runs the child and returns
/// `None` once the child passed: the test then returns. In the child it returns the runtime dir.
///
/// # Panics
///
/// In the test's own process, when the child fails, cannot start, or never reaches this call (`test` is not the
/// calling test's name).
pub fn in_child_with_env(
    test: &str,
    prefix: &str,
    vars: &[(&str, &str)],
) -> Option<std::path::PathBuf> {
    if let Some(dir) = std::env::var_os(CHILD_DIR_ENV) {
        let dir = std::path::PathBuf::from(dir);
        std::fs::write(child_ran_file(&dir), test)
            .expect("marking the child's run in its runtime dir");
        return Some(dir);
    }
    let dir = std::env::temp_dir().join(format!("{prefix}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_file(child_ran_file(&dir));
    std::fs::create_dir_all(&dir).expect("creating the test's runtime dir");
    let status =
        std::process::Command::new(std::env::current_exe().expect("the path of the test binary"))
            .args([test, "--exact", "--nocapture"])
            .env(CHILD_DIR_ENV, &dir)
            .env(ww_local_socket::RUNTIME_DIR_ENV, &dir)
            .envs(vars.iter().copied())
            .status()
            .expect("starting the test binary again");
    assert!(
        status.success(),
        "{test} failed in its child process: {status}"
    );
    let ran = std::fs::remove_file(child_ran_file(&dir)).is_ok();
    assert!(ran, "the child process ran no test named {test}");
    None
}
