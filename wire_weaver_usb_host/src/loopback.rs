use crate::ww_nusb::{Sink, Source};
use rand::RngCore;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use wire_weaver_client_common::TestProgress;
use wire_weaver_usb_link::{MessageKind, WireWeaverUsbLink};

const PACKET_OVERHEAD: usize = 2 + 4 + 4; // (opcode + len) + repeat + seq

pub(crate) async fn loopback_test(
    test_duration: Duration,
    packet_size: usize,
    mut progress_tx: mpsc::UnboundedSender<TestProgress>,
    link: &mut WireWeaverUsbLink<'_, Sink, Source>,
    scratch: &mut [u8],
) {
    let test_data_size = packet_size.saturating_sub(PACKET_OVERHEAD);
    println!("Test data size: {}", test_data_size);
    let r = round_trip(
        test_duration,
        &mut progress_tx,
        link,
        scratch,
        test_data_size,
    )
    .await;
    if r.is_err() {
        return;
    }
    let mut test_data = Vec::with_capacity(test_data_size);
    test_data.resize(test_data_size, 0xCC);
    let r = tx_speed(test_duration, &mut progress_tx, link, &test_data).await;
    if r.is_err() {
        return;
    }
    _ = rx_speed(test_duration, &mut progress_tx, link, &test_data, scratch).await;
}

struct TestTimer {
    end_instant: Instant,
    last_progress_sent: Option<Instant>,
    test_duration: Duration,
}

impl TestTimer {
    fn new(test_duration: Duration) -> TestTimer {
        Self {
            end_instant: Instant::now() + test_duration,
            last_progress_sent: None,
            test_duration,
        }
    }

    fn update(&mut self) -> (bool, Option<f32>) {
        let now = Instant::now();
        if now >= self.end_instant {
            return (true, None);
        }
        if let Some(instant) = self.last_progress_sent {
            if now
                .checked_duration_since(instant)
                .unwrap_or(Duration::from_millis(0))
                >= Duration::from_millis(50)
            {
                let remaining = self
                    .end_instant
                    .checked_duration_since(now)
                    .unwrap_or(Duration::from_millis(0))
                    .as_micros() as f32
                    / self.test_duration.as_micros() as f32;
                self.last_progress_sent = Some(Instant::now());
                (false, Some(1.0 - remaining))
            } else {
                (false, None)
            }
        } else {
            self.last_progress_sent = Some(Instant::now());
            (false, None)
        }
    }
}

