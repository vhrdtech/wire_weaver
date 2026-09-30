//! RTT [Transport]: probe-rs session on an IO thread + [ww_framer] in stream mode with the RTT framer
//! configuration from [ww_link], see [stream](crate::event_loop::stream).

use std::time::{Duration, Instant};

use anyhow::{Context, anyhow};
use probe_rs::probe::DebugProbeInfo;
use probe_rs::rtt::{Rtt, ScanRegion};
use probe_rs::{Core, Permissions, Session};
use tokio::sync::mpsc;
use tracing::{debug, trace, warn};
use ww_link::{RttChecksum, RttHead, RttTail};

use crate::config::RttControlBlock;
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
const SCAN_RETRY_WINDOW: Duration = Duration::from_secs(2);
/// Same for a known control block address (from an ELF file), checking it is quick. Shorter, as the likely reason
/// for a failure is a firmware not matching the ELF file, and RAM is scanned after that.
const EXACT_RETRY_WINDOW: Duration = Duration::from_millis(500);
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
    pub(crate) control_block: Option<RttControlBlock>,
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
        // before touching the probe, a wrong path fails right away
        let control_block = control_block_address(h.control_block)?;
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
        let mut rtt = match control_block {
            Some(address) => {
                match attach_rtt(&mut core, &ScanRegion::Exact(address), EXACT_RETRY_WINDOW) {
                    Ok(rtt) => rtt,
                    Err(e) => {
                        warn!(
                            "{e} at {address:#010x}, firmware doesn't match the ELF file? Scanning RAM instead"
                        );
                        attach_rtt(&mut core, &ScanRegion::Ram, SCAN_RETRY_WINDOW)?
                    }
                }
            }
            None => attach_rtt(&mut core, &ScanRegion::Ram, SCAN_RETRY_WINDOW)?,
        };
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

fn control_block_address(cb: Option<RttControlBlock>) -> anyhow::Result<Option<u64>> {
    let path = match cb {
        None => return Ok(None),
        Some(RttControlBlock::Address(address)) => return Ok(Some(address)),
        Some(RttControlBlock::Elf(path)) => path,
    };
    let elf = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    let address = probe_rs::rtt::find_rtt_control_block_in_raw_file(&elf)
        .with_context(|| format!("parsing {}", path.display()))?
        .ok_or_else(|| {
            anyhow!(
                "no _SEGGER_RTT symbol in {}, is RTT used there?",
                path.display()
            )
        })?;
    debug!(
        "RTT control block at {address:#010x} from {}",
        path.display()
    );
    Ok(Some(address))
}

/// Retried for `retry_window` in case the firmware has just started, see [SCAN_RETRY_WINDOW].
fn attach_rtt(core: &mut Core, region: &ScanRegion, retry_window: Duration) -> anyhow::Result<Rtt> {
    let started = Instant::now();
    let mut attempt = 1;
    loop {
        match Rtt::attach_region(core, region) {
            Ok(rtt) => return Ok(rtt),
            Err(e) => {
                // RTT is not in the firmware at all, no point in retrying
                let retry = !matches!(e, probe_rs::rtt::Error::NoControlBlockLocation)
                    && (attempt < MIN_ATTACH_ATTEMPTS || started.elapsed() < retry_window);
                if !retry {
                    debug!(
                        "RTT attach failed after {attempt} attempts, {:?}",
                        started.elapsed()
                    );
                    return Err(anyhow!("{}", first_line(&e)));
                }
                trace!(
                    "RTT attach attempt {attempt} failed after {:?}",
                    started.elapsed()
                );
                attempt += 1;
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

/// probe-rs appends several lines of hints to some errors, first one is the error itself
fn first_line(e: &impl std::fmt::Display) -> String {
    let e = e.to_string();
    e.lines()
        .next()
        .unwrap_or_default()
        .trim_end_matches('.')
        .to_string()
}

fn find<'a>(
    mut names: impl Iterator<Item = Option<&'a str>> + Clone,
    name: &str,
) -> anyhow::Result<usize> {
    names.clone().position(|n| n == Some(name)).ok_or_else(|| {
        let found: Vec<&str> = names.by_ref().map(|n| n.unwrap_or("<unnamed>")).collect();
        anyhow!(
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
