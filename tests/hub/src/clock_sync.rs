//! Clock sync in the hub (SRV-31): a device serving `ww_timebase::Clock` with host sync is steered by the hub as soon
//! as the hub opens it, stays open for the sync linger after its clients left, and programs see the steering through
//! `ClockSync` instead of steering themselves.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tests_common::TestDevice;
use wire_weaver::prelude::*;
use wire_weaver_client::{ClientConfig, Commander, WwClient};
use ww_hub::client::HubClient;
use ww_hub::{Bound, Exit, HubConfig, Shutdown};
use ww_hub_api::{DeviceStateOwned, SyncEntryOwned, SyncPhaseOwned};
use ww_si::ww_numeric::NumericValue;
use ww_si::{Prefix, Second};
use ww_timebase::{
    ClockError, ClockInfo, SteerPoint, Steerer, SteererOwned, SyncConfig, SyncSource, SyncState,
    SyncStatus,
};

/// What the device's clock took.
#[derive(Default)]
struct Clock {
    /// The last steer point followed
    point: Option<SteerPoint<'static>>,
    steers: u32,
}

mod server {
    use super::*;
    use tests_common::TestProcessEvents;

    /// A clock in µs from a real [Instant], followed steer points kept as they came.
    pub struct Server {
        pub clock: Arc<Mutex<Clock>>,
        pub epoch: Instant,
        /// Whose points it follows
        pub steerer: Option<SteererOwned>,
    }

    fn micros(us: u32) -> Second {
        Second::from_parts(Prefix::Micro, NumericValue::U32(us))
    }

    fn as_micros(s: &Second) -> u32 {
        match s.value {
            NumericValue::U32(v) => v,
            _ => 0,
        }
    }

    impl Server {
        fn now_us(&self) -> u32 {
            self.epoch.elapsed().as_micros() as u32
        }

        fn add(
            &mut self,
            _cx: &mut Context<'_, impl BlockingEventOut>,
            a: u32,
            b: u32,
        ) -> RpcResult<u32> {
            Ready(a + b)
        }

        fn clock_now(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<Second> {
            Ready(micros(self.now_us()))
        }

        fn clock_info(
            &mut self,
            _cx: &mut Context<'_, impl BlockingEventOut>,
        ) -> RpcResult<ClockInfo<'_>> {
            Unimplemented.into()
        }

        fn clock_time_map(
            &mut self,
            _cx: &mut Context<'_, impl BlockingEventOut>,
        ) -> RpcResult<()> {
            Unimplemented.into()
        }

        fn close_clock_time_map(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {}

        fn sideband_clock_time_map(
            &mut self,
            _cx: &mut Context<'_, impl BlockingEventOut>,
            _sideband: ww_client_server::StreamSideband,
        ) -> Option<ww_client_server::StreamSideband> {
            None
        }

        fn clock_configure_sync(
            &mut self,
            _cx: &mut Context<'_, impl BlockingEventOut>,
            _config: SyncConfig,
        ) -> RpcResult<Result<(), ClockError<'_>>> {
            Unimplemented.into()
        }

        fn clock_sync_config(
            &mut self,
            _cx: &mut Context<'_, impl BlockingEventOut>,
        ) -> RpcResult<SyncConfig> {
            Ready(SyncConfig::Off)
        }

        fn clock_sync_status(
            &mut self,
            _cx: &mut Context<'_, impl BlockingEventOut>,
        ) -> RpcResult<SyncStatus<'_>> {
            let locked = self.clock.lock().unwrap().point.is_some();
            Ready(SyncStatus {
                state: if locked {
                    SyncState::Locked
                } else {
                    SyncState::Free
                },
                offset: None,
                last_lock: None,
                source: locked.then_some(SyncSource::Host),
                error: None,
                slew_us: None,
                steerer: self.steerer.as_ref().map(|s| Steerer {
                    id: s.id,
                    host: &s.host,
                    program: &s.program,
                    pid: s.pid,
                }),
                noted: None,
            })
        }

