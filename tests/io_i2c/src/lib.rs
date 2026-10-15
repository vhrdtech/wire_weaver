//! `ww_i2c` end to end: a mock server on `ww_device::Server` through `tests_common::start_device`, the generated
//! client through the real host event loop.

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};
    use std::sync::{Arc, Mutex};
    use tests_common::TestDevice;
    use wire_weaver::prelude::*;
    use ww_i2c::{
        I2cAddress, I2cCapabilities, I2cError, I2cLinesState, I2cMode, I2cOp, I2cReadEnvelope,
        I2cReadKind, I2cReadKindOwned, I2cRecord, I2cRecordKind, I2cTransactionResult,
        I2cTransactionTimeRange, I2cWatchCapabilities, I2cWatchConfig, I2cWatchConfigOwned,
        I2cWatchEvent, I2cWatchReportOwned, MAX_READ_LEN, MAX_SCAN, MAX_WATCHES, Second, Hertz,
    };
    use ww_si::Prefix;
    use ww_si::ww_numeric::NumericValue;
    use ww_stream::tx::Counted;

    fn secs(v: u32) -> Second {
        Second {
            prefix: Prefix::Micro,
            value: NumericValue::U32(v),
        }
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
        fn read(&self, addr: I2cAddress, offset: usize, len: u16) -> Result<Vec<u8>, I2cError> {
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
    fn read_kind_to_owned(kind: &I2cReadKind<'_>) -> Result<I2cReadKindOwned, I2cError> {
        Ok(match kind {
            I2cReadKind::Plain => I2cReadKindOwned::Plain,
            I2cReadKind::RepeatedStart { write } => I2cReadKindOwned::RepeatedStart {
                write: MaxLen::new_unchecked(write.as_slice().to_vec()),
            },
            I2cReadKind::Unknown(_) => return Err(I2cError::Unsupported),
        })
    }

    /// Builds the owned equivalent of a borrowed `I2cWatchReport` to store beyond the call that carried it.
    fn report_to_owned(
        report: &ww_i2c::I2cWatchReport<'_>,
    ) -> Result<I2cWatchReportOwned, I2cError> {
        Ok(match report {
            ww_i2c::I2cWatchReport::Always => I2cWatchReportOwned::Always,
            ww_i2c::I2cWatchReport::Changed { mask } => I2cWatchReportOwned::Changed {
                mask: MaxLen::new_unchecked(mask.as_slice().to_vec()),
            },
            ww_i2c::I2cWatchReport::Matches { mask, value } => I2cWatchReportOwned::Matches {
                mask: MaxLen::new_unchecked(mask.as_slice().to_vec()),
                value: MaxLen::new_unchecked(value.as_slice().to_vec()),
            },
            ww_i2c::I2cWatchReport::Unknown(_) => return Err(I2cError::Unsupported),
        })
    }

    /// `Unsupported` for a read kind or report of a newer API version.
    fn watch_config_to_owned(config: &I2cWatchConfig<'_>) -> Result<I2cWatchConfigOwned, I2cError> {
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
            watch_list_scratch: [u8; MAX_WATCHES],
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
                    watch_list_scratch: [0u8; MAX_WATCHES],
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
                    min_freq: Hertz::kilohertz(1u16),
                    max_freq: Hertz::kilohertz(400u16),
                    driver_name: MaxLen::new_unchecked("mock"),
                    master_supported: true,
                    slave_supported: false,
                    monitor_supported: true,
                    ten_bit_addresses: true,
                    pullups: true,
                    bus_recovery: true,
                })
            }

            fn i2c_i2c_configure(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                mode: I2cMode,
                _io_level: Option<ww_si::Volt>,
            ) -> RpcResult<Result<Hertz, I2cError>> {
                let max = Hertz::kilohertz(400u16);
                Ready(match mode {
                    I2cMode::Disabled | I2cMode::MonitorOnly => Ok(Hertz::hertz(0u8)),
                    I2cMode::Master { speed, .. } if speed > max => Ok(max),
                    I2cMode::Master { speed, .. } => Ok(speed),
                    I2cMode::Slave { .. } | I2cMode::Unknown(_) => Err(I2cError::UnsupportedMode),
                })
            }

            fn i2c_i2c_write(
                &mut self,
                cx: &mut Context<'_, impl BlockingEventOut>,
                addr: I2cAddress,
                data: MaxLen<RefVec<'_, u8>, { ww_i2c::MAX_WRITE_LEN }>,
                _delay_after: Option<Second>,
            ) -> RpcResult<Result<I2cTransactionTimeRange, I2cError>> {
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
            ) -> RpcResult<Result<I2cReadEnvelope<'_>, I2cError>> {
                if len == 0 {
                    return Ready(Err(I2cError::ZeroLengthTransfer));
                }
                let Some(offset) = offset_of(&kind) else {
                    return Ready(Err(I2cError::Unsupported));
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
            ) -> RpcResult<Result<MaxLen<RefVec<'_, u8>, MAX_SCAN>, I2cError>> {
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
            ) -> RpcResult<Result<I2cTransactionResult<'_>, I2cError>> {
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
                        return Ready(Err(I2cError::MalformedRequest));
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
                        I2cOp::Unknown(_) => return Ready(Err(I2cError::Unsupported)),
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
            ) -> RpcResult<Result<I2cLinesState, I2cError>> {
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
            ) -> RpcResult<Result<(), I2cError>> {
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
                    max_watches: MAX_WATCHES as u8,
                    max_len: MAX_WATCH_LEN as u16,
                    min_period: secs(0),
                })
            }

            fn i2c_watch_watch_add(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                watch: I2cWatchConfig<'_>,
            ) -> RpcResult<Result<u8, I2cError>> {
                let config = match watch_config_to_owned(&watch) {
                    Ok(config) => config,
                    Err(e) => return Ready(Err(e)),
                };
                let Some(slot) = self.watches.iter().position(Option::is_none) else {
                    return Ready(Err(I2cError::OutOfCycleSlots));
                };
                self.watches[slot] = Some(WatchSlot {
                    config,
                    last_reported: None,
                    last_match: None,
                    in_error: false,
                });
                Ready(Ok(slot as u8))
            }

            fn i2c_watch_watch_modify(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                slot: u8,
                watch: I2cWatchConfig<'_>,
            ) -> RpcResult<Result<(), I2cError>> {
                let Some(entry) = self.watches.get_mut(slot as usize) else {
                    return Ready(Err(I2cError::InvalidCycleSlot));
                };
                if entry.is_none() {
                    return Ready(Err(I2cError::InvalidCycleSlot));
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
                slot: u8,
            ) -> RpcResult<Result<(), I2cError>> {
                let Some(entry) = self.watches.get_mut(slot as usize) else {
                    return Ready(Err(I2cError::InvalidCycleSlot));
                };
                if entry.is_none() {
                    return Ready(Err(I2cError::InvalidCycleSlot));
                }
                *entry = None;
                Ready(Ok(()))
            }

            fn i2c_watch_watch_list(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<MaxLen<RefVec<'_, u8>, MAX_WATCHES>> {
                let list: Vec<u8> = self
                    .watches
                    .iter()
                    .enumerate()
                    .filter_map(|(i, w)| w.as_ref().map(|_| i as u8))
                    .collect();
                self.watch_list_scratch[..list.len()].copy_from_slice(&list);
                let len = list.len();
                Ready(MaxLen::new(RefVec::new_bytes(&self.watch_list_scratch[..len])).unwrap())
            }

            fn i2c_monitor_records(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _io_level: Option<ww_si::Volt>,
            ) -> RpcResult<Result<(), I2cError>> {
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
                            should_report.then_some(Err(*e))
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
                                I2cWatchReportOwned::Matches { mask, value } => {
                                    let masked: Vec<u8> = data
                                        .iter()
                                        .zip(mask.iter().chain(std::iter::repeat(&0xFFu8)))
                                        .map(|(d, m)| d & m)
                                        .collect();
                                    let is_match = masked == value.as_slice();
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
                            slot: slot_idx as u8,
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
        assert_eq!(err.device(), Some(&I2cError::Nack));

        let addr_ok = I2cAddress::seven(0x52).unwrap();
        let err = client
            .i2c()
            .i2c_read(addr_ok, I2cReadKindOwned::Plain, 0, None)
            .call()
            .await
            .unwrap_err();
        assert_eq!(err.device(), Some(&I2cError::ZeroLengthTransfer));
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
                    mask: MaxLen::new(vec![0x01]).unwrap(),
                    value: MaxLen::new(vec![0x01]).unwrap(),
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
        assert_eq!(ev.result.unwrap_err(), I2cError::Nack);

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
        assert_eq!(err.device(), Some(&I2cError::InvalidCycleSlot));
    }
}
