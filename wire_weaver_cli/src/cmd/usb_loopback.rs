use anyhow::Result;
use human_repr::HumanThroughput;
use indicatif::{ProgressBar, ProgressStyle};
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{error, info};
use wire_weaver_usb_host::wire_weaver_client_common::{Command, TestProgress};

pub(crate) async fn usb_loopback(
    device: &mut mpsc::UnboundedSender<Command>,
    duration_sec: u32,
    packet_size: String,
) -> Result<()> {
    let (progress_tx, mut progress_rx) = mpsc::unbounded_channel();
    let packet_size = packet_size.parse::<usize>().ok();
    device.send(Command::LoopbackTest {
        test_duration: Duration::from_secs(duration_sec as u64),
        packet_size,
        progress_tx,
    })?;
    let mut progress_bar = None;
    while let Some(progress) = progress_rx.recv().await {
        match progress {
            TestProgress::TestStarted(name) => {
                info!("Test started: {name}");
                let pb = ProgressBar::new(1000);
                pb.set_style(
                    ProgressStyle::with_template(
                        "{spinner:.green} {msg} [{elapsed_precise}] [{bar:.cyan/blue}] ({eta})",
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
            TestProgress::TestCompleted(name) => {
                info!("Test completed: {name}");
            }
            TestProgress::FatalError(e) => {
                error!("Fatal error: {e}");
                print_probable_usb_problems();
            }
            TestProgress::LoopbackReport {
                tx_count,
                per_s,
                lost_count,
                data_corrupted_count,
            } => {
                if let Some(pb) = progress_bar.take() {
                    pb.finish_with_message(
                        format!("✅ loopback: round-trip packets: {tx_count} ({}) lost: {lost_count} corrupted: {data_corrupted_count}", per_s.human_throughput_bare())
                    );
                }
                if lost_count != 0 || data_corrupted_count != 0 {
                    error!("USB errors detected");
                    print_probable_usb_problems();
                }
            }
            TestProgress::SpeedReport {
                name,
                count: tx_count,
                per_s,
                bytes_per_s,
            } => {
                if let Some(pb) = progress_bar.take() {
                    pb.finish_with_message(format!(
                        "✅ {name}: one-way packets: {tx_count} ({}) ({})",
                        per_s.human_throughput_bare(),
                        bytes_per_s.human_throughput("B")
                    ));
                }
            }
        }
    }
    Ok(())
}

fn print_probable_usb_problems() {
    info!("Probable issues:");
    error!("* Bad USB cable");
    error!("* Malfunctioning hub");
    error!("* Powered hub used without power");
    error!("* Power issues at device if it is externally powered");
}
