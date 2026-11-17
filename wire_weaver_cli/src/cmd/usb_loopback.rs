use crate::registry::{BoxFuture, CommandDef, Connection, DeviceUse, GlobalFlags, Ran, Run};
use anyhow::{Result, bail};
use clap::{Args, FromArgMatches};
use clap_complete::ArgValueCandidates;
use console::style;
use human_repr::HumanCount;
use indicatif::{ProgressBar, ProgressStyle};
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{error, info};
use wire_weaver_client::{
    DynClient,
    internal::{Command, LoopbackParams, TestProgress},
};

/// Result of one stage, for the summary at the end.
struct StageResult {
    name: &'static str,
    count: u64,
    per_s: f32,
    bytes_per_s: Option<f32>,
    lost: u64,
    corrupted: u64,
}

pub(crate) async fn usb_loopback(
    device: &DynClient,
    duration_sec: u32,
    packet_size: String,
    params: LoopbackParams,
) -> Result<()> {
    let (progress_tx, mut progress_rx) = mpsc::unbounded_channel();
    let packet_size = match packet_size.as_str() {
        "max" => None,
        s => Some(s.parse::<usize>().map_err(|_| {
            anyhow::anyhow!("--packet-size: expected a number of bytes or max, got '{s}'")
        })?),
    };
    device
        .cmd()
        .send(Command::LoopbackTest {
            test_duration: Duration::from_secs(duration_sec as u64),
            packet_size,
            params: Box::new(params),
            progress_tx,
        })
        .await?;
    let mut progress_bar = None;
    let mut results = vec![];
    let mut fatal = None;
    // the event loop drops progress_tx when the test is over
    while let Some(progress) = progress_rx.recv().await {
        match progress {
            TestProgress::TestStarted(name) => {
                let pb = ProgressBar::new(1000);
                pb.set_style(
                    ProgressStyle::with_template(
                        "{spinner:.green} {msg:8} [{elapsed_precise}] [{bar:.cyan/blue}] ({eta})",
                    )?
                    .progress_chars("#>-"),
                );
                pb.set_message(name);
                progress_bar = Some(pb);
            }
            TestProgress::Completion(_, completion) => {
                if let Some(pb) = &mut progress_bar {
                    pb.set_position((completion * 1000.0) as u64);
                }
            }
            TestProgress::TestCompleted(_) => {}
            TestProgress::FatalError(e) => {
                if let Some(pb) = progress_bar.take() {
                    pb.abandon();
                }
                error!("{e}");
                fatal = Some(e);
            }
            TestProgress::LoopbackReport {
                tx_count,
                per_s,
                lost_count,
                data_corrupted_count,
            } => {
                let r = StageResult {
                    name: "loopback",
                    count: tx_count,
                    per_s,
                    bytes_per_s: None,
                    lost: lost_count,
                    corrupted: data_corrupted_count,
                };
                finish(progress_bar.take(), &r);
                results.push(r);
            }
            TestProgress::SpeedReport {
                name,
                count,
                per_s,
                bytes_per_s,
                lost_count,
                data_corrupted_count,
            } => {
                let r = StageResult {
                    name,
                    count,
                    per_s,
                    bytes_per_s: Some(bytes_per_s),
                    lost: lost_count,
                    corrupted: data_corrupted_count,
                };
                finish(progress_bar.take(), &r);
                results.push(r);
            }
        }
    }
    print_summary(&results);
    let errors = results.iter().any(|r| r.lost != 0 || r.corrupted != 0);
    if fatal.is_some() {
        // a stall is more likely a device that can't keep up than the cable
        info!(
            "If it stalls with more in flight than the defaults, the device can't keep up with that: fewer \
            --markers-in-flight / --batches-in-flight / --rx-batch, a longer --echo-timeout-ms or a smaller \
            --packet-size. If it stalls at the defaults, check the device's loopback support and the link."
        );
    } else if errors {
        print_probable_usb_problems();
    }
    match fatal {
        Some(e) => bail!("loopback test failed: {e}"),
        None if errors => bail!("lost or corrupted messages"),
        None if results.len() < 3 => bail!("loopback test ended early"),
        None => Ok(()),
    }
}

fn finish(pb: Option<ProgressBar>, r: &StageResult) {
    let mark = if r.lost == 0 && r.corrupted == 0 {
        "✅"
    } else {
        "❌"
    };
    let msg = format!("{mark} {}: {} ({})", r.name, r.count, per_second(r.per_s));
    match pb {
        Some(pb) => pb.finish_with_message(msg),
        None => info!("{msg}"),
    }
}

