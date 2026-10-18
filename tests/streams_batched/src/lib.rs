//! Batched streams end to end (API-41): plain, counted and reliable streams whose items the device collects in a
//! `ww_stream::batch::Batcher` and `poll_streams` sends as one `Series` per event; the typed client gives one item
//! per batch entry, each with its own time, or whole batches. The device runs the generated async server, the host
//! the real event loop (`tests_common`).

#[cfg(test)]
mod tests {
    use std::time::Duration;
    use streams_batched_api::{Frame, Sample, SampleError};
    use tests_common::TestDevice;
    use wire_weaver::prelude::*;
    use wire_weaver_client::{Cause, Run, StreamEvent};
    use ww_si::{Second, Timed};
    use ww_stream::batch::{Batched, Plain, Scratch, scratch_bytes};
    use ww_stream::store::RamRing;
    use ww_stream::tx::{Counted, Reliable};

    /// Items per batch on the device.
    const N: usize = 8;

    mod server {
        use super::*;
        use tests_common::TestProcessEventsAsync;
        use ww_stream::batch::DEFAULT_POLICY;

        pub struct Server {
            pub plain_stream: Batched<Plain, Timed<Sample>, N>,
            pub counted_stream: Batched<Counted, Timed<Sample>, N>,
            pub reliable_stream: Batched<Reliable<RamRing<16384>>, Timed<Sample>, N>,
            pub frames_stream: Batched<Counted, Frame, N>,
            pub gain: i32,
            pub next_value: i32,
            pub stream_sessions: u32,
        }

        impl Server {
            pub fn new() -> Self {
                Server {
                    plain_stream: Batched::new(Plain::new(), DEFAULT_POLICY),
                    counted_stream: Batched::new(Counted::new(), DEFAULT_POLICY),
                    reliable_stream: Batched::new(Reliable::new(RamRing::new()), DEFAULT_POLICY),
                    frames_stream: Batched::new(Counted::new(), DEFAULT_POLICY),
                    gain: 1,
                    next_value: 0,
                    stream_sessions: 0,
                }
            }

            async fn plain(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
                RpcResult::Ready(())
            }

