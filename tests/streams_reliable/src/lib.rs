//! Counted and reliable streams end to end (API-30, API-31): stream functions opened with arguments, their result,
//! positions and device losses, the store, status reads, close and resume with backfill. The device runs the
//! generated async server with `poll_streams` between requests, the host the real event loop (`tests_common`).

#[cfg(test)]
mod tests {
    use std::time::Duration;
    use streams_reliable_api::{Sample, SampleActual, SampleConfig, SampleError};
    use tests_common::TestDevice;
    use wire_weaver::prelude::*;
    use wire_weaver_client::{Cause, Run, StreamEvent};
    use ww_si::Second;
    use ww_si::ww_numeric::NumericValue;
    use ww_stream::store::RamRing;
    use ww_stream::tx::{Counted, Reliable};

    mod server {
        use super::*;
        use tests_common::TestProcessEventsAsync;

        pub struct Server {
            pub rx_stream: Counted,
            pub samples_stream: Reliable<RamRing<16384>>,
            pub chunks_stream: Counted,
            pub stream_sessions: u32,
            pub next_value: u32,
            pub buf: [u8; 256],
            /// Device clock of the rx events in ms, sent as a wrapping `u16`
            pub rx_ms: u64,
        }

        impl Server {
            pub fn new() -> Self {
                Server {
                    rx_stream: Counted::new(),
                    samples_stream: Reliable::new(RamRing::new()),
                    chunks_stream: Counted::new(),
                    stream_sessions: 0,
                    next_value: 0,
                    buf: [0; 256],
                    rx_ms: RX_MS0,
                }
            }

            async fn rx(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
                RpcResult::Ready(())
            }

            async fn close_rx(&mut self, _cx: &mut Context<'_, impl EventOut>) {}

            async fn stream_rx(&mut self) -> &mut Counted {
                &mut self.rx_stream
            }

            async fn samples(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                config: SampleConfig,
            ) -> RpcResult<Result<SampleActual, SampleError>> {
                if config.rate == 0 {
                    return RpcResult::Ready(Err(SampleError::BadRate));
                }
                if config.rate == u32::MAX {
                    return RpcResult::Ready(Err(SampleError::TooFast));
                }
                RpcResult::Ready(Ok(SampleActual {
                    rate: config.rate.min(1000),
                }))
            }

            async fn close_samples(&mut self, _cx: &mut Context<'_, impl EventOut>) {}

            async fn stream_samples(&mut self) -> &mut Reliable<RamRing<16384>> {
                &mut self.samples_stream
            }

            async fn chunks(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                _chunk: u8,
            ) -> RpcResult<()> {
                RpcResult::Ready(())
            }

            async fn close_chunks(&mut self, _cx: &mut Context<'_, impl EventOut>) {}

            async fn stream_chunks(&mut self) -> &mut Counted {
                &mut self.chunks_stream
            }

            async fn emit_rx(
                &mut self,
                cx: &mut Context<'_, impl EventOut>,
                len: u8,
                count: u8,
            ) -> RpcResult<()> {
                let ser = api_impl::stream_data_ser();
                for i in 0..count {
                    let data = vec![i; len as usize];
                    let t = Second::milli(self.rx_ms as u16);
                    self.rx_ms += 20;
                    ser.rx_send(&mut self.rx_stream, &data, len as u64, Some(t), cx)
                        .await
                        .unwrap();
                }
                RpcResult::Ready(())
            }

            async fn lose_rx(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                units: u32,
            ) -> RpcResult<()> {
                self.rx_stream.lost(units as u64);
                RpcResult::Ready(())
            }

            async fn produce(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                count: u32,
            ) -> RpcResult<()> {
                let ser = api_impl::stream_data_ser();
                for _ in 0..count {
                    let sample = Sample {
                        value: self.next_value,
                    };
                    self.next_value += 1;
                    ser.samples_push(&mut self.samples_stream, &sample, 1, None, &mut self.buf)
                        .await
                        .unwrap();
                }
                RpcResult::Ready(())
            }

            fn new_stream_session(&mut self) -> u32 {
                self.stream_sessions = self.stream_sessions.wrapping_add(1);
                self.stream_sessions
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                streams_reliable_api :: StreamsReliable for super::Server,
                server = true, no_alloc = true, use_async = true,
                method_model = "_=immediate",
                property_model = "_=get_set",
                // debug_to_file = "../../target/tests_streams_reliable_server.rs"
            );
        }

        impl TestProcessEventsAsync for Server {
            type Medium = ();

            async fn process_request_bytes<'a>(
                &mut self,
                bytes: &[u8],
                scratch: &'a mut [u8],
                out: &mut impl EventOut,
                medium: (),
            ) -> Result<&'a [u8], ShrinkWrapError> {
                self.process_request_bytes(bytes, scratch, out, medium)
                    .await
            }

            async fn poll(&mut self, out: &mut impl EventOut, now_ms: u64) {
                let mut buf = [0u8; 256];
                _ = self.poll_streams(out, now_ms, &mut buf, 4096).await;
            }
        }
    }

