//! `ww_log` end to end: a device with `ww_defmt_logger` and a text log on `ww_device::Server` through
//! `tests_common::start_device_async`, the generated client through the real host event loop, defmt frames decoded
//! with `defmt-decoder`.
//!
//! The device writes its frames the way `defmt` macros do (string index, timestamp, arguments) through the logger's
//! `acquire` / `write` / `release`, with indices of a table built here: a host ELF can't carry a real `.defmt`
//! table without the firmware's linker script. The real macros through `global_logger!` are tested in
//! `ww_defmt_logger`, the ELF on hardware (`examples_mcu/rp2`).

#[cfg(test)]
mod tests {
    use std::time::Duration;
    use tests_common::TestDevice;
    use wire_weaver::prelude::*;
    use wire_weaver_client::{Cause, StreamEvent};
    use ww_defmt_logger::DefmtLogger;
    use ww_log::{DefmtEncoding, LevelFilter, Severity};
    use ww_stream::StreamCursor;
    use ww_stream::store::{FullPolicy, RamRing};
    use ww_stream::tx::Reliable;

    /// String indices of the device's "format strings", as `defmt` would intern them.
    const SAMPLE: u16 = 1;
    const BURST: u16 = 2;
    /// What the device puts into `DefmtInfo::build_id`.
    const BUILD_ID: [u8; 20] = [0xb1; 20];

    /// The table the host would read from the firmware's ELF.
    fn table(encoding: DefmtEncoding) -> defmt_decoder::Table {
        let encoding = match encoding {
            DefmtEncoding::Raw => "Raw",
            DefmtEncoding::Rzcobs => "Rzcobs",
            DefmtEncoding::Unknown(d) => panic!("unknown defmt encoding {d}"),
        };
        let entry = |tag: &str, string: &str| serde_json::json!({ "string": { "tag": tag, "string": string }, "raw_symbol": "" });
        serde_json::from_value(serde_json::json!({
            "timestamp": entry("Timestamp", "{=u32}"),
            "entries": {
                SAMPLE.to_string(): entry("Info", "sample {=u32}"),
                BURST.to_string(): entry("Warn", "burst {=u32}"),
            },
            "bitflags": {},
            "encoding": encoding,
        }))
        .expect("defmt_decoder::Table from JSON")
    }

    mod server {
        use super::*;
        use tests_common::TestProcessEventsAsync;

        pub struct Server {
            /// The firmware would have it as its `#[defmt::global_logger]` (`ww_defmt_logger::global_logger!`)
            pub logger: Box<DefmtLogger<512>>,
            pub defmt_frames_stream: Reliable<RamRing<8192>>,
            pub text_records_stream: Reliable<RamRing<8192>>,
            pub stream_sessions: u32,
            pub level: LevelFilter,
            pub buf: [u8; 512],
        }

        impl Server {
            /// Boots: both logs start their sessions, so what is logged before a host connects is kept.
            pub async fn boot() -> Self {
                let mut server = Server {
                    logger: Box::new(DefmtLogger::new()),
                    defmt_frames_stream: Reliable::new(
                        RamRing::new().with_policy(FullPolicy::DropOldest),
                    ),
                    text_records_stream: Reliable::new(RamRing::new()),
                    stream_sessions: 0,
                    level: LevelFilter::Info,
                    buf: [0; 512],
                };
                let session = server.new_stream_session();
                server
                    .defmt_frames_stream
                    .start_session(session)
                    .await
                    .unwrap();
                let session = server.new_stream_session();
                server
                    .text_records_stream
                    .start_session(session)
                    .await
                    .unwrap();
                server
            }

            /// One frame as `defmt::info!("sample {=u32}", value)` writes it, with a timestamp.
            pub fn log_frame(&self, index: u16, value: u32) {
                self.logger.acquire();
                // safety: between acquire and release
                unsafe {
                    self.logger.write(&index.to_le_bytes());
                    self.logger.write(&(value * 10).to_le_bytes());
                    self.logger.write(&value.to_le_bytes());
                    self.logger.release();
                }
            }

            pub async fn pump(&mut self) {
                self.logger
                    .pump(&mut self.defmt_frames_stream)
                    .await
                    .unwrap();
            }

            async fn log_samples(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                first: u32,
                count: u32,
            ) -> RpcResult<()> {
                for value in first..first + count {
                    self.log_frame(SAMPLE, value);
                    self.pump().await;
                }
                RpcResult::Ready(())
            }

            async fn burst(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                first: u32,
                count: u32,
            ) -> RpcResult<()> {
                for value in first..first + count {
                    self.log_frame(BURST, value);
                }
                self.pump().await;
                RpcResult::Ready(())
            }

