//! RTT [Transport]: probe-rs session on an IO thread + [ww_framer] in stream mode with the RTT framer
//! configuration from [ww_link], see [stream](crate::event_loop::stream).

use std::time::{Duration, Instant};

use probe_rs::probe::DebugProbeInfo;
use probe_rs::rtt::{Rtt, ScanRegion};
use probe_rs::{Core, Permissions, Session};
use tokio::sync::mpsc;
use tracing::{debug, trace};
use ww_link::{RttChecksum, RttHead, RttTail};

use crate::event_loop::DeviceHandle;
use crate::event_loop::command::Command;
use crate::event_loop::stream::{self, BlockingStreamIo, StreamConfig};
use crate::event_loop::transport::{Opened, Transport};

/// Channel names the device creates with `rtt_init!`, see `ww_device::rtt`
const UP_CHANNEL: &str = "ww_up";
const DOWN_CHANNEL: &str = "ww_down";
/// Firmware might be just starting and not have initialized RTT yet: scanning RAM for the control block is
/// retried for this long. A scan in progress is never cut off (it takes ~1.5 s for 144 KiB through an ST-LINK),
/// so at least [MIN_ATTACH_ATTEMPTS] are made even if one scan alone takes longer than that.
const ATTACH_RETRY_WINDOW: Duration = Duration::from_secs(2);
const MIN_ATTACH_ATTEMPTS: u32 = 2;
const CONFIG: StreamConfig = StreamConfig {
    name: "ww_rtt",
    poll_interval: Duration::from_millis(1),
    read_chunk: 4096,
};

type RttTx = stream::StreamTx<RttHead, RttChecksum, RttTail>;
type RttRx = stream::StreamRx<RttHead, RttChecksum, RttTail>;

pub(crate) struct RttHandle {
    pub(crate) probe: DebugProbeInfo,
    /// probe-rs chip name
    pub(crate) target: String,
    pub(crate) speed_hz: Option<u32>,
}

pub async fn rtt_worker(cmd_rx: mpsc::Receiver<Command>) {
    debug!("rtt worker started");
    crate::event_loop::core::worker(cmd_rx, RttTransport).await;
    debug!("rtt worker exited");
}

struct RttTransport;

impl Transport for RttTransport {
    type Tx = RttTx;
    type Rx = RttRx;

    /// Attaching is slow (RAM is scanned for the RTT control block) and blocking, so it happens on the IO thread.
    async fn connect(&mut self, handle: DeviceHandle) -> Result<Opened<RttTx, RttRx>, String> {
        let handle = handle
            .downcast::<RttHandle>()
            .map_err(|_| "expected RttHandle".to_string())?;
        stream::open(CONFIG, move || {
            ProbeRtt::attach(*handle).map_err(|e| format!("RTT: {e:#}"))
        })
        .await
    }
}

struct ProbeRtt {
    session: Session,
    rtt: Rtt,
    up: usize,
    down: usize,
}

impl ProbeRtt {
    fn attach(h: RttHandle) -> anyhow::Result<Self> {
        debug!("attaching to {} through {}", h.target, h.probe);
        let mut probe = h.probe.open()?;
        if let Some(speed_hz) = h.speed_hz {
            let khz = probe.set_speed(speed_hz / 1000)?;
            debug!("probe speed set to {khz} kHz");
        }
        // does not reset the core, a running firmware is attached to as is (probe-rs halts it for a few ms
        // to clear hardware breakpoints, on attach and on detach)
        let mut session = probe.attach(h.target.as_str(), Permissions::default())?;
        let mut core = session.core(0)?;
        let mut rtt = attach_rtt(&mut core)?;
        let up = find(rtt.up_channels().iter().map(|c| c.name()), UP_CHANNEL)?;
        let down = find(rtt.down_channels().iter().map(|c| c.name()), DOWN_CHANNEL)?;
        // A previous host could have stopped reading in the middle of a message and there is nothing to
        // re-synchronize on in a stream, start clean. The device does not send anything until link setup.
        let mut buf = [0u8; 1024];
        loop {
            let n = rtt.up_channels()[up].read(&mut core, &mut buf)?;
            if n == 0 {
                break;
            }
            trace!("dropped {n} stale bytes");
        }
        drop(core);
        Ok(ProbeRtt {
            session,
            rtt,
            up,
            down,
        })
    }
}

fn attach_rtt(core: &mut Core) -> Result<Rtt, probe_rs::rtt::Error> {
    let started = Instant::now();
    let mut attempt = 1;
    loop {
        match Rtt::attach_region(core, &ScanRegion::Ram) {
            // RTT is not in the firmware at all, no point in retrying
            Err(e @ probe_rs::rtt::Error::NoControlBlockLocation) => return Err(e),
            Err(e) if attempt < MIN_ATTACH_ATTEMPTS || started.elapsed() < ATTACH_RETRY_WINDOW => {
                debug!(
                    "RTT attach attempt {attempt} failed after {:?}: {e}",
                    started.elapsed()
                );
                attempt += 1;
                std::thread::sleep(Duration::from_millis(50));
            }
            other => return other,
        }
    }
}

fn find<'a>(
    mut names: impl Iterator<Item = Option<&'a str>> + Clone,
    name: &str,
) -> anyhow::Result<usize> {
    names.clone().position(|n| n == Some(name)).ok_or_else(|| {
        let found: Vec<&str> = names.by_ref().map(|n| n.unwrap_or("<unnamed>")).collect();
        anyhow::anyhow!(
            "no channel named '{name}' (found: {found:?}), see ww_device::rtt on how to create it"
        )
    })
}

impl BlockingStreamIo for ProbeRtt {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, String> {
        let mut core = self.session.core(0).map_err(|e| e.to_string())?;
        self.rtt.up_channels()[self.up]
            .read(&mut core, buf)
            .map_err(|e| e.to_string())
    }

    fn write(&mut self, bytes: &[u8]) -> Result<usize, String> {
        let mut core = self.session.core(0).map_err(|e| e.to_string())?;
        self.rtt.down_channels()[self.down]
            .write(&mut core, bytes)
            .map_err(|e| e.to_string())
    }
}