async fn round_trip(
    test_duration: Duration,
    progress_tx: &mut mpsc::UnboundedSender<TestProgress>,
    link: &mut WireWeaverUsbLink<'_, Sink, Source>,
    scratch: &mut [u8],
    test_data_size: usize,
) -> Result<(), ()> {
    _ = progress_tx.send(TestProgress::TestStarted("loopback"));
    let mut timer = TestTimer::new(test_duration);
    let mut seq = 0;
    let mut tx_count = 0;
    let mut lost_count = 0;
    let mut data_corrupted_count = 0;
    let mut test_data = vec![0; test_data_size];
    loop {
        let (should_exit, send_progress) = timer.update();
        if should_exit {
            break;
        }
        if let Some(progress) = send_progress {
            _ = progress_tx.send(TestProgress::Completion("loopback", progress));
        }

        rand::rng().fill_bytes(&mut test_data);
        let r = link.send_loopback(1, seq, &test_data).await;
        if let Err(e) = r {
            _ = progress_tx.send(TestProgress::FatalError(format!("tx error: {e:?}")));
            return Err(());
        }
        tx_count += 1;
        let (rx_seq, rx_data) = receive_message(link, progress_tx, scratch).await?;
        if rx_seq != seq {
            lost_count += 1;
        }
        if test_data != rx_data {
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
    Ok(())
}

async fn tx_speed(
    test_duration: Duration,
    progress_tx: &mut mpsc::UnboundedSender<TestProgress>,
    link: &mut WireWeaverUsbLink<'_, Sink, Source>,
    test_data: &[u8],
) -> Result<(), ()> {
    _ = progress_tx.send(TestProgress::TestStarted("tx_speed"));
    let mut timer = TestTimer::new(test_duration);
    let mut tx_count = 0;
    loop {
        let (should_exit, send_progress) = timer.update();
        if should_exit {
            break;
        }
        if let Some(progress) = send_progress {
            _ = progress_tx.send(TestProgress::Completion("loopback", progress));
        }
        let r = link.send_loopback(0, 0, test_data).await;
        if let Err(e) = r {
            _ = progress_tx.send(TestProgress::FatalError(format!("tx error: {e:?}")));
            return Err(());
        }
        tx_count += 1;
    }
    let per_s = tx_count as f32 / test_duration.as_secs_f32();
    _ = progress_tx.send(TestProgress::SpeedReport {
        name: "tx_speed",
        count: tx_count,
        per_s,
        bytes_per_s: per_s * (test_data.len() + PACKET_OVERHEAD) as f32,
    });
    _ = progress_tx.send(TestProgress::TestCompleted("tx_speed"));
    Ok(())
}

async fn rx_speed(
    test_duration: Duration,
    progress_tx: &mut mpsc::UnboundedSender<TestProgress>,
    link: &mut WireWeaverUsbLink<'_, Sink, Source>,
    test_data: &[u8],
    scratch: &mut [u8],
) -> Result<(), ()> {
    _ = progress_tx.send(TestProgress::TestStarted("rx_speed"));
    let mut timer = TestTimer::new(test_duration);
    let mut rx_count = 0;
    const BATCH_SIZE: u32 = 256;
    loop {
        let (should_exit, send_progress) = timer.update();
        if should_exit {
            break;
        }
        if let Some(progress) = send_progress {
            _ = progress_tx.send(TestProgress::Completion("loopback", progress));
        }
        let r = link.send_loopback(BATCH_SIZE, 0, test_data).await;
        if let Err(e) = r {
            _ = progress_tx.send(TestProgress::FatalError(format!("tx error: {e:?}")));
            return Err(());
        }
        for expected_seq in 0..BATCH_SIZE {
            let (rx_seq, rx_data) = receive_message(link, progress_tx, scratch).await?;
            if rx_seq != expected_seq {
                _ = progress_tx.send(TestProgress::FatalError(format!(
                    "rx seq miss: expected: {expected_seq}, got: {rx_seq}"
                )));
                return Err(());
            }
            if rx_data != test_data {
                _ = progress_tx.send(TestProgress::FatalError("received wrong data".into()));
                return Err(());
            }
            rx_count += 1;
        }
    }
    let per_s = rx_count as f32 / test_duration.as_secs_f32();
    _ = progress_tx.send(TestProgress::SpeedReport {
        name: "rx_speed",
        count: rx_count,
        per_s,
        bytes_per_s: per_s * (test_data.len() + PACKET_OVERHEAD) as f32,
    });
    _ = progress_tx.send(TestProgress::TestCompleted("rx_speed"));
    Ok(())
}

async fn receive_message<'i>(
    link: &mut WireWeaverUsbLink<'_, Sink, Source>,
    progress_tx: &mut mpsc::UnboundedSender<TestProgress>,
    scratch: &'i mut [u8],
) -> Result<(u32, &'i [u8]), ()> {
    let mut rx_seq_data = None;
    let mut last_kind = String::new();
    for _ in 0..3 {
        match tokio::time::timeout(Duration::from_secs(1), link.receive_message(scratch)).await {
            Ok(Ok(MessageKind::Loopback { seq, len, .. })) => {
                rx_seq_data = Some((seq, &scratch[..len]));
                break;
            }
            Ok(Ok(m)) => last_kind = format!("{m:?}"),
            Ok(Err(e)) => {
                _ = progress_tx.send(TestProgress::FatalError(format!("rx error: {e:?}")));
                return Err(());
            }
            Err(_timeout) => {
                _ = progress_tx.send(TestProgress::FatalError("rx timeout".into()));
                return Err(());
            }
        }
    }
    if let Some((rx_seq, rx_data)) = rx_seq_data {
        Ok((rx_seq, rx_data))
    } else {
        _ = progress_tx.send(TestProgress::FatalError(format!(
            "unexpected message: {last_kind}"
        )));
        Err(())
    }
}