            async fn log_text(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                severity: Severity,
                message: &'_ str,
            ) -> RpcResult<()> {
                if self.level.allows(severity) {
                    let cut = message.len().min(16);
                    let record = ww_log::LogRecord {
                        severity,
                        target: MaxLen::new_unchecked("log::tests"),
                        message: MaxLen::new_unchecked(&message[..cut]),
                        truncated: cut < message.len(),
                    };
                    api_impl::stream_data_ser()
                        .text()
                        .records_push(
                            &mut self.text_records_stream,
                            &record,
                            1,
                            None,
                            &mut self.buf,
                        )
                        .await
                        .unwrap();
                }
                RpcResult::Ready(())
            }

            // TODO(API-36): consts served from a value, without a handler named after the const
            #[allow(non_snake_case)]
            async fn get_defmt_DEFMT(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
            ) -> GetResult<ww_log::DefmtInfo<'_>, ()> {
                GetResult::Value(ww_log::DefmtInfo {
                    encoding: ww_defmt_logger::encoding(),
                    build_id: MaxLen::new_unchecked(RefVec::new_bytes(&BUILD_ID)),
                })
            }

            async fn defmt_frames(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
            ) -> RpcResult<Result<(), ww_log::LogError<'_>>> {
                RpcResult::Ready(Ok(()))
            }

            async fn close_defmt_frames(&mut self, _cx: &mut Context<'_, impl EventOut>) {}

            async fn stream_defmt_frames(&mut self) -> &mut Reliable<RamRing<8192>> {
                &mut self.defmt_frames_stream
            }

            async fn text_records(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
            ) -> RpcResult<Result<(), ww_log::LogError<'_>>> {
                RpcResult::Ready(Ok(()))
            }

            async fn close_text_records(&mut self, _cx: &mut Context<'_, impl EventOut>) {}

            async fn stream_text_records(&mut self) -> &mut Reliable<RamRing<8192>> {
                &mut self.text_records_stream
            }

            async fn set_text_level(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                value: LevelFilter,
            ) -> SetResult<ww_log::LogError<'_>> {
                self.level = value;
                SetResult::Set
            }

