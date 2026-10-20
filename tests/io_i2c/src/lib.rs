//! `ww_i2c` end to end: a mock server on `ww_device::Server` through `tests_common::start_device`, the generated
//! client through the real host event loop.

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};
    use std::sync::{Arc, Mutex};
    use tests_common::TestDevice;
    use wire_weaver::prelude::*;
    use ww_i2c::{
        Hertz, I2cAddress, I2cCapabilities, I2cConfig, I2cError, I2cErrorOwned, I2cLinesState,
        I2cMatchByte, I2cMode, I2cOp, I2cReadEnvelope, I2cReadKind, I2cReadKindOwned, I2cRecord,
        I2cRecordKind, I2cTransactionResult, I2cTransactionTimeRange, I2cWatchCapabilities,
        I2cWatchConfig, I2cWatchConfigOwned, I2cWatchEvent, I2cWatchReportOwned, MAX_READ_LEN,
        MAX_SCAN, MAX_WATCHES, Second,
    };
    use ww_si::Prefix;
    use ww_si::ww_numeric::NumericValue;
    use ww_stream::tx::Counted;

    fn secs(v: u32) -> Second {
        Second::from_parts(Prefix::Micro, NumericValue::U32(v))
    }

    fn unsupported() -> I2cError<'static> {
        CommonError::Unsupported.into()
    }

    fn invalid(arg: u8, reason: &'static str) -> I2cError<'static> {
        CommonError::InvalidArgument {
            arg,
            reason: MaxLen::new_unchecked(reason),
        }
        .into()
    }

    fn addr_key(addr: I2cAddress) -> u16 {
        match addr {
            I2cAddress::Seven(a) => a.value() as u16,
            I2cAddress::Ten(a) => a.value(),
        }
    }

    /// State of the simulated bus, shared with the test so it can poke registers and assert writes.
    #[derive(Default)]
    struct Model {
        /// Simulated device registers, keyed by a normalized 7/10-bit address.
        registers: HashMap<u16, Vec<u8>>,
        /// Addresses that never acknowledge.
        nack: HashSet<u16>,
        /// Every `I2c::i2c_write`, in order.
        writes: Vec<(u16, Vec<u8>)>,
        pullups: bool,
        monitor_enabled: bool,
        /// Fake device clock, advanced by one on every bus operation.
        now_us: u32,
    }

    impl Model {
        fn tick(&mut self) -> Second {
            self.now_us += 1;
            secs(self.now_us)
        }

        /// Same read logic a real `I2c::i2c_read` and a watch's autonomous read share: a plain read returns the
        /// register's front bytes, a register read (`RepeatedStart`) returns `len` bytes starting at `offset`
        /// (the first byte the write part names).
        fn read(
            &self,
            addr: I2cAddress,
            offset: usize,
            len: u16,
        ) -> Result<Vec<u8>, I2cError<'static>> {
            let key = addr_key(addr);
            if self.nack.contains(&key) {
                return Err(I2cError::Nack);
            }
            let reg = self.registers.get(&key).cloned().unwrap_or_default();
            let mut out = vec![0u8; len as usize];
            for (i, b) in out.iter_mut().enumerate() {
                *b = reg.get(offset + i).copied().unwrap_or(0);
            }
            Ok(out)
        }
    }

    /// `None` for a read kind of a newer API version, which this mock doesn't support.
    fn offset_of(kind: &I2cReadKind<'_>) -> Option<usize> {
        match kind {
            I2cReadKind::Plain => Some(0),
            I2cReadKind::RepeatedStart { write } => {
                Some(write.as_slice().first().copied().unwrap_or(0) as usize)
            }
            I2cReadKind::Unknown(_) => None,
        }
    }

    fn offset_of_owned(kind: &I2cReadKindOwned) -> usize {
        match kind {
            I2cReadKindOwned::Plain => 0,
            I2cReadKindOwned::RepeatedStart { write } => {
                write.first().copied().unwrap_or(0) as usize
            }
            I2cReadKindOwned::Unknown(_) => unreachable!("rejected by watch_config_to_owned"),
        }
    }

    /// Builds the owned equivalent of a borrowed `I2cReadKind` to store beyond the call that carried it.
    fn read_kind_to_owned(kind: &I2cReadKind<'_>) -> Result<I2cReadKindOwned, I2cError<'static>> {
        Ok(match kind {
            I2cReadKind::Plain => I2cReadKindOwned::Plain,
            I2cReadKind::RepeatedStart { write } => I2cReadKindOwned::RepeatedStart {
                write: MaxLen::new_unchecked(write.as_slice().to_vec()),
            },
            I2cReadKind::Unknown(_) => return Err(unsupported()),
        })
    }

    /// Builds the owned equivalent of a borrowed `I2cWatchReport` to store beyond the call that carried it.
    fn report_to_owned(
        report: &ww_i2c::I2cWatchReport<'_>,
    ) -> Result<I2cWatchReportOwned, I2cError<'static>> {
        Ok(match report {
            ww_i2c::I2cWatchReport::Always => I2cWatchReportOwned::Always,
            ww_i2c::I2cWatchReport::Changed { mask } => I2cWatchReportOwned::Changed {
                mask: MaxLen::new_unchecked(mask.as_slice().to_vec()),
            },
            ww_i2c::I2cWatchReport::Matches { bytes } => I2cWatchReportOwned::Matches {
                bytes: MaxLen::new_unchecked(
                    bytes
                        .iter()
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|_| invalid(0, "match rule"))?,
                ),
            },
            ww_i2c::I2cWatchReport::Unknown(_) => return Err(unsupported()),
        })
    }

    /// `Unsupported` for a read kind or report of a newer API version.
    fn watch_config_to_owned(
        config: &I2cWatchConfig<'_>,
    ) -> Result<I2cWatchConfigOwned, I2cError<'static>> {
        Ok(I2cWatchConfigOwned {
            addr: config.addr,
            kind: read_kind_to_owned(&config.kind)?,
            len: config.len,
            period: config.period,
            report: report_to_owned(&config.report)?,
        })
    }

    /// One autonomous watch, see `ww_i2c::I2cWatch`.
    struct WatchSlot {
        config: I2cWatchConfigOwned,
        /// Last masked bytes reported under `I2cWatchReport::Changed`.
        last_reported: Option<Vec<u8>>,
        /// Last match state reported under `I2cWatchReport::Matches`.
        last_match: Option<bool>,
        /// Whether the previous tick reported (and is still reporting) an error.
        in_error: bool,
    }

    mod mock_server {
        use super::*;
        use tests_common::TestProcessEvents;
        use ww_i2c::MAX_WATCH_LEN;

        pub struct Server {
            pub model: Arc<Mutex<Model>>,
            pub watches: Vec<Option<WatchSlot>>,
            read_scratch: [u8; MAX_READ_LEN],
            scan_scratch: [u8; MAX_SCAN],
            watch_list_scratch: [u32; MAX_WATCHES],
            records: Counted,
            changes: Counted,
            stream_sessions: u32,
        }

        impl Server {
            pub fn new(model: Arc<Mutex<Model>>) -> Self {
                Server {
                    model,
                    watches: (0..MAX_WATCHES).map(|_| None).collect(),
                    read_scratch: [0u8; MAX_READ_LEN],
                    scan_scratch: [0u8; MAX_SCAN],
                    watch_list_scratch: [0u32; MAX_WATCHES],
                    records: Counted::new(),
                    changes: Counted::new(),
                    stream_sessions: 0,
                }
            }

            fn i2c_i2c_capabilities(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<I2cCapabilities<'_>> {
                Ready(I2cCapabilities {
                    io_levels: MaxLen::new(RefVec::Slice { slice: &[] }).unwrap(),
                    io_level_min: None,
                    io_level_max: None,
                    max_write_len: ww_i2c::MAX_WRITE_LEN as u32,
                    max_read_len: MAX_READ_LEN as u32,
                    min_freq: Hertz::kilo(1u16),
                    max_freq: Hertz::kilo(400u16),
                    driver_name: MaxLen::new_unchecked("mock"),
                    master_supported: true,
                    monitor_supported: true,
                    ten_bit_addresses: true,
                    pullups: true,
                    bus_recovery: true,
                })
            }

            fn i2c_i2c_configure(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                config: I2cConfig,
            ) -> RpcResult<Result<I2cConfig, I2cError<'_>>> {
                // a fixed 3.3 V bus: no level control, so the answer's IO level is `None`
                if config.io_level.is_some() {
                    return Ready(Err(unsupported()));
                }
                let max = Hertz::kilo(400u16);
                let mode = match config.mode {
                    I2cMode::Master { speed, monitor } if speed > max => I2cMode::Master {
                        speed: max,
                        monitor,
                    },
                    I2cMode::Unknown(_) => {
                        return Ready(Err(unsupported()));
                    }
                    mode => mode,
                };
                Ready(Ok(I2cConfig {
                    mode,
                    io_level: None,
                }))
            }

            fn i2c_i2c_write(
                &mut self,
                cx: &mut Context<'_, impl BlockingEventOut>,
                addr: I2cAddress,
                data: MaxLen<RefVec<'_, u8>, { ww_i2c::MAX_WRITE_LEN }>,
                _delay_after: Option<Second>,
            ) -> RpcResult<Result<I2cTransactionTimeRange, I2cError<'_>>> {
                let mut m = self.model.lock().unwrap();
                let key = addr_key(addr);
                if m.nack.contains(&key) {
                    return Ready(Err(I2cError::Nack));
                }
                let bytes = data.as_slice().to_vec();
                let start = m.tick();
                m.registers.insert(key, bytes.clone());
                m.writes.push((key, bytes.clone()));
                let end = m.tick();
                if m.monitor_enabled {
                    let record = I2cRecord {
                        end,
                        addr,
                        read: false,
                        kind: I2cRecordKind::Complete,
                        nack_at: None,
                        bytes: MaxLen::new(RefVec::new_bytes(&bytes)).unwrap(),
                    };
                    api_impl::stream_data_ser()
                        .i2c_monitor()
                        .records_send_blocking(&mut self.records, &record, 1, Some(start), cx)
                        .ok();
                }
                Ready(Ok(I2cTransactionTimeRange { start, end }))
            }

            fn i2c_i2c_read(
                &mut self,
                cx: &mut Context<'_, impl BlockingEventOut>,
                addr: I2cAddress,
                kind: I2cReadKind<'_>,
                len: u16,
                _delay_after: Option<Second>,
            ) -> RpcResult<Result<I2cReadEnvelope<'_>, I2cError<'_>>> {
                if len == 0 {
                    return Ready(Err(invalid(2, "len is 0")));
                }
                let Some(offset) = offset_of(&kind) else {
                    return Ready(Err(unsupported()));
                };
                let mut m = self.model.lock().unwrap();
                let start = m.tick();
                let result = m.read(addr, offset, len);
                let end = m.tick();
                let data = match result {
                    Ok(data) => data,
                    Err(e) => return Ready(Err(e)),
                };
                if m.monitor_enabled {
                    let record = I2cRecord {
                        end,
                        addr,
                        read: true,
                        kind: I2cRecordKind::Complete,
                        nack_at: None,
                        bytes: MaxLen::new(RefVec::new_bytes(&data)).unwrap(),
                    };
                    api_impl::stream_data_ser()
                        .i2c_monitor()
                        .records_send_blocking(&mut self.records, &record, 1, Some(start), cx)
                        .ok();
                }
                self.read_scratch[..data.len()].copy_from_slice(&data);
                let len = data.len();
                Ready(Ok(I2cReadEnvelope {
                    duration: I2cTransactionTimeRange { start, end },
                    data: MaxLen::new(RefVec::new_bytes(&self.read_scratch[..len])).unwrap(),
                }))
            }

            fn i2c_i2c_scan(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<Result<MaxLen<RefVec<'_, u8>, MAX_SCAN>, I2cError<'_>>> {
                let m = self.model.lock().unwrap();
                let mut found: Vec<u8> = m
                    .registers
                    .keys()
                    .copied()
                    .filter(|k| *k <= 0x7F && !m.nack.contains(k))
                    .filter(|k| !(0x00..=0x07).contains(k) && !(0x78..=0x7F).contains(k))
                    .map(|k| k as u8)
                    .collect();
                found.sort_unstable();
                self.scan_scratch[..found.len()].copy_from_slice(&found);
                let len = found.len();
                Ready(Ok(MaxLen::new(RefVec::new_bytes(
                    &self.scan_scratch[..len],
                ))
                .unwrap()))
            }

            fn i2c_i2c_transaction(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                addr: I2cAddress,
                ops: MaxLen<RefVec<'_, I2cOp<'_>>, { ww_i2c::MAX_OPS }>,
            ) -> RpcResult<Result<I2cTransactionResult<'_>, I2cError<'_>>> {
                let mut m = self.model.lock().unwrap();
                let start = m.tick();
                let key = addr_key(addr);
                if m.nack.contains(&key) {
                    return Ready(Err(I2cError::Nack));
                }
                let mut read_out = Vec::new();
                let mut reg = m.registers.get(&key).cloned().unwrap_or_default();
                for op in ops.iter() {
                    let Ok(op) = op else {
                        return Ready(Err(invalid(1, "op")));
                    };
                    match op {
                        I2cOp::Write(data) => reg = data.as_slice().to_vec(),
                        I2cOp::Read(len) => {
                            let mut chunk = vec![0u8; len as usize];
                            for (i, b) in chunk.iter_mut().enumerate() {
                                *b = reg.get(i).copied().unwrap_or(0);
                            }
                            read_out.extend_from_slice(&chunk);
                        }
                        // an op of a newer API version
                        I2cOp::Unknown(_) => return Ready(Err(unsupported())),
                    }
                }
                m.registers.insert(key, reg);
                let end = m.tick();
                self.read_scratch[..read_out.len()].copy_from_slice(&read_out);
                let len = read_out.len();
                Ready(Ok(I2cTransactionResult {
                    duration: I2cTransactionTimeRange { start, end },
                    read: MaxLen::new(RefVec::new_bytes(&self.read_scratch[..len])).unwrap(),
                }))
            }

            fn i2c_i2c_lines_state(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<I2cLinesState> {
                Ready(I2cLinesState {
                    is_scl_high: true,
                    is_sda_high: true,
                    scl: None,
                    sda: None,
                })
            }

            fn i2c_i2c_wiggle_clock(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _count: u8,
            ) -> RpcResult<Result<I2cLinesState, I2cError<'_>>> {
                Ready(Ok(I2cLinesState {
                    is_scl_high: true,
                    is_sda_high: true,
                    scl: None,
                    sda: None,
                }))
            }

            fn i2c_i2c_set_pullups(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                enabled: bool,
            ) -> RpcResult<Result<(), I2cError<'_>>> {
                self.model.lock().unwrap().pullups = enabled;
                Ready(Ok(()))
            }

            fn i2c_i2c_pullups(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<bool> {
                Ready(self.model.lock().unwrap().pullups)
            }

            fn i2c_watch_changes(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<()> {
                Ready(())
            }

            fn close_i2c_watch_changes(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {}

            fn stream_i2c_watch_changes(&mut self) -> &mut Counted {
                &mut self.changes
            }

            fn i2c_watch_watch_capabilities(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<I2cWatchCapabilities> {
                Ready(I2cWatchCapabilities {
                    max_watches: MAX_WATCHES as u32,
                    max_len: MAX_WATCH_LEN as u16,
                    min_period: secs(0),
                })
            }

            fn i2c_watch_watch_add(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                watch: I2cWatchConfig<'_>,
            ) -> RpcResult<Result<u32, I2cError<'_>>> {
                let config = match watch_config_to_owned(&watch) {
                    Ok(config) => config,
                    Err(e) => return Ready(Err(e)),
                };
                let Some(slot) = self.watches.iter().position(Option::is_none) else {
                    return Ready(Err(I2cError::OutOfWatchSlots));
                };
                self.watches[slot] = Some(WatchSlot {
                    config,
                    last_reported: None,
                    last_match: None,
                    in_error: false,
                });
                Ready(Ok(slot as u32))
            }

            fn i2c_watch_watch_modify(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                slot: u32,
                watch: I2cWatchConfig<'_>,
            ) -> RpcResult<Result<(), I2cError<'_>>> {
                let Some(entry) = self.watches.get_mut(slot as usize) else {
                    return Ready(Err(I2cError::InvalidWatchSlot));
                };
                if entry.is_none() {
                    return Ready(Err(I2cError::InvalidWatchSlot));
                }
                let config = match watch_config_to_owned(&watch) {
                    Ok(config) => config,
                    Err(e) => return Ready(Err(e)),
                };
                *entry = Some(WatchSlot {
                    config,
                    last_reported: None,
                    last_match: None,
                    in_error: false,
                });
                Ready(Ok(()))
            }

            fn i2c_watch_watch_remove(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                slot: u32,
            ) -> RpcResult<Result<(), I2cError<'_>>> {
                let Some(entry) = self.watches.get_mut(slot as usize) else {
                    return Ready(Err(I2cError::InvalidWatchSlot));
                };
                if entry.is_none() {
                    return Ready(Err(I2cError::InvalidWatchSlot));
                }
                *entry = None;
                Ready(Ok(()))
            }

            fn i2c_watch_watch_list(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<MaxLen<RefVec<'_, u32>, MAX_WATCHES>> {
                let list: Vec<u32> = self
                    .watches
                    .iter()
                    .enumerate()
                    .filter_map(|(i, w)| w.as_ref().map(|_| i as u32))
                    .collect();
                self.watch_list_scratch[..list.len()].copy_from_slice(&list);
                let len = list.len();
                Ready(
                    MaxLen::new(RefVec::Slice {
                        slice: &self.watch_list_scratch[..len],
                    })
                    .unwrap(),
                )
            }

            fn i2c_monitor_records(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _io_level: Option<ww_si::Volt>,
            ) -> RpcResult<Result<(), I2cError<'_>>> {
                self.model.lock().unwrap().monitor_enabled = true;
                Ready(Ok(()))
            }

            fn close_i2c_monitor_records(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {
                self.model.lock().unwrap().monitor_enabled = false;
            }

            fn stream_i2c_monitor_records(&mut self) -> &mut Counted {
                &mut self.records
            }

            fn new_stream_session(&mut self) -> u32 {
                self.stream_sessions = self.stream_sessions.wrapping_add(1);
                self.stream_sessions
            }

            /// Test-only: one read cycle for every configured watch, applying `I2cWatchReport` the same way a
            /// real firmware's polling loop would.
            fn tick_watches(
                &mut self,
                cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<u8> {
                let mut sent = 0u8;
                for slot_idx in 0..self.watches.len() {
                    let Some(slot) = self.watches[slot_idx].as_mut() else {
                        continue;
                    };
                    let t = {
                        let mut m = self.model.lock().unwrap();
                        m.tick()
                    };
                    let result = {
                        let m = self.model.lock().unwrap();
                        m.read(
                            slot.config.addr,
                            offset_of_owned(&slot.config.kind),
                            slot.config.len,
                        )
                    };
                    let report = match &result {
                        Err(e) => {
                            let should_report = !slot.in_error;
                            slot.in_error = true;
                            should_report.then_some(Err(e.clone()))
                        }
                        Ok(data) => {
                            // Recovering from an error reports unconditionally, but still updates the Changed /
                            // Matches baseline to this read, so a later unchanged read goes quiet again instead
                            // of being treated as "no previous value" and reported a second time.
                            let was_error = slot.in_error;
                            slot.in_error = false;
                            match &slot.config.report {
                                I2cWatchReportOwned::Always => Some(Ok(data.clone())),
                                I2cWatchReportOwned::Changed { mask } => {
                                    let masked: Vec<u8> = data
                                        .iter()
                                        .zip(mask.iter().chain(std::iter::repeat(&0xFFu8)))
                                        .map(|(d, m)| d & m)
                                        .collect();
                                    let changed = slot.last_reported.as_ref() != Some(&masked);
                                    slot.last_reported = Some(masked);
                                    (was_error || changed).then(|| Ok(data.clone()))
                                }
                                I2cWatchReportOwned::Matches { bytes } => {
                                    let is_match = data.len() == bytes.len()
                                        && data
                                            .iter()
                                            .zip(bytes.iter())
                                            .all(|(d, b)| d & b.mask == b.value);
                                    let changed = slot.last_match != Some(is_match);
                                    slot.last_match = Some(is_match);
                                    (was_error || changed).then(|| Ok(data.clone()))
                                }
                                I2cWatchReportOwned::Unknown(_) => {
                                    unreachable!("rejected by watch_config_to_owned")
                                }
                            }
                        }
                    };
                    if let Some(result) = report {
                        let mut scratch = [0u8; MAX_WATCH_LEN];
                        let result = match result {
                            Ok(data) => {
                                scratch[..data.len()].copy_from_slice(&data);
                                Ok(MaxLen::new(RefVec::new_bytes(&scratch[..data.len()])).unwrap())
                            }
                            Err(e) => Err(e),
                        };
                        let event = I2cWatchEvent {
                            slot: slot_idx as u32,
                            result,
                        };
                        api_impl::stream_data_ser()
                            .i2c_watch()
                            .changes_send_blocking(&mut self.changes, &event, 1, Some(t), cx)
                            .ok();
                        sent += 1;
                    }
                }
                Ready(sent)
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                io_i2c_api :: IoI2cTest for super::Server,
                server = true, no_alloc = true, use_async = false,
                omit = ["i2c.slave"],
                method_model = "_=immediate",
                property_model = "_=get_set",
                // debug_to_file = "../../target/tests_io_i2c_server.rs"
            );
        }

        impl TestProcessEvents for Server {
            type Medium = ();

            fn process_request_bytes<'a>(
                &mut self,
                bytes: &[u8],
                scratch: &'a mut [u8],
                out: &mut impl BlockingEventOut,
                medium: (),
            ) -> Result<&'a [u8], ShrinkWrapError> {
                self.process_request_bytes(bytes, scratch, out, medium)
            }
        }
    }

    mod std_client {
        use wire_weaver_client::{ClientConfig, Commander, WwClient};

        pub struct StdClient {
            pub cmd: Commander,
        }

        impl WwClient for StdClient {
            fn default_config() -> ClientConfig {
                ClientConfig::default()
            }

            fn from_cmd(cmd: Commander) -> Self {
                Self { cmd }
            }
        }

        mod api_client {
            wire_weaver::ww_codegen!(
                io_i2c_api :: IoI2cTest for super::StdClient,
                client = "std_client",
            );
        }
    }

    use std_client::StdClient;

    fn start(path: &str) -> (TestDevice, Arc<Mutex<Model>>) {
        let _ = tracing_subscriber::fmt::try_init();
        let model = Arc::new(Mutex::new(Model::default()));
        let server = mock_server::Server::new(model.clone());
        let device = tests_common::start_device(
            path,
            server,
            io_i2c_api::IO_I_2_C_TEST_FULL_GID,
            mock_server::api_impl::api_hash(),
        );
        (device, model)
    }

    async fn connect(device: &TestDevice) -> StdClient {
        StdClient::config(|c| c.in_process_path(device.path()))
            .connect()
            .await
            .expect("connect")
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn i2c_write_read_roundtrip() {
        let (device, _model) = start("io_i2c/write_read");
        let client = connect(&device).await;

        let addr = I2cAddress::seven(0x50).unwrap();
        client
            .i2c()
            .i2c_write(addr, MaxLen::new(vec![1, 2, 3]).unwrap(), None)
            .call()
            .await
            .unwrap();
        let read = client
            .i2c()
            .i2c_read(addr, I2cReadKindOwned::Plain, 3, None)
            .call()
            .await
            .unwrap();
        assert_eq!(read.data.as_slice(), &[1, 2, 3]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn i2c_configure_answers_what_was_set() {
        let (device, _model) = start("io_i2c/configure");
        let client = connect(&device).await;

        let actual = client
            .i2c()
            .i2c_configure(I2cConfig {
                mode: I2cMode::Master {
                    speed: Hertz::mega(1u8),
                    monitor: false,
                },
                io_level: None,
            })
            .call()
            .await
            .unwrap();
        assert_eq!(
            actual.mode,
            I2cMode::Master {
                speed: Hertz::kilo(400u16),
                monitor: false
            }
        );
        assert_eq!(actual.io_level, None);

        let err = client
            .i2c()
            .i2c_configure(I2cConfig {
                mode: I2cMode::Disabled,
                io_level: Some(ww_si::Volt::milli(1800u16)),
            })
            .call()
            .await
            .unwrap_err();
        assert_eq!(
            err.device(),
            Some(&I2cErrorOwned::Common(CommonErrorOwned::Unsupported))
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn i2c_nack_and_zero_length() {
        let (device, model) = start("io_i2c/nack");
        model.lock().unwrap().nack.insert(0x51);
        let client = connect(&device).await;

        let addr = I2cAddress::seven(0x51).unwrap();
        let err = client
            .i2c()
            .i2c_write(addr, MaxLen::new(vec![1]).unwrap(), None)
            .call()
            .await
            .unwrap_err();
        assert_eq!(err.device(), Some(&I2cErrorOwned::Nack));

        let addr_ok = I2cAddress::seven(0x52).unwrap();
        let err = client
            .i2c()
            .i2c_read(addr_ok, I2cReadKindOwned::Plain, 0, None)
            .call()
            .await
            .unwrap_err();
        assert!(matches!(
            err.device(),
            Some(I2cErrorOwned::Common(CommonErrorOwned::InvalidArgument {
                arg: 2,
                ..
            }))
        ));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn i2c_scan_and_lines() {
        let (device, model) = start("io_i2c/scan");
        model.lock().unwrap().registers.insert(0x50, vec![1]);
        model.lock().unwrap().registers.insert(0x60, vec![2]);
        let client = connect(&device).await;

        let found = client.i2c().i2c_scan().call().await.unwrap();
        assert_eq!(found.as_slice(), &[0x50, 0x60]);

        let lines = client.i2c().i2c_lines_state().call().await.unwrap();
        assert!(lines.is_scl_high && lines.is_sda_high);

        assert!(!client.i2c().i2c_pullups().call().await.unwrap());
        client.i2c().i2c_set_pullups(true).call().await.unwrap();
        assert!(client.i2c().i2c_pullups().call().await.unwrap());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn i2c_monitor_records_writes_and_reads() {
        let (device, _model) = start("io_i2c/monitor");
        let client = connect(&device).await;
        let ((), mut records) = client
            .i2c_monitor()
            .records(None)
            .call()
            .await
            .unwrap()
            .unwrap();

        let addr = I2cAddress::seven(0x50).unwrap();
        client
            .i2c()
            .i2c_write(addr, MaxLen::new(vec![9]).unwrap(), None)
            .call()
            .await
            .unwrap();
        let record = records.recv().await.unwrap();
        assert!(!record.read);
        assert_eq!(record.bytes.as_slice(), &[9]);

        client
            .i2c()
            .i2c_read(addr, I2cReadKindOwned::Plain, 1, None)
            .call()
            .await
            .unwrap();
        let record = records.recv().await.unwrap();
        assert!(record.read);
        assert_eq!(record.bytes.as_slice(), &[9]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn i2c_watch_always_reports_every_read() {
        let (device, model) = start("io_i2c/watch_always");
        model.lock().unwrap().registers.insert(0x50, vec![1]);
        let client = connect(&device).await;
        let mut changes = client.i2c_watch().changes().call().await.unwrap();

        let addr = I2cAddress::seven(0x50).unwrap();
        let slot = client
            .i2c_watch()
            .watch_add(I2cWatchConfigOwned {
                addr,
                kind: I2cReadKindOwned::Plain,
                len: 1,
                period: secs(0),
                report: I2cWatchReportOwned::Always,
            })
            .call()
            .await
            .unwrap();

        for _ in 0..3 {
            assert_eq!(client.tick_watches().call().await.unwrap(), 1);
            let ev = changes.recv().await.unwrap();
            assert_eq!(ev.slot, slot);
            assert_eq!(ev.result.unwrap().as_slice(), &[1]);
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn i2c_watch_changed_reports_only_on_change() {
        let (device, model) = start("io_i2c/watch_changed");
        model.lock().unwrap().registers.insert(0x50, vec![0]);
        let client = connect(&device).await;
        let mut changes = client.i2c_watch().changes().call().await.unwrap();

        let addr = I2cAddress::seven(0x50).unwrap();
        client
            .i2c_watch()
            .watch_add(I2cWatchConfigOwned {
                addr,
                kind: I2cReadKindOwned::Plain,
                len: 1,
                period: secs(0),
                report: I2cWatchReportOwned::Changed {
                    mask: MaxLen::new(vec![0xFF]).unwrap(),
                },
            })
            .call()
            .await
            .unwrap();

        // first tick always reports (no previous value)
        assert_eq!(client.tick_watches().call().await.unwrap(), 1);
        let ev = changes.recv().await.unwrap();
        assert_eq!(ev.result.unwrap().as_slice(), &[0]);

        // same value again: no report
        assert_eq!(client.tick_watches().call().await.unwrap(), 0);
        assert!(changes.try_recv().unwrap().is_none());

        // value changes: reported
        model.lock().unwrap().registers.insert(0x50, vec![5]);
        assert_eq!(client.tick_watches().call().await.unwrap(), 1);
        let ev = changes.recv().await.unwrap();
        assert_eq!(ev.result.unwrap().as_slice(), &[5]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn i2c_watch_matches_reports_on_transition_only() {
        let (device, model) = start("io_i2c/watch_matches");
        model.lock().unwrap().registers.insert(0x50, vec![0]);
        let client = connect(&device).await;
        let mut changes = client.i2c_watch().changes().call().await.unwrap();

        let addr = I2cAddress::seven(0x50).unwrap();
        client
            .i2c_watch()
            .watch_add(I2cWatchConfigOwned {
                addr,
                kind: I2cReadKindOwned::Plain,
                len: 1,
                period: secs(0),
                report: I2cWatchReportOwned::Matches {
                    bytes: MaxLen::new(vec![I2cMatchByte {
                        mask: 0x01,
                        value: 0x01,
                    }])
                    .unwrap(),
                },
            })
            .call()
            .await
            .unwrap();

        // first tick always reports (no previous match state to compare against)
        assert_eq!(client.tick_watches().call().await.unwrap(), 1);
        let ev = changes.recv().await.unwrap();
        assert_eq!(ev.result.unwrap().as_slice(), &[0]);

        // still not matching: no report
        assert_eq!(client.tick_watches().call().await.unwrap(), 0);
        assert!(changes.try_recv().unwrap().is_none());

        // alert bit set: starts matching, reported
        model.lock().unwrap().registers.insert(0x50, vec![0x01]);
        assert_eq!(client.tick_watches().call().await.unwrap(), 1);
        let ev = changes.recv().await.unwrap();
        assert_eq!(ev.result.unwrap().as_slice(), &[0x01]);

        // still matching: no report
        assert_eq!(client.tick_watches().call().await.unwrap(), 0);

        // alert bit cleared: stops matching, reported
        model.lock().unwrap().registers.insert(0x50, vec![0x00]);
        assert_eq!(client.tick_watches().call().await.unwrap(), 1);
        let ev = changes.recv().await.unwrap();
        assert_eq!(ev.result.unwrap().as_slice(), &[0x00]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn i2c_watch_error_reported_once_then_recovery_always_reports() {
        let (device, model) = start("io_i2c/watch_error");
        model.lock().unwrap().registers.insert(0x50, vec![1]);
        let client = connect(&device).await;
        let mut changes = client.i2c_watch().changes().call().await.unwrap();

        let addr = I2cAddress::seven(0x50).unwrap();
        client
            .i2c_watch()
            .watch_add(I2cWatchConfigOwned {
                addr,
                kind: I2cReadKindOwned::Plain,
                len: 1,
                period: secs(0),
                report: I2cWatchReportOwned::Changed {
                    mask: MaxLen::new(vec![0xFF]).unwrap(),
                },
            })
            .call()
            .await
            .unwrap();

        // baseline read, reported (first ever)
        assert_eq!(client.tick_watches().call().await.unwrap(), 1);
        changes.recv().await.unwrap();

        // the address starts nacking: the first failing tick is reported
        model.lock().unwrap().nack.insert(0x50);
        assert_eq!(client.tick_watches().call().await.unwrap(), 1);
        let ev = changes.recv().await.unwrap();
        assert_eq!(ev.result.unwrap_err(), I2cErrorOwned::Nack);

        // it keeps failing: no further reports
        assert_eq!(client.tick_watches().call().await.unwrap(), 0);
        assert!(changes.try_recv().unwrap().is_none());

        // it recovers with the very same bytes as before the error: still reported once, unconditionally
        model.lock().unwrap().nack.remove(&0x50);
        assert_eq!(client.tick_watches().call().await.unwrap(), 1);
        let ev = changes.recv().await.unwrap();
        assert_eq!(ev.result.unwrap().as_slice(), &[1]);

        // back to normal Changed semantics: the same value again is not reported
        assert_eq!(client.tick_watches().call().await.unwrap(), 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn i2c_watch_modify_remove_and_list() {
        let (device, model) = start("io_i2c/watch_list");
        model.lock().unwrap().registers.insert(0x50, vec![1]);
        let client = connect(&device).await;

        let addr = I2cAddress::seven(0x50).unwrap();
        let cfg = I2cWatchConfigOwned {
            addr,
            kind: I2cReadKindOwned::Plain,
            len: 1,
            period: secs(0),
            report: I2cWatchReportOwned::Always,
        };
        let slot0 = client
            .i2c_watch()
            .watch_add(cfg.clone())
            .call()
            .await
            .unwrap();
        let slot1 = client
            .i2c_watch()
            .watch_add(cfg.clone())
            .call()
            .await
            .unwrap();
        assert_eq!(
            client
                .i2c_watch()
                .watch_list()
                .call()
                .await
                .unwrap()
                .as_slice(),
            &[slot0, slot1]
        );

        client
            .i2c_watch()
            .watch_modify(slot0, cfg.clone())
            .call()
            .await
            .unwrap();
        client.i2c_watch().watch_remove(slot1).call().await.unwrap();
        assert_eq!(
            client
                .i2c_watch()
                .watch_list()
                .call()
                .await
                .unwrap()
                .as_slice(),
            &[slot0]
        );

        let err = client
            .i2c_watch()
            .watch_remove(slot1)
            .call()
            .await
            .unwrap_err();
        assert_eq!(err.device(), Some(&I2cErrorOwned::InvalidWatchSlot));
    }

    // ---- slave mode: a second mock server built on `ww_bus_slave::I2cSlave`, driven by `SlaveBusOp` instead of a
    // real bus, with a fake clock advanced by `SlaveBusOp::Advance` (STD-35, SRV-26) ----

    mod slave_mock {
        use super::*;
        use io_i2c_api::{SlaveBusOp, SlaveBusResult};
        use tests_common::TestProcessEvents;
        use ww_bus_slave::{I2cSlave, ReadByte};
        use ww_client_server::StreamSideband;
        use ww_i2c::{I2cSlaveCapabilities, I2cSlaveConfig};
        use ww_stream::sink::SinkCredit;

        /// Same sizes as `ww_bus_slave`'s own host tests: a 64-byte map, 3 answers of up to 8 bytes, 8 events of up
        /// to 16 bytes.
        pub const MAP: usize = 64;
        const STAGE: usize = 32;
        pub const Q: usize = 3;
        const A: usize = 8;
        const EV: usize = 8;
        const REC: usize = 16;

        fn wrong_state() -> I2cError<'static> {
            CommonError::WrongState {
                reason: MaxLen::new_unchecked("slave-only mock"),
            }
            .into()
        }

        pub struct Server {
            engine: I2cSlave<MAP, STAGE, Q, A, EV, REC>,
            now_us: u32,
            events: Counted,
            slave_answers_credit: SinkCredit,
            map_scratch: [u8; MAX_READ_LEN],
            stream_sessions: u32,
        }

        impl Server {
            pub fn new() -> Self {
                Server {
                    engine: I2cSlave::new(),
                    now_us: 0,
                    events: Counted::new(),
                    slave_answers_credit: SinkCredit::new(1024),
                    map_scratch: [0u8; MAX_READ_LEN],
                    stream_sessions: 0,
                }
            }

            /// Drains the engine's events (and drop count) onto `slave_events`, see `IoI2cSlaveTest::slave_pump`.
            fn pump(&mut self, out: &mut impl BlockingEventOut) {
                // drain what the ring kept first: a drop (reported after) always follows the oldest kept record,
                // the ring only drops once it is full of them.
                while let Some((t, ev)) = self.engine.peek_event() {
                    let _ = api_impl::stream_data_ser()
                        .i2c()
                        .slave_events_send_blocking(&mut self.events, &ev, 1, Some(t), out);
                    self.engine.pop_event();
                }
                let dropped = self.engine.take_dropped();
                if dropped > 0 {
                    self.events.lost(dropped as u64);
                }
            }

            fn new_stream_session(&mut self) -> u32 {
                self.stream_sessions = self.stream_sessions.wrapping_add(1);
                self.stream_sessions
            }

            fn i2c_i2c_capabilities(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<I2cCapabilities<'_>> {
                Ready(I2cCapabilities {
                    io_levels: MaxLen::new(RefVec::Slice { slice: &[] }).unwrap(),
                    io_level_min: None,
                    io_level_max: None,
                    max_write_len: ww_i2c::MAX_WRITE_LEN as u32,
                    max_read_len: MAX_READ_LEN as u32,
                    min_freq: Hertz::kilo(1u16),
                    max_freq: Hertz::kilo(400u16),
                    driver_name: MaxLen::new_unchecked("mock-slave"),
                    master_supported: false,
                    monitor_supported: false,
                    ten_bit_addresses: false,
                    pullups: false,
                    bus_recovery: false,
                })
            }

            fn i2c_i2c_configure(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                config: I2cConfig,
            ) -> RpcResult<Result<I2cConfig, I2cError<'_>>> {
                match config.mode {
                    I2cMode::Disabled => Ready(Ok(I2cConfig {
                        mode: I2cMode::Disabled,
                        io_level: None,
                    })),
                    _ => Ready(Err(unsupported())),
                }
            }

            fn i2c_i2c_write(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _addr: I2cAddress,
                _data: MaxLen<RefVec<'_, u8>, { ww_i2c::MAX_WRITE_LEN }>,
                _delay_after: Option<Second>,
            ) -> RpcResult<Result<I2cTransactionTimeRange, I2cError<'_>>> {
                Ready(Err(wrong_state()))
            }

            fn i2c_i2c_read(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _addr: I2cAddress,
                _kind: I2cReadKind<'_>,
                _len: u16,
                _delay_after: Option<Second>,
            ) -> RpcResult<Result<I2cReadEnvelope<'_>, I2cError<'_>>> {
                Ready(Err(wrong_state()))
            }

            fn i2c_i2c_scan(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<Result<MaxLen<RefVec<'_, u8>, MAX_SCAN>, I2cError<'_>>> {
                Ready(Err(wrong_state()))
            }

            fn i2c_i2c_transaction(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _addr: I2cAddress,
                _ops: MaxLen<RefVec<'_, I2cOp<'_>>, { ww_i2c::MAX_OPS }>,
            ) -> RpcResult<Result<I2cTransactionResult<'_>, I2cError<'_>>> {
                Ready(Err(wrong_state()))
            }

            fn i2c_i2c_lines_state(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<I2cLinesState> {
                Ready(I2cLinesState {
                    is_scl_high: true,
                    is_sda_high: true,
                    scl: None,
                    sda: None,
                })
            }

            fn i2c_i2c_wiggle_clock(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _count: u8,
            ) -> RpcResult<Result<I2cLinesState, I2cError<'_>>> {
                Ready(Err(unsupported()))
            }

            fn i2c_i2c_set_pullups(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _enabled: bool,
            ) -> RpcResult<Result<(), I2cError<'_>>> {
                Ready(Err(unsupported()))
            }

            fn i2c_i2c_pullups(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<bool> {
                Ready(false)
            }

            fn i2c_slave_capabilities(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<I2cSlaveCapabilities> {
                Ready(I2cSlaveCapabilities {
                    map_len: MAP as u32,
                    max_pointer_bytes: 4,
                    queue_len: Q as u32,
                    max_stretch: Some(secs(1_000_000)),
                    nack_when_empty: true,
                    ten_bit_address: false,
                    max_freq: Hertz::kilo(400u16),
                })
            }

            fn i2c_slave_events(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                config: I2cSlaveConfig,
            ) -> RpcResult<Result<(), I2cError<'_>>> {
                Ready(self.engine.configure(&config).map_err(Into::into))
            }

            fn close_i2c_slave_events(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {
                self.engine.disable();
            }

            fn stream_i2c_slave_events(&mut self) -> &mut Counted {
                &mut self.events
            }

            fn i2c_slave_map_write(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                offset: u32,
                data: MaxLen<RefVec<'_, u8>, { ww_i2c::MAX_WRITE_LEN }>,
            ) -> RpcResult<Result<(), I2cError<'_>>> {
                Ready(
                    self.engine
                        .map_write(offset, data.as_slice())
                        .map_err(Into::into),
                )
            }

            fn i2c_slave_map_read(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                offset: u32,
                len: u16,
            ) -> RpcResult<Result<MaxLen<RefVec<'_, u8>, MAX_READ_LEN>, I2cError<'_>>> {
                let len = len as usize;
                if len > self.map_scratch.len() {
                    return Ready(Err(I2cError::BufferTooBig));
                }
                match self.engine.map_read(offset, &mut self.map_scratch[..len]) {
                    Ok(()) => Ready(Ok(
                        MaxLen::new(RefVec::new_bytes(&self.map_scratch[..len])).unwrap()
                    )),
                    Err(e) => Ready(Err(e.into())),
                }
            }

            fn i2c_slave_clear_answers(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<u32> {
                Ready(self.engine.clear_answers())
            }

            fn credit_i2c_slave_answers(&mut self) -> &mut SinkCredit {
                &mut self.slave_answers_credit
            }

            fn sideband_i2c_slave_answers(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _sideband: StreamSideband,
            ) -> Option<StreamSideband> {
                None
            }

            fn write_i2c_slave_answers(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                data: &shrink_wrap::tail_bytes::TailBytes<'_>,
            ) {
                self.engine
                    .push_answer(data.0)
                    .expect("test script respects slave_capabilities().queue_len and answer size");
                self.slave_answers_credit.free(data.0.len() as u32);
            }

            fn slave_bus(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                op: SlaveBusOp,
            ) -> RpcResult<SlaveBusResult> {
                let now = self.now_us;
                Ready(match op {
                    SlaveBusOp::Advance(us) => {
                        self.now_us = self.now_us.wrapping_add(us);
                        SlaveBusResult::Advanced
                    }
                    SlaveBusOp::Address { read } => {
                        SlaveBusResult::Ack(self.engine.address(read, now))
                    }
                    SlaveBusOp::Restart => {
                        self.engine.restart(now);
                        SlaveBusResult::Done
                    }
                    SlaveBusOp::Write(byte) => {
                        self.engine.write(byte, now);
                        SlaveBusResult::Written
                    }
                    SlaveBusOp::Read => match self.engine.read(now) {
                        ReadByte::Byte(b) => SlaveBusResult::Byte(b),
                        ReadByte::Stretch => SlaveBusResult::Stretch,
                    },
                    SlaveBusOp::Resume => SlaveBusResult::Resumed(self.engine.resume(now)),
                    SlaveBusOp::Unsent(n) => {
                        self.engine.unsent(n);
                        SlaveBusResult::Done
                    }
                    SlaveBusOp::Stop => {
                        self.engine.stop(now);
                        SlaveBusResult::Done
                    }
                    SlaveBusOp::BusError => {
                        self.engine.bus_error(now);
                        SlaveBusResult::Done
                    }
                    SlaveBusOp::Unknown(_) => unreachable!("test never sends an unknown op"),
                })
            }

            fn slave_pump(&mut self, cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<()> {
                self.pump(cx);
                Ready(())
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                io_i2c_api :: IoI2cSlaveTest for super::Server,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
                // debug_to_file = "../../target/tests_io_i2c_slave_server.rs"
            );
        }

        impl TestProcessEvents for Server {
            type Medium = ();

            fn process_request_bytes<'a>(
                &mut self,
                bytes: &[u8],
                scratch: &'a mut [u8],
                out: &mut impl BlockingEventOut,
                medium: (),
            ) -> Result<&'a [u8], ShrinkWrapError> {
                self.process_request_bytes(bytes, scratch, out, medium)
            }
        }
    }

    mod slave_std_client {
        use wire_weaver_client::{ClientConfig, Commander, WwClient};

        pub struct SlaveClient {
            pub cmd: Commander,
        }

        impl WwClient for SlaveClient {
            fn default_config() -> ClientConfig {
                ClientConfig::default()
            }

            fn from_cmd(cmd: Commander) -> Self {
                Self { cmd }
            }
        }

        mod api_client {
            wire_weaver::ww_codegen!(
                io_i2c_api :: IoI2cSlaveTest for super::SlaveClient,
                client = "std_client",
            );
        }
    }

    use slave_std_client::SlaveClient;

    fn start_slave(path: &str) -> TestDevice {
        let _ = tracing_subscriber::fmt::try_init();
        let server = slave_mock::Server::new();
        tests_common::start_device(
            path,
            server,
            io_i2c_api::IO_I_2_C_SLAVE_TEST_FULL_GID,
            slave_mock::api_impl::api_hash(),
        )
    }

    async fn connect_slave(device: &TestDevice) -> SlaveClient {
        SlaveClient::config(|c| c.in_process_path(device.path()))
            .connect()
            .await
            .expect("connect")
    }

    fn registers_config(
        addr: u8,
        pointer_bytes: u8,
        len: u32,
        page: Option<u16>,
        read_only: bool,
    ) -> ww_i2c::I2cSlaveConfig {
        ww_i2c::I2cSlaveConfig {
            addr: I2cAddress::seven(addr).unwrap(),
            io_level: None,
            answers: ww_i2c::I2cSlaveAnswers::Registers {
                pointer_bytes,
                len,
                page,
                read_only,
            },
        }
    }

    fn queue_config(addr: u8, empty: ww_i2c::I2cSlaveUnderrun) -> ww_i2c::I2cSlaveConfig {
        ww_i2c::I2cSlaveConfig {
            addr: I2cAddress::seven(addr).unwrap(),
            io_level: None,
            answers: ww_i2c::I2cSlaveAnswers::Queue { empty },
        }
    }

    use io_i2c_api::{SlaveBusOp, SlaveBusResult};

    async fn bus(client: &mut SlaveClient, op: SlaveBusOp) -> SlaveBusResult {
        client.slave_bus(op).call().await.unwrap()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn slave_registers_write_read_and_page_wrap() {
        let device = start_slave("io_i2c_slave/registers");
        let mut client = connect_slave(&device).await;

        let ((), mut events) = client
            .i2c()
            .slave_events(registers_config(0x50, 1, 64, Some(8), false))
            .call()
            .await
            .unwrap()
            .unwrap();

        // master writes starting at 6, in an 8-byte page: 6, 7, then wraps to 0, 1
        assert_eq!(
            bus(&mut client, SlaveBusOp::Address { read: false }).await,
            SlaveBusResult::Ack(true)
        );
        for b in [6, 0xA, 0xB, 0xC, 0xD] {
            assert_eq!(
                bus(&mut client, SlaveBusOp::Write(b)).await,
                SlaveBusResult::Written
            );
        }
        assert_eq!(
            bus(&mut client, SlaveBusOp::Stop).await,
            SlaveBusResult::Done
        );

        let map = client.i2c().slave_map_read(0, 8).call().await.unwrap();
        assert_eq!(map.as_slice(), &[0xC, 0xD, 0, 0, 0, 0, 0xA, 0xB]);

        // a register read crossing the page: pointer 7 -> 0xB, then wraps within the page to 0
        assert_eq!(
            bus(&mut client, SlaveBusOp::Address { read: false }).await,
            SlaveBusResult::Ack(true)
        );
        assert_eq!(
            bus(&mut client, SlaveBusOp::Write(7)).await,
            SlaveBusResult::Written
        );
        assert_eq!(
            bus(&mut client, SlaveBusOp::Address { read: true }).await,
            SlaveBusResult::Ack(true)
        );
        assert_eq!(
            bus(&mut client, SlaveBusOp::Read).await,
            SlaveBusResult::Byte(0xB)
        );
        assert_eq!(
            bus(&mut client, SlaveBusOp::Read).await,
            SlaveBusResult::Byte(0)
        );
        assert_eq!(
            bus(&mut client, SlaveBusOp::Stop).await,
            SlaveBusResult::Done
        );

        client.slave_pump().call().await.unwrap();
        let ev = events.recv().await.unwrap();
        assert!(matches!(
            ev,
            ww_i2c::I2cSlaveEventOwned::Transfer(ref t) if !t.read && t.bytes.as_slice() == [6u8, 0xA, 0xB, 0xC, 0xD]
        ));
        let ev = events.recv().await.unwrap();
        assert!(matches!(
            ev,
            ww_i2c::I2cSlaveEventOwned::Transfer(ref t) if !t.read && t.kind == I2cRecordKind::RepeatedStart
        ));
        let ev = events.recv().await.unwrap();
        assert!(matches!(
            ev,
            ww_i2c::I2cSlaveEventOwned::Transfer(ref t) if t.read && t.bytes.as_slice() == [0xBu8, 0]
        ));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn slave_host_map_write_applies_between_transactions_not_inside() {
        let device = start_slave("io_i2c_slave/host_update");
        let mut client = connect_slave(&device).await;
        let (_, _events) = client
            .i2c()
            .slave_events(registers_config(0x50, 1, 64, None, false))
            .call()
            .await
            .unwrap()
            .unwrap();
        client
            .i2c()
            .slave_map_write(0, MaxLen::new(vec![1, 1]).unwrap())
            .call()
            .await
            .unwrap();

        assert_eq!(
            bus(&mut client, SlaveBusOp::Address { read: false }).await,
            SlaveBusResult::Ack(true)
        );
        assert_eq!(
            bus(&mut client, SlaveBusOp::Write(0)).await,
            SlaveBusResult::Written
        );
        assert_eq!(
            bus(&mut client, SlaveBusOp::Address { read: true }).await,
            SlaveBusResult::Ack(true)
        );
        assert_eq!(
            bus(&mut client, SlaveBusOp::Read).await,
            SlaveBusResult::Byte(1)
        );
        // the host updates the map mid-transaction: the master still reads the old value
        client
            .i2c()
            .slave_map_write(0, MaxLen::new(vec![2, 2]).unwrap())
            .call()
            .await
            .unwrap();
        assert_eq!(
            bus(&mut client, SlaveBusOp::Read).await,
            SlaveBusResult::Byte(1)
        );
        assert_eq!(
            bus(&mut client, SlaveBusOp::Stop).await,
            SlaveBusResult::Done
        );

        let map = client.i2c().slave_map_read(0, 2).call().await.unwrap();
        assert_eq!(map.as_slice(), &[2, 2]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn slave_answer_queue_one_per_read_fill_for_longer_rest_dropped() {
        let device = start_slave("io_i2c_slave/answer_queue");
        let mut client = connect_slave(&device).await;
        let (_, _events) = client
            .i2c()
            .slave_events(queue_config(0x50, ww_i2c::I2cSlaveUnderrun::Fill(0xEE)))
            .call()
            .await
            .unwrap()
            .unwrap();

        let mut answers = client.i2c().slave_answers().await.unwrap();
        answers.send_bytes(&[1, 2, 3]).await.unwrap();
        answers.send_bytes(&[4, 5]).await.unwrap();
        answers.flush().await.unwrap();

        // shorter read: the rest of the first answer is dropped
        assert_eq!(
            bus(&mut client, SlaveBusOp::Address { read: true }).await,
            SlaveBusResult::Ack(true)
        );
        assert_eq!(
            bus(&mut client, SlaveBusOp::Read).await,
            SlaveBusResult::Byte(1)
        );
        assert_eq!(
            bus(&mut client, SlaveBusOp::Read).await,
            SlaveBusResult::Byte(2)
        );
        assert_eq!(
            bus(&mut client, SlaveBusOp::Stop).await,
            SlaveBusResult::Done
        );

        // longer read: the second answer, then fill
        assert_eq!(
            bus(&mut client, SlaveBusOp::Address { read: true }).await,
            SlaveBusResult::Ack(true)
        );
        for expect in [4, 5, 0xEE, 0xEE] {
            assert_eq!(
                bus(&mut client, SlaveBusOp::Read).await,
                SlaveBusResult::Byte(expect)
            );
        }
        assert_eq!(
            bus(&mut client, SlaveBusOp::Stop).await,
            SlaveBusResult::Done
        );

        // nothing queued: fill only
        assert_eq!(
            bus(&mut client, SlaveBusOp::Address { read: true }).await,
            SlaveBusResult::Ack(true)
        );
        assert_eq!(
            bus(&mut client, SlaveBusOp::Read).await,
            SlaveBusResult::Byte(0xEE)
        );
        assert_eq!(
            bus(&mut client, SlaveBusOp::Stop).await,
            SlaveBusResult::Done
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn slave_stretch_read_waiting_then_timely_answer() {
        let device = start_slave("io_i2c_slave/stretch_timely");
        let mut client = connect_slave(&device).await;
        let ((), mut events) = client
            .i2c()
            .slave_events(queue_config(
                0x50,
                ww_i2c::I2cSlaveUnderrun::Stretch {
                    max: secs(5_000),
                    fill: 0xFF,
                },
            ))
            .call()
            .await
            .unwrap()
            .unwrap();

        // a command, then a read with nothing queued yet: the master stretches
        assert_eq!(
            bus(&mut client, SlaveBusOp::Address { read: false }).await,
            SlaveBusResult::Ack(true)
        );
        assert_eq!(
            bus(&mut client, SlaveBusOp::Write(0x30)).await,
            SlaveBusResult::Written
        );
        assert_eq!(
            bus(&mut client, SlaveBusOp::Address { read: true }).await,
            SlaveBusResult::Ack(true)
        );
        assert_eq!(
            bus(&mut client, SlaveBusOp::Read).await,
            SlaveBusResult::Stretch
        );
        client.slave_pump().call().await.unwrap();
        // the write (as a repeated start) and ReadWaiting are seen before the read ends
        let ev = events.recv().await.unwrap();
        assert!(matches!(ev, ww_i2c::I2cSlaveEventOwned::Transfer(ref t) if !t.read));
        let ev = events.recv().await.unwrap();
        assert!(matches!(ev, ww_i2c::I2cSlaveEventOwned::ReadWaiting));

        // the host answers in time
        let mut answers = client.i2c().slave_answers().await.unwrap();
        answers.send_bytes(&[0xA1, 0xA2]).await.unwrap();
        answers.flush().await.unwrap();
        assert_eq!(
            bus(&mut client, SlaveBusOp::Resume).await,
            SlaveBusResult::Resumed(Some(0xA1))
        );
        assert_eq!(
            bus(&mut client, SlaveBusOp::Read).await,
            SlaveBusResult::Byte(0xA2)
        );
        assert_eq!(
            bus(&mut client, SlaveBusOp::Read).await,
            SlaveBusResult::Byte(0xFF)
        );
        assert_eq!(
            bus(&mut client, SlaveBusOp::Stop).await,
            SlaveBusResult::Done
        );
        client.slave_pump().call().await.unwrap();
        let ev = events.recv().await.unwrap();
        match ev {
            ww_i2c::I2cSlaveEventOwned::Transfer(t) => {
                assert!(t.read);
                assert_eq!(t.bytes.as_slice(), &[0xA1u8, 0xA2, 0xFF]);
                assert_eq!(t.filled, 1);
                assert!(t.stretched.is_some());
            }
            ev => panic!("{ev:?}"),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn slave_stretch_past_deadline_sends_fill_then_late_answer_goes_to_next_read() {
        let device = start_slave("io_i2c_slave/stretch_late");
        let mut client = connect_slave(&device).await;
        let (_, _events) = client
            .i2c()
            .slave_events(queue_config(
                0x50,
                ww_i2c::I2cSlaveUnderrun::Stretch {
                    max: secs(1000),
                    fill: 0xEE,
                },
            ))
            .call()
            .await
            .unwrap()
            .unwrap();

        assert_eq!(
            bus(&mut client, SlaveBusOp::Address { read: true }).await,
            SlaveBusResult::Ack(true)
        );
        assert_eq!(
            bus(&mut client, SlaveBusOp::Read).await,
            SlaveBusResult::Stretch
        );
        bus(&mut client, SlaveBusOp::Advance(999)).await;
        assert_eq!(
            bus(&mut client, SlaveBusOp::Resume).await,
            SlaveBusResult::Resumed(None)
        );
        bus(&mut client, SlaveBusOp::Advance(1)).await;
        assert_eq!(
            bus(&mut client, SlaveBusOp::Resume).await,
            SlaveBusResult::Resumed(Some(0xEE))
        );

        // a late answer: the rest of this read is still fill, the answer goes to the next read
        let mut answers = client.i2c().slave_answers().await.unwrap();
        answers.send_bytes(&[5]).await.unwrap();
        answers.flush().await.unwrap();
        assert_eq!(
            bus(&mut client, SlaveBusOp::Read).await,
            SlaveBusResult::Byte(0xEE)
        );
        assert_eq!(
            bus(&mut client, SlaveBusOp::Stop).await,
            SlaveBusResult::Done
        );

        assert_eq!(
            bus(&mut client, SlaveBusOp::Address { read: true }).await,
            SlaveBusResult::Ack(true)
        );
        assert_eq!(
            bus(&mut client, SlaveBusOp::Read).await,
            SlaveBusResult::Byte(5)
        );
        assert_eq!(
            bus(&mut client, SlaveBusOp::Stop).await,
            SlaveBusResult::Done
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn slave_nack_while_empty() {
        let device = start_slave("io_i2c_slave/nack");
        let mut client = connect_slave(&device).await;
        let (_, _events) = client
            .i2c()
            .slave_events(queue_config(0x50, ww_i2c::I2cSlaveUnderrun::Nack))
            .call()
            .await
            .unwrap()
            .unwrap();

        // nothing queued: not even a write is acknowledged
        assert_eq!(
            bus(&mut client, SlaveBusOp::Address { read: false }).await,
            SlaveBusResult::Ack(false)
        );

        let mut answers = client.i2c().slave_answers().await.unwrap();
        answers.send_bytes(&[1, 2]).await.unwrap();
        answers.flush().await.unwrap();

        assert_eq!(
            bus(&mut client, SlaveBusOp::Address { read: false }).await,
            SlaveBusResult::Ack(true)
        );
        assert_eq!(
            bus(&mut client, SlaveBusOp::Write(9)).await,
            SlaveBusResult::Written
        );
        assert_eq!(
            bus(&mut client, SlaveBusOp::Stop).await,
            SlaveBusResult::Done
        );

        assert_eq!(
            bus(&mut client, SlaveBusOp::Address { read: true }).await,
            SlaveBusResult::Ack(true)
        );
        for expect in [1, 2, 0xFF] {
            assert_eq!(
                bus(&mut client, SlaveBusOp::Read).await,
                SlaveBusResult::Byte(expect)
            );
        }
        assert_eq!(
            bus(&mut client, SlaveBusOp::Stop).await,
            SlaveBusResult::Done
        );

        // empty again: not acknowledged
        assert_eq!(
            bus(&mut client, SlaveBusOp::Address { read: false }).await,
            SlaveBusResult::Ack(false)
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn slave_clear_answers_drops_only_waiting() {
        let device = start_slave("io_i2c_slave/clear_answers");
        let mut client = connect_slave(&device).await;
        let (_, _events) = client
            .i2c()
            .slave_events(queue_config(0x50, ww_i2c::I2cSlaveUnderrun::Fill(0xEE)))
            .call()
            .await
            .unwrap()
            .unwrap();

        let mut answers = client.i2c().slave_answers().await.unwrap();
        for b in [0u8, 1, 2] {
            answers.send_bytes(&[b]).await.unwrap();
        }
        answers.flush().await.unwrap();

        assert_eq!(
            bus(&mut client, SlaveBusOp::Address { read: true }).await,
            SlaveBusResult::Ack(true)
        );
        assert_eq!(
            bus(&mut client, SlaveBusOp::Read).await,
            SlaveBusResult::Byte(0)
        );
        let dropped = client.i2c().slave_clear_answers().call().await.unwrap();
        assert_eq!(dropped, 2);
        assert_eq!(
            bus(&mut client, SlaveBusOp::Stop).await,
            SlaveBusResult::Done
        );

        // the cleared answers never come back: a new read is fill only
        assert_eq!(
            bus(&mut client, SlaveBusOp::Address { read: true }).await,
            SlaveBusResult::Ack(true)
        );
        assert_eq!(
            bus(&mut client, SlaveBusOp::Read).await,
            SlaveBusResult::Byte(0xEE)
        );
        assert_eq!(
            bus(&mut client, SlaveBusOp::Stop).await,
            SlaveBusResult::Done
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn slave_dropped_events_are_counted() {
        let device = start_slave("io_i2c_slave/dropped");
        let mut client = connect_slave(&device).await;
        let ((), mut events) = client
            .i2c()
            .slave_events(registers_config(0x50, 0, 4, None, false))
            .call()
            .await
            .unwrap()
            .unwrap();

        // the event ring holds 8 (EV): 10 single-byte writes without a pump overflow it by 2
        for _ in 0..10 {
            assert_eq!(
                bus(&mut client, SlaveBusOp::Address { read: false }).await,
                SlaveBusResult::Ack(true)
            );
            assert_eq!(
                bus(&mut client, SlaveBusOp::Write(0)).await,
                SlaveBusResult::Written
            );
            assert_eq!(
                bus(&mut client, SlaveBusOp::Stop).await,
                SlaveBusResult::Done
            );
        }
        client.slave_pump().call().await.unwrap();

        let mut seen = 0;
        for _ in 0..8 {
            match events.recv_event().await.unwrap() {
                wire_weaver_client::StreamEvent::Item { .. } => seen += 1,
                ev => panic!("{ev:?}"),
            }
        }
        assert_eq!(seen, 8);

        // the drop is reported after the 8 it kept, not before: it only shows up once there is a next event to
        // carry it
        assert_eq!(
            bus(&mut client, SlaveBusOp::Address { read: false }).await,
            SlaveBusResult::Ack(true)
        );
        assert_eq!(
            bus(&mut client, SlaveBusOp::Write(0)).await,
            SlaveBusResult::Written
        );
        assert_eq!(
            bus(&mut client, SlaveBusOp::Stop).await,
            SlaveBusResult::Done
        );
        client.slave_pump().call().await.unwrap();
        match events.recv_event().await.unwrap() {
            wire_weaver_client::StreamEvent::Gap { units, .. } => assert_eq!(units, 2),
            ev => panic!("{ev:?}"),
        }
    }
}