fn print_summary(results: &[StageResult]) {
    if results.is_empty() {
        return;
    }
    println!();
    println!(
        "{}",
        style(format!(
            "{:<10} {:>12} {:>12} {:>12} {:>8} {:>10}",
            "stage", "messages", "messages/s", "bytes/s", "lost", "corrupted"
        ))
        .bold()
    );
    for r in results {
        let what = match r.name {
            "loopback" => "round trip",
            "tx_speed" => "host→dev",
            "rx_speed" => "dev→host",
            other => other,
        };
        let bytes = r
            .bytes_per_s
            .map(|b| format!("{}/s", b.human_count_bytes()))
            .unwrap_or_else(|| "-".into());
        // host → device: the device discards the messages, losses aren't seen
        let tracked = r.name != "tx_speed";
        let count = |n: u64| {
            if !tracked {
                style("-".to_string()).dim()
            } else if n == 0 {
                style(n.to_string()).green()
            } else {
                style(n.to_string()).red().bold()
            }
        };
        println!(
            "{:<10} {:>12} {:>12} {:>12} {:>8} {:>10}",
            what,
            r.count,
            per_second(r.per_s),
            bytes,
            count(r.lost),
            count(r.corrupted),
        );
    }
}

/// Always per second (`human_throughput` switches to /min and /d for small rates).
fn per_second(per_s: f32) -> String {
    format!("{}/s", per_s.human_count_bare())
}

fn print_probable_usb_problems() {
    info!("Probable issues:");
    error!("* Bad USB cable");
    error!("* Malfunctioning hub");
    error!("* Powered hub used without power");
    error!("* Power issues at device if it is externally powered");
}

#[derive(Args)]
pub(crate) struct LoopbackArgs {
    /// How long to run each stage (loopback, tx_speed, rx_speed), in seconds
    #[arg(long, default_value = "10")]
    duration_sec: u32,

    /// Size of each test message in bytes (8 bytes of header and data), or max: the largest message both
    /// the device and the host accept
    #[arg(long, default_value = "max", add = ArgValueCandidates::new(crate::complete::packet_sizes))]
    packet_size: String,

    #[command(flatten)]
    tuning: LoopbackTuning,
}

/// Timeouts and flow control of `ww usb-loopback`, defaults from [LoopbackParams::default].
#[derive(Args)]
#[command(next_help_heading = "Test tuning")]
pub(crate) struct LoopbackTuning {
    /// An echo (round trip, marker or device → host reply) missing for this long is lost
    #[arg(long, value_name = "MS", default_value_t = LoopbackParams::default().echo_timeout.as_millis() as u64)]
    pub echo_timeout_ms: u64,

    /// Give up after this many echo timeouts in a row
    #[arg(long, value_name = "N", default_value_t = LoopbackParams::default().timeouts_in_a_row)]
    pub timeouts_in_a_row: u32,

    /// Host → device: messages between two echoed markers
    #[arg(long, value_name = "N", default_value_t = LoopbackParams::default().marker_every)]
    pub marker_every: u32,

    /// Host → device: markers in flight at most, the host waits for their echoes beyond that
    #[arg(long, value_name = "N", default_value_t = LoopbackParams::default().markers_in_flight)]
    pub markers_in_flight: u32,

    /// Device → host: messages the device sends for one request (at least 2)
    #[arg(long, value_name = "N", default_value_t = LoopbackParams::default().rx_batch)]
    pub rx_batch: u32,

    /// Device → host: requests in flight at most
    #[arg(long, value_name = "N", default_value_t = LoopbackParams::default().batches_in_flight)]
    pub batches_in_flight: u32,
}

impl LoopbackTuning {
    pub fn params(&self) -> LoopbackParams {
        LoopbackParams {
            echo_timeout: std::time::Duration::from_millis(self.echo_timeout_ms),
            timeouts_in_a_row: self.timeouts_in_a_row,
            marker_every: self.marker_every,
            markers_in_flight: self.markers_in_flight,
            rx_batch: self.rx_batch,
            batches_in_flight: self.batches_in_flight,
        }
    }
}

/// `ww usb-loopback`.
fn command() -> clap::Command {
    LoopbackArgs::augment_args(clap::Command::new("usb-loopback"))
        .about("Test the link to a device: round trips, then host → device and device → host speed")
        .long_about(
            "Test the link to a device: round trips, then host → device and device → host speed\n\n\
             Works with any WireWeaver device, whatever its API: the device's link layer answers the loopback \
             messages. Reports round trips per second with lost and corrupted echoes, then one-way messages and \
             bytes per second in each direction.",
        )
}

fn run(run: Run<'_>) -> BoxFuture<'_, Result<Ran>> {
    Box::pin(async move {
        let args = LoopbackArgs::from_arg_matches(&run.matches)?;
        usb_loopback(
            run.device(),
            args.duration_sec,
            args.packet_size,
            args.tuning.params(),
        )
        .await?;
        Ok(Ran::DONE)
    })
}

inventory::submit! {
    CommandDef::new("usb-loopback", 20, command, run)
        .globals(&[("", GlobalFlags::Device)])
        // the loopback test does not call the API at all, the link layer answers
        .connection(|_| Ok(Connection::Device(DeviceUse::TRAITS_ONLY)))
}
