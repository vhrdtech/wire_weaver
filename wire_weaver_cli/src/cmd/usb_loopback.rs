use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};
use tokio::sync::mpsc;
use tracing::{error, info};
use wire_weaver_usb_host::wire_weaver_client_common::{Command, TestProgress};

pub(crate) async fn usb_loopback(device: &mut mpsc::UnboundedSender<Command>) -> Result<()> {
    let (progress_tx, mut progress_rx) = mpsc::unbounded_channel();
    device.send(Command::LoopbackTest {
        use_prbs: false,
        test_duration: None,
        measure_tx_speed: false,
        measure_rx_speed: false,
        progress_tx,
    })?;
    let pb = ProgressBar::new(1000);
    pb.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} {msg} [{elapsed_precise}] [{bar:.cyan/blue}] ({eta})",
        )?
        .progress_chars("#>-"),
    );
    while let Some(progress) = progress_rx.recv().await {
        match progress {
            TestProgress::TestStarted(name) => {
                info!("Test started: {name}");
                pb.set_message(name);
            }
            TestProgress::Completion(_, completion) => {
                pb.set_position((completion * 1000.0) as u64);
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
                pb.finish_with_message(
                    format!("✅ loopback: round-trip packets: {tx_count} ({per_s}/s) lost: {lost_count} corrupted: {data_corrupted_count}")
                );
                if lost_count != 0 || data_corrupted_count != 0 {
                    error!("USB errors detected");
                    print_probable_usb_problems();
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