            async fn get_text_level(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
            ) -> GetResult<LevelFilter, ww_log::LogError<'_>> {
                GetResult::Value(self.level)
            }

            fn new_stream_session(&mut self) -> u32 {
                self.stream_sessions = self.stream_sessions.wrapping_add(1);
                self.stream_sessions
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                log_defmt_api :: LogTest for super::Server,
                server = true, no_alloc = true, use_async = true,
                method_model = "_=immediate",
                property_model = "_=get_set",
                // debug_to_file = "../../target/tests_log_defmt_server.rs"
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
                // the event loop: frames from the ring into the store, then the stores out
                self.pump().await;
                let mut buf = [0u8; 1024];
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
                log_defmt_api :: LogTest for super::Client,
                client = "std_client",
                // debug_to_file = "../../target/tests_log_defmt_client.rs"
            );
        }
    }

    use client::Client;

    fn start(path: &str, server: server::Server) -> TestDevice {
        let _ = tracing_subscriber::fmt::try_init();
        tests_common::start_device_async(
            path,
            server,
            log_defmt_api::LOG_TEST_FULL_GID,
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

    /// Decodes the frames of the next items until `want` messages (and gaps) arrived: `"Info sample 7 @70"` per
    /// frame, `"lost N"` per gap.
    async fn decode(
        frames: &mut wire_weaver_client::PositionedStream<ww_log::DefmtFramesOwned>,
        decoder: &mut dyn defmt_decoder::StreamDecoder,
        want: usize,
    ) -> Vec<String> {
        let mut out = Vec::new();
        while out.len() < want {
            match next(frames).await {
                StreamEvent::Item { units, value, .. } => {
                    decoder.received(&value.bytes.into_inner());
                    let mut decoded = 0;
                    loop {
                        match decoder.decode() {
                            Ok(frame) => {
                                out.push(format!(
                                    "{} {} @{}",
                                    frame.level().map(|l| l.as_str()).unwrap_or("-"),
                                    frame.display_message(),
                                    frame.display_timestamp().unwrap()
                                ));
                                decoded += 1;
                            }
                            Err(defmt_decoder::DecodeError::UnexpectedEof) => break,
                            Err(e) => panic!("{e:?}"),
                        }
                    }
                    assert_eq!(decoded, units, "an item holds `units` whole frames");
                }
                StreamEvent::Gap { units, cause, .. } => {
                    assert_eq!(cause, Cause::Device);
                    out.push(format!("lost {units}"));
                }
                StreamEvent::Resumed { .. } | StreamEvent::BackfillDone => {}
                ev => panic!("{ev:?}"),
            }
        }
        out
    }

    fn samples(range: std::ops::Range<u32>) -> Vec<String> {
        range
            .map(|v| format!("info sample {v} @{}", v * 10))
            .collect()
    }

    /// Logs from boot, before any host: kept in the store and delivered when the host resumes the session the
    /// device started, decoded with the table of the build id the device reports. Then live logs.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn logs_from_before_the_host_connects() {
        let mut server = server::Server::boot().await;
        for value in 0..20 {
            server.log_frame(SAMPLE, value);
        }
        server.pump().await;
        let device = start("log_defmt/before_connect", server);
        let mut client = connect(&device).await;

        let info = client.defmt().read_DEFMT().read().await.unwrap();
        assert_eq!(info.build_id.into_inner(), BUILD_ID);
        let table = table(info.encoding);
        let mut decoder = table.new_stream_decoder();

        let status = client.defmt().frames_status().read().await.unwrap();
        assert_eq!(status.next, UVlq64(20));
        let cursor = StreamCursor {
            session: status
                .session
                .expect("the device started a session at boot"),
            pos: UVlq64(0),
        };
        let mut frames = client.defmt().frames_resume(cursor).call().await.unwrap();
        let mut got = decode(&mut frames, decoder.as_mut(), 20).await;
        got.sort(); // backfill and live runs may interleave
        let mut expected = samples(0..20);
        expected.sort();
        assert_eq!(got, expected);

        client.log_samples(20, 5).call().await.unwrap();
        assert_eq!(
            decode(&mut frames, decoder.as_mut(), 5).await,
            samples(20..25)
        );
    }

    /// More frames at once than the logger's ring holds: the ones that didn't fit arrive as one gap of exactly
    /// their number, where they were dropped, and the decoder goes on with the next frame.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ring_overflow_is_exact() {
        let device = start("log_defmt/overflow", server::Server::boot().await);
        let mut client = connect(&device).await;
        let info = client.defmt().read_DEFMT().read().await.unwrap();
        let table = table(info.encoding);
        let mut decoder = table.new_stream_decoder();
        let mut frames = client.defmt().frames().call().await.unwrap().unwrap().1;

        client.log_samples(0, 3).call().await.unwrap();
        // a frame takes 6 bytes in the ring besides its own ~10: 512 bytes hold about 30 of 100
        client.burst(100, 100).call().await.unwrap();
        client.log_samples(3, 2).call().await.unwrap();

        let got = decode(&mut frames, decoder.as_mut(), 3).await;
        assert_eq!(got, samples(0..3));
        let mut kept = 0;
        let mut lost = None;
        while lost.is_none() {
            for msg in decode(&mut frames, decoder.as_mut(), 1).await {
                match msg.strip_prefix("lost ") {
                    Some(n) => lost = Some(n.parse::<u32>().unwrap()),
                    None => {
                        assert!(lost.is_none(), "{msg} after the gap");
                        assert_eq!(
                            msg,
                            format!("warn burst {} @{}", 100 + kept, (100 + kept) * 10)
                        );
                        kept += 1;
                    }
                }
            }
        }
        let lost = lost.unwrap();
        assert!(kept > 0 && lost > 0, "kept {kept}, lost {lost}");
        assert_eq!(kept + lost, 100);
        assert_eq!(
            decode(&mut frames, decoder.as_mut(), 2).await,
            samples(3..5)
        );
    }

    /// Text records: the level filter, bounded messages marked truncated, records from before the host resumes.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn text_records_and_level() {
        let device = start("log_defmt/text", server::Server::boot().await);
        let mut client = connect(&device).await;
        assert_eq!(
            client.text().read_level().read().await.unwrap(),
            LevelFilter::Info
        );
        client
            .log_text(Severity::Warn, "before".into())
            .call()
            .await
            .unwrap();
        client
            .log_text(Severity::Debug, "filtered".into())
            .call()
            .await
            .unwrap();
        client
            .text()
            .write_level(LevelFilter::Debug)
            .write()
            .await
            .unwrap();
        client
            .log_text(
                Severity::Debug,
                "a debug message longer than 16 bytes".into(),
            )
            .call()
            .await
            .unwrap();

        let status = client.text().records_status().read().await.unwrap();
        let cursor = StreamCursor {
            session: status.session.unwrap(),
            pos: UVlq64(0),
        };
        let mut records = client.text().records_resume(cursor).call().await.unwrap();
        let mut got = Vec::new();
        while got.len() < 2 {
            if let StreamEvent::Item { value, .. } = next(&mut records).await {
                got.push(value);
            }
        }
        got.sort_by_key(|r| r.severity);
        assert_eq!(got[0].severity, Severity::Warn);
        assert_eq!(&*got[0].message, "before");
        assert!(!got[0].truncated);
        assert_eq!(got[1].severity, Severity::Debug);
        assert_eq!(&*got[1].message, "a debug message ");
        assert!(got[1].truncated);
        assert_eq!(&*got[1].target, "log::tests");
    }
}
