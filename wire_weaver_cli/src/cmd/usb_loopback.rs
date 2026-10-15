use anyhow::{Result, bail};
use console::style;
use human_repr::HumanThroughput;
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
    if errors || fatal.is_some() {
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
    let msg = format!(
        "{mark} {}: {} ({})",
        r.name,
        r.count,
        r.per_s.human_throughput_bare()
    );
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
            "stage", "messages", "per second", "bytes/s", "lost", "corrupted"
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
            .map(|b| b.human_throughput("B").to_string())
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
            r.per_s.human_throughput_bare().to_string(),
            bytes,
            count(r.lost),
            count(r.corrupted),
        );
    }
}

fn print_probable_usb_problems() {
    info!("Probable issues:");
    error!("* Bad USB cable");
    error!("* Malfunctioning hub");
    error!("* Powered hub used without power");
    error!("* Power issues at device if it is externally powered");
}