        fn clock_set_time_of_day(
            &mut self,
            _cx: &mut Context<'_, impl BlockingEventOut>,
            _t: ww_date_time::DateTime,
        ) -> RpcResult<Result<(), ClockError<'_>>> {
            Unimplemented.into()
        }

        fn clock_steer(
            &mut self,
            _cx: &mut Context<'_, impl BlockingEventOut>,
            point: SteerPoint<'_>,
        ) -> RpcResult<Result<Option<SyncSource>, ClockError<'_>>> {
            self.steerer = point.steerer.as_ref().map(|s| SteererOwned {
                id: s.id,
                host: s.host.to_string(),
                program: s.program.to_string(),
                pid: s.pid,
            });
            let mut clock = self.clock.lock().unwrap();
            clock.steers += 1;
            clock.point = Some(SteerPoint {
                steerer: None,
                ..point
            });
            Ready(Ok(Some(SyncSource::Host)))
        }

        fn clock_shared_now(
            &mut self,
            _cx: &mut Context<'_, impl BlockingEventOut>,
        ) -> RpcResult<Option<u64>> {
            let Some(p) = self.clock.lock().unwrap().point.clone() else {
                return Ready(None);
            };
            let dl = self.now_us().wrapping_sub(as_micros(&p.local)) as f64;
            Ready(Some(
                (p.shared_us as f64 + dl * (1.0 + p.rate_ppb as f64 * 1e-9)) as u64,
            ))
        }

        fn clock_set_sources(
            &mut self,
            _cx: &mut Context<'_, impl BlockingEventOut>,
            _sources: Option<MaxLen<RefVec<'_, SyncSource>, 8>>,
        ) -> RpcResult<Result<(), ClockError<'_>>> {
            Unimplemented.into()
        }

        fn clock_sources(
            &mut self,
            _cx: &mut Context<'_, impl BlockingEventOut>,
        ) -> RpcResult<Option<MaxLen<RefVec<'_, SyncSource>, 8>>> {
            Ready(None)
        }
    }

    pub mod api_impl {
        wire_weaver::ww_codegen!(
            hub_api :: ClockedTest for super::Server,
            server = true, no_alloc = true, use_async = false,
            method_model = "_=immediate",
            property_model = "_=get_set",
            introspect = "no_docs",
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

pub struct Client {
    pub cmd: Commander,
}

impl WwClient for Client {
    fn default_config() -> ClientConfig {
        ClientConfig::default()
    }

    fn from_cmd(cmd: Commander) -> Self {
        Client { cmd }
    }
}

mod api_client {
    wire_weaver::ww_codegen!(hub_api :: ClockedTest for super::Client, client = "std_client");
}

struct Bench {
    _device: TestDevice,
    clock: Arc<Mutex<Clock>>,
    dir: PathBuf,
    key: String,
    hub: tokio::task::JoinHandle<Exit>,
    shutdown: Shutdown,
}

impl Bench {
    fn socket(&self) -> String {
        self.dir
            .join(ww_local_socket::files::HUB_SOCKET)
            .display()
            .to_string()
    }

    async fn client(&self) -> Client {
        let (socket, key) = (self.socket(), self.key.clone());
        Client::config(|c| c.hub_device_at(socket.clone(), key.clone()))
            .connect()
            .await
            .expect("connect through the hub")
    }

    async fn hub(&self) -> HubClient {
        HubClient::config(|c| c.hub_api_at(self.socket()))
            .connect()
            .await
            .unwrap()
    }

    async fn stop(self) {
        self.shutdown.shutdown();
        assert_eq!(self.hub.await.unwrap(), Exit::Shutdown);
    }
}

fn start(name: &str, linger: Duration, sync_linger: Option<Duration>) -> Bench {
    let _ = tracing_subscriber::fmt::try_init();
    let clock = Arc::new(Mutex::new(Clock::default()));
    let path = format!("hub_sync/{name}");
    let device = tests_common::start_device(
        &path,
        server::Server {
            clock: clock.clone(),
            epoch: Instant::now(),
            steerer: None,
        },
        hub_api::HUB_TEST_FULL_GID,
        server::api_impl::api_hash(),
    );
    let dir = std::env::temp_dir().join(format!("ww-hub-sync-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bound = Bound::bind(HubConfig {
        runtime_dir: dir.clone(),
        idle_exit: Duration::from_secs(60),
        linger,
        version: ww_hub::HUB_VERSION.into(),
        sync: true,
        sync_linger,
        // fast, for the test: in-process round trips are tens of µs
        sync_options: wire_weaver_client::clock_sync::Options {
            poll: Duration::from_millis(5),
            steer_every: Duration::from_millis(20),
            check_every: Some(Duration::from_millis(20)),
            acquire: wire_weaver_client::clock_sync::Pace {
                poll: Duration::from_millis(5),
                steer_every: Duration::from_millis(20),
                check_every: Some(Duration::from_millis(20)),
            },
            estimator: wire_weaver_client::clock_sync::EstimatorConfig {
                min_span: Duration::from_millis(50),
                ..Default::default()
            },
            ..Default::default()
        },
        state_dir: None,
        adopt_attach: std::time::Duration::from_secs(5),
    })
    .unwrap()
    .expect("no other hub in this dir");
    let shutdown = bound.shutdown_handle();
    Bench {
        _device: device,
        clock,
        dir,
        key: format!("inproc:{path}"),
        hub: tokio::spawn(bound.run()),
        shutdown,
    }
}

/// The device's entry in `ClockSync::devices`, once `f` holds for it (within 3 s).
async fn sync_entry(
    hub: &HubClient,
    key: &str,
    what: &str,
    f: impl Fn(&SyncEntryOwned) -> bool,
) -> SyncEntryOwned {
    for _ in 0..300 {
        let entries = hub.sync().devices().call().await.unwrap();
        if let Some(e) = entries.into_iter().find(|e| e.key == key)
            && f(&e)
        {
            return e;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("{what}: not within 3 s");
}

async fn is_open(hub: &HubClient, key: &str) -> bool {
    hub.registry()
        .devices()
        .call()
        .await
        .unwrap()
        .into_iter()
        .any(|d| d.key == key && d.state == DeviceStateOwned::Open)
}

/// The hub steers a device it opened for a client, says so, and keeps it open after the client left.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_hub_steers_and_keeps_the_device_open() {
    let bench = start(
        "steers",
        Duration::from_millis(100),
        Some(Duration::from_secs(60)),
    );
    let hub = bench.hub().await;
    let mut stats = hub.sync().stats().call().await.unwrap();
    let client = bench.client().await;
    assert_eq!(client.add(1, 2).call().await.unwrap(), 3);

    let e = sync_entry(&hub, &bench.key, "steered and checked", |e| {
        e.phase == SyncPhaseOwned::Steering && e.steers > 0 && e.followed.is_some()
    })
    .await;
    assert_eq!(e.followed, Some(true));
    let steerer = e.steerer.expect("the device names its steerer");
    assert_eq!(steerer.program, "ww hub");
    assert_eq!(steerer.pid, std::process::id());
    assert_eq!(e.state, Some(SyncState::Locked));
    // the client's own steering (CLT-46) leaves it to the hub
    let own = client.cmd.clock_sync().expect("on by default");
    assert_eq!(
        own.phase(),
        wire_weaver_client::clock_sync::Phase::ThroughHub
    );
    assert_eq!(own.status().steers, 0);
    assert!(bench.clock.lock().unwrap().steers > 0);
    // the stream has them too
    let streamed = tokio::time::timeout(Duration::from_secs(2), stats.recv())
        .await
        .expect("stats within 2 s")
        .unwrap();
    assert_eq!(streamed.key, bench.key);

    // the client leaves: the device stays open (and steered) well past the plain linger
    client.cmd.disconnect().asynch().await.unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert!(
        is_open(&hub, &bench.key).await,
        "a steered device lingers for the sync linger"
    );
    let steers = bench.clock.lock().unwrap().steers;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        bench.clock.lock().unwrap().steers > steers,
        "still steered without clients"
    );

    // linger 0: only while clients are connected, so the plain linger closes it
    hub.sync().write_linger(Some(0)).write().await.unwrap();
    assert_eq!(hub.sync().read_linger().read().await.unwrap(), Some(0));
    for _ in 0..200 {
        if !is_open(&hub, &bench.key).await {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        !is_open(&hub, &bench.key).await,
        "closed after the plain linger"
    );
    assert!(hub.sync().devices().call().await.unwrap().is_empty());
    drop(stats);
    drop(hub);
    bench.stop().await;
}

/// Steering switched off for a device: it coasts, and lingers no longer than any device.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn steering_switched_off_and_on() {
    let bench = start("off", Duration::from_millis(100), None);
    let hub = bench.hub().await;
    let client = bench.client().await;
    assert_eq!(client.add(1, 2).call().await.unwrap(), 3);
    sync_entry(&hub, &bench.key, "steered", |e| e.steers > 0).await;

    hub.sync()
        .set_enabled(Some(bench.key.clone()), false)
        .call()
        .await
        .unwrap();
    sync_entry(&hub, &bench.key, "off", |e| e.phase == SyncPhaseOwned::Off).await;
    let steers = bench.clock.lock().unwrap().steers;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        bench.clock.lock().unwrap().steers,
        steers,
        "no steer points while off"
    );

    // on for every device again
    hub.sync().set_enabled(None, true).call().await.unwrap();
    sync_entry(&hub, &bench.key, "steered again", |e| {
        e.phase == SyncPhaseOwned::Steering
    })
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(bench.clock.lock().unwrap().steers > steers);

    // off again, the client leaves: closed after the plain linger although the sync linger is forever
    hub.sync().set_enabled(None, false).call().await.unwrap();
    sync_entry(&hub, &bench.key, "off", |e| e.phase == SyncPhaseOwned::Off).await;
    client.cmd.disconnect().asynch().await.unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        !is_open(&hub, &bench.key).await,
        "closed after the plain linger"
    );
    drop(hub);
    bench.stop().await;
}