            async fn close_plain(&mut self, _cx: &mut Context<'_, impl EventOut>) {}

            async fn sideband_plain(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                _sideband: ww_client_server::StreamSideband,
            ) -> Option<ww_client_server::StreamSideband> {
                None
            }

            async fn stream_plain(&mut self) -> &mut Batched<Plain, Timed<Sample>, N> {
                &mut self.plain_stream
            }

            async fn counted(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
                RpcResult::Ready(())
            }

            async fn close_counted(&mut self, _cx: &mut Context<'_, impl EventOut>) {}

            async fn stream_counted(&mut self) -> &mut Batched<Counted, Timed<Sample>, N> {
                &mut self.counted_stream
            }

            async fn reliable(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                gain: i32,
            ) -> RpcResult<Result<(), SampleError>> {
                if gain == 0 {
                    return RpcResult::Ready(Err(SampleError::ZeroGain));
                }
                self.gain = gain;
                RpcResult::Ready(Ok(()))
            }

            async fn close_reliable(&mut self, _cx: &mut Context<'_, impl EventOut>) {}

            async fn stream_reliable(
                &mut self,
            ) -> &mut Batched<Reliable<RamRing<16384>>, Timed<Sample>, N> {
                &mut self.reliable_stream
            }

            async fn frames(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
                RpcResult::Ready(())
            }

            async fn close_frames(&mut self, _cx: &mut Context<'_, impl EventOut>) {}

            async fn stream_frames(&mut self) -> &mut Batched<Counted, Frame, N> {
                &mut self.frames_stream
            }

            async fn produce(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                count: u32,
                t0_us: u32,
            ) -> RpcResult<()> {
                let ser = api_impl::stream_data_ser();
                // plain and counted as one block of regularly sampled values (t0 + k * dt, wrapping at 2^32 µs),
                // reliable and frames item by item
                let t0 = Second::micro(t0_us);
                let dt = Second::micro(40u8);
                let first = self.next_value;
                let block = (first..first + count as i32).map(|value| Sample { value });
                ser.plain_push_many(&mut self.plain_stream, t0, dt, block.clone())
                    .expect("40 µs is a whole number of µs");
                ser.counted_push_many(&mut self.counted_stream, t0, dt, block)
                    .expect("40 µs is a whole number of µs");
                for k in 0..count {
                    let t = Second::micro(t0_us.wrapping_add(k * 40));
                    let value = self.next_value;
                    self.next_value += 1;
                    let sample = Sample {
                        value: value * self.gain,
                    };
                    ser.reliable_push(&mut self.reliable_stream, sample, t);
                    let frame = Frame {
                        number: value as u32,
                        ok: value % 3 != 0,
                    };
                    ser.frames_push(&mut self.frames_stream, frame);
                }
                RpcResult::Ready(())
            }

            async fn flush(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
                self.plain_stream.batcher.flush();
                self.counted_stream.batcher.flush();
                self.reliable_stream.batcher.flush();
                self.frames_stream.batcher.flush();
                RpcResult::Ready(())
            }

            fn new_stream_session(&mut self) -> u32 {
                self.stream_sessions = self.stream_sessions.wrapping_add(1);
                self.stream_sessions
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                streams_batched_api :: StreamsBatched for super::Server,
                server = true, no_alloc = true, use_async = true,
                method_model = "_=immediate",
                property_model = "_=get_set",
                // debug_to_file = "../../target/tests_streams_batched_server.rs"
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
                // where a batch's items are rebuilt while it is encoded, as big as the biggest batch
                let mut scratch = Scratch::<{ scratch_bytes::<Timed<Sample>>(N) }>::new();
                _ = self
                    .poll_streams(out, now_ms, &mut buf, 4096, &mut scratch)
                    .await;
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
                streams_batched_api :: StreamsBatched for super::Client,
                client = "std_client",
                // debug_to_file = "../../target/tests_streams_batched_client.rs"
            );
        }
    }

    use client::Client;

    const WAIT: Duration = Duration::from_secs(2);

    fn start(path: &str) -> TestDevice {
        let _ = tracing_subscriber::fmt::try_init();
        tests_common::start_device_async(
            path,
            server::Server::new(),
            streams_batched_api::STREAMS_BATCHED_FULL_GID,
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
        let r = tokio::time::timeout(WAIT, s.recv_event()).await;
        r.expect("an event in time").expect("stream open")
    }

    /// Device time of the `k`-th sample from `t0_us`, as the host unwraps it.
    fn t_of(t0_us: u32, k: u64) -> Option<Second> {
        Some(Second::micro(t0_us as u64 + k * 40))
    }

    /// `<name>_push_many` stamps a block `t0 + k * dt` exactly, wrapping like the device clock, and pushes nothing
    /// when `dt` is not a whole number of `t0`'s prefix.
    #[test]
    fn push_many_stamps_a_block() {
        let ser = server::api_impl::stream_data_ser();
        let mut stream =
            Batched::<Plain, Timed<Sample>, N>::new(Plain::new(), ww_stream::batch::DEFAULT_POLICY);
        let block = (0..3).map(|value| Sample { value });
        assert_eq!(
            ser.plain_push_many(
                &mut stream,
                Second::milli(5u32),
                Second::micro(40u8),
                block.clone()
            ),
            None
        );
        assert_eq!(stream.batcher.filling(), 0);
        let (_, lost) = ser
            .plain_push_many(
                &mut stream,
                Second::milli(u32::MAX),
                Second::milli(2u8),
                block,
            )
            .unwrap();
        assert_eq!((lost, stream.batcher.filling()), (0, 3));
    }

    /// Items one by one with their positions and own times, across batches sealed full, by a flush and by age; the
    /// device clock wraps in the middle and the host unwraps it.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn counted_items_with_their_times() {
        let device = start("streams_batched/counted");
        let mut client = connect(&device).await;
        let mut counted = client
            .counted()
            .max_age(Duration::from_millis(5))
            .call()
            .await
            .unwrap();
        let t0 = u32::MAX - 200;
        // one full batch of 8, the other 2 go out once they are 5 ms old
        client.produce(10, t0).call().await.unwrap();
        for k in 0..10u64 {
            match next(&mut counted).await {
                StreamEvent::Item {
                    pos,
                    units,
                    t,
                    run,
                    value,
                    ..
                } => {
                    assert_eq!((pos, units, run), (k, 1, Run::Live));
                    assert_eq!(t, t_of(t0, k), "item {k}");
                    assert_eq!(value, Sample { value: k as i32 });
                }
                ev => panic!("{ev:?}"),
            }
        }
        assert_eq!(counted.stats().received, 10);

        // whole batches: 8 sealed full, then 3 by age; the clock goes on
        client
            .produce(11, t0.wrapping_add(400))
            .call()
            .await
            .unwrap();
        let batch = tokio::time::timeout(WAIT, counted.recv_batch())
            .await
            .unwrap()
            .unwrap();
        assert_eq!((batch.pos, batch.items.len()), (10, 8));
        assert_eq!(batch.items[0].value, Sample { value: 10 });
        assert_eq!(batch.items[7].t, t_of(t0, 17));
        // a batch taken in part: the rest of it
        assert!(matches!(
            next(&mut counted).await,
            StreamEvent::Item { pos: 18, .. }
        ));
        let batch = tokio::time::timeout(WAIT, counted.recv_batch())
            .await
            .unwrap()
            .unwrap();
        assert_eq!((batch.pos, batch.items.len()), (19, 2));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reliable_result_and_items() {
        let device = start("streams_batched/reliable");
        let mut client = connect(&device).await;
        let r = client.reliable(0).call().await.unwrap();
        assert!(matches!(r, Err(SampleError::ZeroGain)), "{:?}", r.err());

        let ((), mut reliable) = client
            .reliable(3)
            .max_age(Duration::from_millis(5))
            .call()
            .await
            .unwrap()
            .unwrap();
        // in one request, with no event loop turn in between: the first batch of 8 is sealed, the second can't be
        // (the first isn't encoded yet) and is lost, the last 4 go out once they are 5 ms old
        client.produce(20, 0).call().await.unwrap();
        let mut got = vec![];
        while got.len() < 13 {
            got.push(next(&mut reliable).await);
        }
        for (i, ev) in got.into_iter().enumerate() {
            let k = if i < 8 { i as u64 } else { i as u64 + 7 };
            match ev {
                StreamEvent::Gap {
                    at, units, cause, ..
                } if i == 8 => assert_eq!((at, units, cause), (8, 8, Cause::Device)),
                StreamEvent::Item {
                    pos,
                    units,
                    t,
                    value,
                    ..
                } => {
                    assert_eq!((pos, units), (k, 1));
                    assert_eq!(t, t_of(0, k));
                    assert_eq!(
                        value,
                        Sample {
                            value: 3 * k as i32
                        }
                    );
                }
                ev => panic!("{i}: {ev:?}"),
            }
        }
        let status = client.reliable_status().read().await.unwrap();
        assert!(status.open);
        assert_eq!(status.next.0, 20);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn plain_values_and_batches() {
        let device = start("streams_batched/plain");
        let mut client = connect(&device).await;
        let mut plain = client.plain().call().await.unwrap();
        client.produce(3, 500).call().await.unwrap();
        client.flush().call().await.unwrap();
        for k in 0..3 {
            let v = tokio::time::timeout(WAIT, plain.recv())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(v, Sample { value: k });
        }
        client.produce(2, 900).call().await.unwrap();
        client.flush().call().await.unwrap();
        let batch = tokio::time::timeout(WAIT, plain.recv_batch())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[1].t, t_of(900, 1));
        assert_eq!(batch[1].value, Sample { value: 4 });
    }

    /// `batched(untimed)`: items without a time column.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn untimed_frames() {
        let device = start("streams_batched/untimed");
        let mut client = connect(&device).await;
        let mut frames = client.frames().call().await.unwrap();
        client.produce(4, 0).call().await.unwrap();
        client.flush().call().await.unwrap();
        for k in 0..4u32 {
            match next(&mut frames).await {
                StreamEvent::Item { pos, t, value, .. } => {
                    assert_eq!((pos, t), (k as u64, None));
                    assert_eq!(
                        value,
                        Frame {
                            number: k,
                            ok: k % 3 != 0
                        }
                    );
                }
                ev => panic!("{ev:?}"),
            }
        }
    }
}
