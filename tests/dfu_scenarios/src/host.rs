//! The host side: generated std clients of the three firmwares, connecting over the device's link, and the two ways to
//! send a plan.

use std::time::Duration;

use wire_weaver_client::{ClientConfig, Commander, WwClient};

use crate::device::Addr;

macro_rules! client {
    ($m:ident, $name:ident, $api:ident, $root:ident) => {
        pub mod $m {
            use super::*;

            pub struct $name {
                pub cmd: Commander,
            }

            impl WwClient for $name {
                fn default_config() -> ClientConfig {
                    // a power cut is noticed by a timeout on UDP
                    ClientConfig::default().default_timeout(Duration::from_secs(2))
                }

                fn from_cmd(cmd: Commander) -> Self {
                    Self { cmd }
                }
            }

            mod api_client {
                wire_weaver::ww_codegen!(
                    $api :: $root for super::$name,
                    client = "std_client",
                );
            }
        }
    };
}

client!(app, AppClient, dfu_app_api, DfuApp);
client!(lean_app, LeanAppClient, dfu_lean_app_api, DfuLeanApp);
client!(updater, UpdaterClient, dfu_updater_api, DfuUpdater);

/// Client config that selects the device at `addr`.
pub fn at(addr: &Addr) -> impl Fn(ClientConfig) -> ClientConfig + use<> {
    let addr = addr.clone();
    move |c| match &addr {
        Addr::InProcess(path) => c.in_process_path(path.clone()),
        Addr::WebSocket(url) => c.websocket_url(url.clone()),
        Addr::Udp(a) => c.udp_addr(a.clone()),
    }
}

/// Connect a generated client to the device at `addr`: `connect!(AppClient, &dev.addr)`.
#[macro_export]
macro_rules! connect {
    ($client:ty, $addr:expr) => {
        <$client>::config($crate::host::at($addr))
            .connect()
            .await
            .expect("connect")
    };
}

/// Send `plan`'s steps from `from` on, one call each, chunks in pieces of `max_piece`. A macro: the `Stage` clients
/// of the app and the updater are different generated types.
#[macro_export]
macro_rules! send_steps {
    ($stage:expr, $plan:expr, $from:expr, $max_piece:expr) => {{
        for (index, step) in $plan.steps.iter().enumerate().skip($from as usize) {
            let index = index as u32;
            match step {
                $crate::plan::Step::Erase { block } => {
                    $stage.erase(index, *block).call().await.unwrap()
                }
                $crate::plan::Step::Copy { dst, src, hash } => {
                    $stage.copy(index, *dst, *src, *hash).call().await.unwrap()
                }
                $crate::plan::Step::Write { dst, hash, data } => {
                    for (i, piece) in data.chunks($max_piece as usize).enumerate() {
                        let offset = (i * $max_piece as usize) as u32;
                        $stage
                            .write(
                                index,
                                *dst,
                                *hash,
                                offset,
                                MaxLen::new(piece.to_vec()).unwrap(),
                            )
                            .call()
                            .await
                            .unwrap()
                    }
                }
            }
        }
    }};
}

/// Stream plan bytes `$from..$to` of `$plan`'s body on the `plan` credit sink of `$bulk`, in segments of
/// `$max_piece`, and wait until the device has them all (`flush`).
#[macro_export]
macro_rules! send_bulk {
    ($bulk:expr, $plan:expr, $from:expr, $to:expr, $max_piece:expr) => {{
        let mut sink = $bulk.plan().await.unwrap();
        let (from, to) = ($from as usize, $to as usize);
        // named by the host, a fixed one keeps the transcripts the same from run to run
        sink.open_session(std::num::NonZeroU32::new(from as u32 + 1).unwrap())
            .await
            .unwrap();
        let mut offset = from;
        while offset < to {
            let end = (offset + $max_piece as usize).min(to);
            sink.send_owned(ww_dfu::PlanSegmentOwned {
                offset: offset as u32,
                data: MaxLen::new($plan.body[offset..end].to_vec()).unwrap(),
            })
            .await
            .unwrap();
            offset = end;
        }
        sink.flush().await.unwrap();
        sink
    }};
}

/// Continue the `plan` credit sink session of `$cursor` (saved by a host before it restarted) on `$bulk` with the
/// generated `plan_resume`, then stream plan bytes `$from..$to` (from where the node has them, its `resume_from`) in
/// segments of `$max_piece` and wait until the device has them all. Returns the sink and the position the device
/// answered (segments it has in the session).
#[macro_export]
macro_rules! resume_bulk {
    ($bulk:expr, $cursor:expr, $plan:expr, $from:expr, $to:expr, $max_piece:expr) => {{
        let (mut sink, at) = $bulk.plan_resume($cursor).await.unwrap();
        let (from, to) = ($from as usize, $to as usize);
        let mut offset = from;
        while offset < to {
            let end = (offset + $max_piece as usize).min(to);
            sink.send_owned(ww_dfu::PlanSegmentOwned {
                offset: offset as u32,
                data: MaxLen::new($plan.body[offset..end].to_vec()).unwrap(),
            })
            .await
            .unwrap();
            offset = end;
        }
        sink.flush().await.unwrap();
        (sink, at)
    }};
}
