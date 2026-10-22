//! RTT [Transport]: probe-rs session on an IO thread + [ww_framer] in stream mode with the RTT framer
//! configuration from [ww_link], see [stream](crate::event_loop::stream).

use std::time::{Duration, Instant};

use anyhow::{Context, anyhow, bail};
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
use crate::event_loop::transport::{HeldRx, HeldTx, Opened, SharedClaim, Transport};

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
    /// Written into the holder's lock file (CLT-38, CLT-41)
    pub(crate) meta: Option<Box<crate::client_meta::ClientMetaOwned>>,
    /// [ClientConfig::holder_mode](crate::ClientConfig::holder_mode)
    pub(crate) holder_mode: crate::holders::Mode,
    /// How the holder answers a polite handover to the hub (CLT-43), see `ValidatedConfig::handover_offer`
    pub(crate) handover: Option<bool>,
}

pub async fn rtt_worker(cmd_rx: mpsc::Receiver<Command>) {
    debug!("rtt worker started");
    crate::event_loop::core::worker(cmd_rx, RttTransport).await;
    debug!("rtt worker exited");
}

struct RttTransport;

impl Transport for RttTransport {
    fn link_setup_timeout(&self) -> std::time::Duration {
        crate::event_loop::transport::link_setup_timeout::RTT
    }

    type Tx = HeldTx<RttTx>;
    type Rx = HeldRx<RttRx>;

    /// Attaching is slow (RAM is scanned for the RTT control block) and blocking, so it happens on the IO thread.
    async fn connect(
        &mut self,
        handle: DeviceHandle,
        tap: ww_tap::RecorderLink,
    ) -> Result<Opened<Self::Tx, Self::Rx>, String> {
        let handle = handle
            .downcast::<RttHandle>()
            .map_err(|_| "expected RttHandle".to_string())?;
        // Claim our own lock before opening the probe (CLT-38): a busy error from another WireWeaver holder then
        // names who, instead of probe-rs's generic one. Taken here, on the event loop's runtime, so that the claim
        // can listen for a handover to the hub (CLT-43); both halves keep it, the probe goes with the IO thread.
        let claim = claim(&handle).map_err(|e| format!("RTT: {e:#}"))?;
        let mut tap = tap;
        let peer = format!("{} via {}", handle.target, handle.probe.identifier);
        ww_tap::Tap::session(
            &mut tap,
            &crate::tap::host_session(
                ww_tap::Medium::Rtt,
                crate::tap::STREAM_PLAIN,
                crate::DEFAULT_MAX_MESSAGE_SIZE,
                &peer,
            ),
        );
        let opened: stream::StreamOpened<RttHead, RttChecksum, RttTail> =
            stream::open(CONFIG, tap, move || {
                ProbeRtt::attach(*handle).map_err(|e| format!("RTT: {e:#}"))
            })
            .await?;
        Ok(Opened {
            tx: HeldTx {
                inner: opened.tx,
                claim: claim.clone(),
            },
            rx: HeldRx {
                inner: opened.rx,
                claim,
            },
        })
    }
}

fn claim(h: &RttHandle) -> anyhow::Result<SharedClaim> {
    let key = crate::holders::key_for(crate::holders::DeviceRef::Rtt {
        probe_serial: h.probe.serial_number.as_deref().unwrap_or_default(),
        chip: &h.target,
    });
    match crate::holders::claim_offering(&key, h.holder_mode, h.meta.as_deref(), h.handover) {
        Ok(claim) => Ok(Some(std::sync::Arc::new(claim))),
        Err(crate::holders::ClaimError::HeldBy(holder)) => bail!(
            "probe is already open by another WireWeaver program or connection; {holder}{}",
            crate::hub::busy_hint(&holder)
        ),
        Err(crate::holders::ClaimError::Io(e)) => {
            warn!("could not record holding {key} ({e:#}), continuing without it");
            Ok(None)
        }
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

        let mut probe = h.probe.open().map_err(|e| with_claim_hint(e, &h.probe))?;
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

/// Same idea as `with_hint` in `usb/connect.rs`: a busy error already passed our own lock (CLT-38: nothing else
/// speaking WireWeaver holds this probe), so the holder is a program that never wrote a lock file (probe-rs's
/// own CLI, OpenOCD). On Linux, named by scanning `/proc` for whoever has the probe's USB device node open;
/// elsewhere, just the plain error.
fn with_claim_hint(e: probe_rs::probe::DebugProbeError, info: &DebugProbeInfo) -> anyhow::Error {
    use probe_rs::probe::{DebugProbeError as E, ProbeCreationError as C};
    let maybe_busy = matches!(e, E::ProbeCouldNotBeCreated(C::CouldNotOpen) | E::Usb(_));
    if !maybe_busy {
        return anyhow!("{e}");
    }
    if let Some(who) = crate::holders::holder_of_usb_device(
        info.vendor_id,
        info.product_id,
        info.serial_number.as_deref(),
    ) {
        return anyhow!("{e}; held by {who}");
    }
    anyhow!("{e}")
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