    mod client {
        use wire_weaver_client::{ClientConfig, Commander, WwClient};

        pub struct Client {
            pub cmd: Commander,
        }

        impl WwClient for Client {
            fn default_config() -> ClientConfig {
                ClientConfig::default()
            }

            fn from_cmd(cmd: Commander) -> Self {
                Self { cmd }
            }
        }

        mod api_client {
            wire_weaver::ww_codegen!(
                streams_reliable_api :: StreamsReliable for super::Client,
                client = "std_client",
                // debug_to_file = "../../target/tests_streams_reliable_client.rs"
            );
        }
    }

    use client::Client;

    /// Device clock of the first rx event, ms: the third one is past the `u16` wrap at 65 536
    const RX_MS0: u64 = 65_500;

    fn start(path: &str) -> TestDevice {
        let _ = tracing_subscriber::fmt::try_init();
        tests_common::start_device_async(
            path,
            server::Server::new(),
            streams_reliable_api::STREAMS_RELIABLE_FULL_GID,
            server::api_impl::api_hash(),
        )
    }

    async fn connect(device: &TestDevice) -> Client {
        Client::config(|c| c.in_process_path(device.path()))
            .connect()
            .await
            .expect("connect")
    }

    async fn next<T: DeserializeShrinkWrapOwned>(
        s: &mut wire_weaver_client::PositionedStream<T>,
    ) -> StreamEvent<T> {
        let r = tokio::time::timeout(Duration::from_secs(2), s.recv_event()).await;
        r.expect("an event in time").expect("stream open")
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn counted_positions_and_device_losses() {
        let device = start("streams_reliable/counted");
        let mut client = connect(&device).await;
        let mut rx = client.rx().call().await.unwrap();
        assert_eq!(rx.session(), Some(1));

        client.emit_rx(3, 2).call().await.unwrap();
        for (i, pos) in [(0u8, 0u64), (1, 3)] {
            match next(&mut rx).await {
                StreamEvent::Item {
                    pos: p,
                    units,
                    run,
                    value,
                    t,
                    ..
                } => {
                    assert_eq!((p, units, run), (pos, 3, Run::Live));
                    assert_eq!(value.0, vec![i; 3]);
                    assert_eq!(t, Some(Second::milli(RX_MS0 + 20 * i as u64)));
                }
                ev => panic!("{ev:?}"),
            }
        }

        // the producer dropped 100 bytes: the next event says so, exactly where
        client.lose_rx(100).call().await.unwrap();
        client.emit_rx(2, 1).call().await.unwrap();
        match next(&mut rx).await {
            StreamEvent::Gap {
                at, units, cause, ..
            } => assert_eq!((at, units, cause), (6, 100, Cause::Device)),
            ev => panic!("{ev:?}"),
        }
        match next(&mut rx).await {
            StreamEvent::Item { pos, units, t, .. } => {
                assert_eq!((pos, units), (106, 2));
                // the device's u16 of ms wrapped, the head's time counts on as a U64
                let t = t.unwrap();
                assert_eq!(t.value, NumericValue::U64(RX_MS0 + 40));
                assert_eq!(t.prefix, ww_si::Prefix::Milli);
            }
            ev => panic!("{ev:?}"),
        }
    }

    /// tests_common makes the client strict: an open error outside the stream function's `#[errors(..)]` fails the
    /// test (API-32).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[should_panic(
        expected = "'StreamsReliable::samples' returned TooFast, which is not in its #[errors(BadRate)]"
    )]
    async fn unlisted_open_error_fails_in_tests() {
        let device = start("streams_reliable/unlisted_open_error");
        let mut client = connect(&device).await;
        let _ = client.samples(SampleConfig { rate: u32::MAX }).call().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn stream_function_result_and_error() {
        let device = start("streams_reliable/result");
        let mut client = connect(&device).await;
        let r = client
            .samples(SampleConfig { rate: 0 })
            .call()
            .await
            .unwrap();
        assert!(matches!(r, Err(SampleError::BadRate)), "{:?}", r.err());

        let (actual, _samples) = client
            .samples(SampleConfig { rate: 5000 })
            .call()
            .await
            .unwrap()
            .unwrap();
        assert_eq!(actual, SampleActual { rate: 1000 });

        // a counted stream with an argument and no result
        let chunks = client.chunks(8).call().await.unwrap();
        assert_eq!(chunks.session(), Some(2));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reliable_in_order_with_status() {
        let device = start("streams_reliable/in_order");
        let mut client = connect(&device).await;
        let (_, mut samples) = client
            .samples(SampleConfig { rate: 10 })
            .call()
            .await
            .unwrap()
            .unwrap();
        let mut expect = 0u64;
        for _ in 0..2 {
            client.produce(100).call().await.unwrap();
            for _ in 0..100 {
                match next(&mut samples).await {
                    StreamEvent::Item {
                        pos, run, value, ..
                    } => {
                        assert_eq!((pos, run), (expect, Run::Live));
                        assert_eq!(
                            value,
                            Sample {
                                value: expect as u32
                            }
                        );
                    }
                    ev => panic!("{ev:?}"),
                }
                expect += 1;
            }
        }

        // more than the 16 KiB store holds at once: the rest is counted as lost and reported with the next event
        client.produce(1000).call().await.unwrap();
        let mut items = 0;
        let mut lost = 0;
        // what fit is delivered and acknowledged, the store empties
        let mut status = client.samples_status().read().await.unwrap();
        while status.oldest.is_some() {
            if let Some(StreamEvent::Item { pos, .. }) = samples.try_recv_event().unwrap() {
                assert_eq!(pos, 200 + items);
                items += 1;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
            status = client.samples_status().read().await.unwrap();
        }
        client.produce(1).call().await.unwrap();
        loop {
            match next(&mut samples).await {
                StreamEvent::Item { pos: 1200, .. } => break,
                StreamEvent::Item { pos, .. } => {
                    assert_eq!(pos, 200 + items);
                    items += 1;
                }
                StreamEvent::Gap {
                    at, units, cause, ..
                } => {
                    assert_eq!((at, cause), (200 + items, Cause::Device));
                    lost += units;
                }
                ev => panic!("{ev:?}"),
            }
        }
        assert!(items > 100, "{items}");
        assert_eq!(items + lost, 1000);

        let status = client.samples_status().read().await.unwrap();
        assert!(status.open);
        assert_eq!(status.session, Some(UVlq32(1)));
        assert_eq!(status.next, UVlq64(1201));
        let store = status.store.expect("a reliable stream has a store");
        assert_eq!(store.capacity_bytes, UVlq64(16384));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn close_then_resume_with_backfill() {
        let device = start("streams_reliable/resume");
        let mut client = connect(&device).await;
        let (_, mut samples) = client
            .samples(SampleConfig { rate: 10 })
            .call()
            .await
            .unwrap()
            .unwrap();
        client.produce(10).call().await.unwrap();
        for _ in 0..10 {
            assert!(matches!(next(&mut samples).await, StreamEvent::Item { .. }));
        }
        let cursor = samples.cursor().expect("a cursor after items");
        assert_eq!(cursor.pos, UVlq64(10));
        samples.close().await.unwrap();
        drop(samples);

        // nobody listens: the store keeps them
        client.produce(30).call().await.unwrap();
        let status = client.samples_status().read().await.unwrap();
        assert!(!status.open);
        assert_eq!(status.next, UVlq64(40));

        let mut samples = client.samples_resume(cursor).call().await.unwrap();
        let mut values = vec![];
        while values.len() < 30 {
            match next(&mut samples).await {
                StreamEvent::Item { value, .. } => values.push(value.value),
                StreamEvent::Resumed { .. } | StreamEvent::BackfillDone => {}
                ev => panic!("{ev:?}"),
            }
        }
        values.sort();
        assert_eq!(values, (10..40).collect::<Vec<_>>());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn resume_without_a_session() {
        let device = start("streams_reliable/no_session");
        let client = connect(&device).await;
        let cursor = ww_stream::StreamCursor {
            session: UVlq32(7),
            pos: UVlq64(0),
        };
        assert!(client.samples_resume(cursor).call().await.is_err());
    }

    /// Every other acknowledgement is lost on the way: cumulative acks and the device's Query keep the store
    /// draining, nothing is lost or duplicated.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn lost_acknowledgements() {
        let device = start("streams_reliable/lost_acks");
        let mut drop_next = false;
        device.filter_requests(move |bytes| {
            let Ok(request) = ww_client_server::Request::from_ww_bytes(bytes) else {
                return false;
            };
            if matches!(
                request.kind,
                ww_client_server::RequestKind::StreamSideband { .. }
            ) {
                drop_next = !drop_next;
                return drop_next;
            }
            false
        });
        let mut client = connect(&device).await;
        let (_, mut samples) = client
            .samples(SampleConfig { rate: 10 })
            .call()
            .await
            .unwrap()
            .unwrap();
        for batch in 0..5u64 {
            client.produce(200).call().await.unwrap();
            for i in 0..200 {
                let ev =
                    tokio::time::timeout(Duration::from_millis(1500), samples.recv_event()).await;
                let ev = match ev {
                    Ok(ev) => ev.unwrap(),
                    Err(_) => {
                        let st = client.samples_status().read().await;
                        panic!(
                            "stuck at {batch}/{i}: host {:?}, device {st:?}",
                            samples.stats()
                        );
                    }
                };
                match ev {
                    StreamEvent::Item { pos, value, .. } => {
                        assert_eq!(pos, batch * 200 + i);
                        assert_eq!(value.value as u64, pos);
                    }
                    ev => panic!("{ev:?}"),
                }
            }
            // acknowledged in the end, also when the last Ack was lost (the device asks with a Query)
            let start = std::time::Instant::now();
            while client
                .samples_status()
                .read()
                .await
                .unwrap()
                .oldest
                .is_some()
            {
                assert!(
                    start.elapsed() < Duration::from_secs(2),
                    "store never drains"
                );
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
        assert_eq!(samples.stats().duplicates, 0);
    }
}