/// A program holding the whole device (a root claim, SRV-32) pauses the hub's steering: the device is still read,
/// no steer point reaches it until the claim goes.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_root_claim_pauses_steering() {
    let bench = start("paused", Duration::from_millis(100), None);
    let hub = bench.hub().await;
    let client = bench.client().await;
    assert_eq!(client.add(1, 2).call().await.unwrap(), 3);
    sync_entry(&hub, &bench.key, "steered", |e| e.steers > 0).await;

    let claim = hub
        .claims()
        .claim(bench.key.clone(), vec![], "firmware update".into(), None)
        .call()
        .await
        .expect("root claimed");
    let e = sync_entry(&hub, &bench.key, "paused", |e| {
        e.phase == SyncPhaseOwned::Paused
    })
    .await;
    let steers = bench.clock.lock().unwrap().steers;
    // still read while paused
    sync_entry(&hub, &bench.key, "read on", |p| p.samples > e.samples).await;
    assert_eq!(
        bench.clock.lock().unwrap().steers,
        steers,
        "no steer points while the root is claimed"
    );

    hub.claims().release(claim).call().await.unwrap();
    sync_entry(&hub, &bench.key, "steered again", |e| {
        e.phase == SyncPhaseOwned::Steering
    })
    .await;
    for _ in 0..300 {
        if bench.clock.lock().unwrap().steers > steers {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        bench.clock.lock().unwrap().steers > steers,
        "steered after the release"
    );
    drop(client);
    drop(hub);
    bench.stop().await;
}
