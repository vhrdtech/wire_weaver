use crate::ww_nusb::{Sink, Source};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use wire_weaver_client_common::TestProgress;
use wire_weaver_usb_link::{MessageKind, WireWeaverUsbLink};

pub(crate) async fn loopback_test(
    _use_prbs: bool,
    test_duration: Option<Duration>,
    _measure_tx_speed: bool,
    _measure_rx_speed: bool,
    progress_tx: mpsc::UnboundedSender<TestProgress>,
    link: &mut WireWeaverUsbLink<'_, Sink, Source>,
    scratch: &mut [u8],
) {
    _ = progress_tx.send(TestProgress::TestStarted("loopback"));
    let test_duration = test_duration.unwrap_or(Duration::from_secs(10));
    let end_instant = Instant::now() + test_duration;
    let mut last_progress_sent = None;
    let mut seq = 0;
    let mut tx_count = 0;
    let mut lost_count = 0;
    let mut data_corrupted_count = 0;
    loop {
        let now = Instant::now();
        if now >= end_instant {
            break;
        }
        if let Some(instant) = last_progress_sent {
            if now
                .checked_duration_since(instant)
                .unwrap_or(Duration::from_millis(0))
                >= Duration::from_millis(50)
            {
                let remaining = end_instant
                    .checked_duration_since(now)
                    .unwrap_or(Duration::from_millis(0))
                    .as_micros() as f32
                    / test_duration.as_micros() as f32;
                _ = progress_tx.send(TestProgress::Completion("loopback", 1.0 - remaining));
                last_progress_sent = Some(Instant::now());
            }
        } else {
            last_progress_sent = Some(Instant::now());
        }
        let tx_data = &[1, 2, 3, 4, 5, 6];
        let r = link.send_loopback(1, seq, tx_data).await;
        if let Err(e) = r {
            _ = progress_tx.send(TestProgress::FatalError(format!("tx error: {e:?}")));
            return;
        }
        tx_count += 1;
        let (rx_seq, rx_data) = match link.receive_message(scratch).await {
            Ok(MessageKind::Loopback { seq, len }) => (seq, &scratch[..len]),
            Ok(MessageKind::Ping) => match link.receive_message(scratch).await {
                Ok(MessageKind::Loopback { seq, len }) => (seq, &scratch[..len]),
                Ok(m) => {
                    _ = progress_tx.send(TestProgress::FatalError(format!(
                        "unexpected message: {m:?}"
                    )));
                    return;
                }
                Err(e) => {
                    _ = progress_tx.send(TestProgress::FatalError(format!("rx error: {e:?}")));
                    return;
                }
            },
            Ok(m) => {
                _ = progress_tx.send(TestProgress::FatalError(format!(
                    "unexpected message: {m:?}"
                )));
                return;
            }
            Err(e) => {
                _ = progress_tx.send(TestProgress::FatalError(format!("rx error: {e:?}")));
                return;
            }
        };
        if rx_seq != seq {
            lost_count += 1;
        }
        if tx_data != rx_data {
            data_corrupted_count += 1;
        }
        seq = seq.wrapping_add(1);
    }
    _ = progress_tx.send(TestProgress::LoopbackReport {
        tx_count,
        per_s: tx_count as f32 / test_duration.as_secs_f32(),
        lost_count,
        data_corrupted_count,
    });
    _ = progress_tx.send(TestProgress::TestCompleted("loopback"));
}
