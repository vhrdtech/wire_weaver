//! `ww_counters` end to end: a mock server on `ww_device::Server` through `tests_common::start_device` serves words
//! laid out as the test firmware `fixtures/counters_fw.elf` (built from `fixtures/counters_fw` in the cnt repo) lays
//! out its counters; the host reads them through the real event loop and decodes them with that ELF (`cnt_core`).

#[cfg(test)]
mod tests {
    use cnt_core::elf;
    use cnt_core::{Counters, Source, Storage};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};
    use tests_common::TestDevice;
    use wire_weaver::prelude::*;
    use ww_counters::host::{CountersDevice, WwSource};
    use ww_counters::{CounterBuffer, CountersInfoOwned};

    /// Words the test firmware's counters take: RAM 7 of 16, BKP 4 of 8 (`CNT_*_BUFFER_SIZE_WORDS` of the fixture).
    const RAM_USED: usize = 7;
    const BKP_USED: usize = 4;

    fn fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/counters_fw.elf")
    }

    fn fixture_build_id() -> Vec<u8> {
        elf::build_id(&std::fs::read(fixture()).unwrap())
            .unwrap()
            .unwrap()
    }

    /// The device's buffers, shared with the test to change words while it runs.
    pub struct Buffers {
        pub ram: Vec<AtomicU32>,
        pub ram_used: usize,
        pub bkp: Option<Vec<AtomicU32>>,
    }

    fn words(n: usize) -> Vec<AtomicU32> {
        (0..n).map(|_| AtomicU32::new(0)).collect()
    }

    mod server {
        use super::*;
        use tests_common::TestProcessEvents;
        use wire_weaver::BlockingEventOut;
        use ww_counters::{
            BufferInfo, CountersError, CountersInfo, MAX_WORDS, read_words, reset_words,
        };

        pub struct Server {
            pub buffers: Arc<Buffers>,
            pub build_id: Vec<u8>,
            pub scratch: [UVlq32; MAX_WORDS],
        }

        impl Server {
            fn buffer(&self, buffer: CounterBuffer) -> Option<(&[AtomicU32], usize)> {
                match buffer {
                    CounterBuffer::Ram => Some((&self.buffers.ram, self.buffers.ram_used)),
                    CounterBuffer::Backup => self.buffers.bkp.as_deref().map(|b| (b, BKP_USED)),
                }
            }

            fn counters_read(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                buffer: CounterBuffer,
                offset: u16,
            ) -> RpcResult<Result<MaxLen<RefVec<'_, UVlq32>, 64>, CountersError<'_>>> {
                let buffers = self.buffers.clone();
                let (words, used) = match buffer {
                    CounterBuffer::Ram => (&buffers.ram[..], buffers.ram_used),
                    CounterBuffer::Backup => match &buffers.bkp {
                        Some(bkp) => (&bkp[..], BKP_USED),
                        None => return RpcResult::Ready(Err(CountersError::unsupported())),
                    },
                };
                RpcResult::Ready(read_words(words, used, offset, &mut self.scratch))
            }

            fn counters_reset(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                buffer: CounterBuffer,
            ) -> RpcResult<Result<(), CountersError<'_>>> {
                match self.buffer(buffer) {
                    Some((words, _)) => {
                        reset_words(words);
                        RpcResult::Ready(Ok(()))
                    }
                    None => RpcResult::Ready(Err(CountersError::unsupported())),
                }
            }
        }

        /// Used words and the build id are fixed at link time: a `fn` (`const_model`).
        impl api_impl::CountersTestConsts for Server {
            fn counters_info(&self) -> CountersInfo<'_> {
                CountersInfo {
                    format: cnt_core::SIGNATURE_VERSION,
                    ram: BufferInfo::new(self.buffers.ram_used, &self.buffers.ram),
                    backup: self
                        .buffers
                        .bkp
                        .as_deref()
                        .map(|b| BufferInfo::new(BKP_USED, b)),
                    build_id: MaxLen::new_unchecked(RefVec::new_bytes(&self.build_id)),
                }
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                counters_api :: CountersTest for super::Server,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
                const_model = "INFO=fn",
                // polling read only
                omit = ["counters.watch"],
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

        #[derive(Clone)]
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
                counters_api :: CountersTest for super::StdClient,
                client = "std_client",
            );
        }
    }

    use std_client::StdClient;

    /// The calls `WwSource` makes, on the generated client.
    struct Device(StdClient);

    impl CountersDevice for Device {
        fn read(&mut self, buffer: CounterBuffer, offset: u16) -> anyhow::Result<Vec<u32>> {
            let page = self
                .0
                .counters()
                .read(buffer, offset)
                .blocking_call()
                .map_err(|e| anyhow::anyhow!("{e:?}"))?;
            Ok(page.iter().map(|w| w.0).collect())
        }

        fn reset(&mut self, buffer: CounterBuffer) -> anyhow::Result<()> {
            self.0
                .counters()
                .reset(buffer)
                .blocking_call()
                .map_err(|e| anyhow::anyhow!("{e:?}"))
        }
    }

    fn start(path: &str, build_id: Vec<u8>, ram_used: usize, bkp: bool) -> (TestDevice, Arc<Buffers>) {
        let _ = tracing_subscriber::fmt::try_init();
        let buffers = Arc::new(Buffers {
            ram: words(ram_used.max(16)),
            ram_used,
            bkp: bkp.then(|| words(8)),
        });
        let device = tests_common::start_device(
            path,
            server::Server {
                buffers: buffers.clone(),
                build_id,
                scratch: [UVlq32(0); ww_counters::MAX_WORDS],
            },
            counters_api::COUNTERS_TEST_FULL_GID,
            server::api_impl::api_hash(),
        );
        (device, buffers)
    }

    async fn connect(device: &TestDevice) -> (StdClient, CountersInfoOwned) {
        let client = StdClient::config(|c| c.in_process_path(device.path()))
            .connect()
            .await
            .expect("connect");
        let info = client.counters().read_INFO().read().await.unwrap();
        (client, info)
    }

    fn set(buf: &[AtomicU32], words: &[(usize, u32)]) {
        for (idx, value) in words {
            buf[*idx].store(*value, Ordering::Relaxed);
        }
    }

    fn values(counters: &Counters) -> Vec<(String, u64)> {
        counters
            .blocks()
            .flat_map(|b| {
                b.values()
                    .map(move |(c, v)| (format!("{}:{}", b.storage(), c.qualified_name()), v.to_u64()))
            })
            .filter(|(_, v)| *v != 0)
            .collect()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn info_reports_buffers_and_build_id() {
        let id = fixture_build_id();
        let (device, _) = start("counters/info", id.clone(), RAM_USED, true);
        let (_, info) = connect(&device).await;
        assert_eq!(info.format, 2);
        assert_eq!((info.ram.used_words, info.ram.capacity_words), (7, 16));
        let bkp = info.backup.unwrap();
        assert_eq!((bkp.used_words, bkp.capacity_words), (4, 8));
        assert_eq!(info.build_id.as_slice(), id);
    }

    /// The whole path: ELF found by the device's build id, words over WireWeaver, decoded with the ELF.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn decoded_with_the_elf_found_by_build_id() {
        let (device, buffers) = start("counters/decode", fixture_build_id(), RAM_USED, true);
        // boots = 3, loops = 2^32 + 5, uart/CrcError = 2, radio/RxBytes = 100
        set(&buffers.ram, &[(0, 3), (1, 5), (2, 1), (4, 2)]);
        set(buffers.bkp.as_ref().unwrap(), &[(2, 100)]);
        let (client, info) = connect(&device).await;

        // The ELF directory has the firmware's ELF under its build id
        let dir = std::env::temp_dir().join(format!("ww-counters-test-{}", std::process::id()));
        elf::add(&fixture(), &dir).unwrap();
        let found = elf::find_in_dirs(&info.build_id, &[dir.clone()]).unwrap();
        let mut counters = Counters::load_elf(&found).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();

        let result = tokio::task::spawn_blocking(move || {
            let mut source = WwSource::new(Device(client), info);
            source.check(&counters, &found)?;
            counters.read(&mut source)?;
            let first = values(&counters);

            // RAM reset over WireWeaver, BKP stays
            counters.reset(Storage::Ram, &mut source)?;
            let changes: Vec<_> = counters
                .read_changes(&mut source)?
                .iter()
                .map(|c| (c.counter.qualified_name(), c.event))
                .collect();
            anyhow::Ok((first, changes, values(&counters)))
        })
        .await
        .unwrap()
        .unwrap();
        let (first, changes, after_reset) = result;
        assert_eq!(
            first,
            [
                ("RAM:boots".to_string(), 3),
                ("RAM:loops".to_string(), (1 << 32) + 5),
                ("RAM:uart/CrcError".to_string(), 2),
                ("BKP:radio/RxBytes".to_string(), 100),
            ]
        );
        assert_eq!(changes.len(), 3);
        assert!(changes.iter().all(|(_, e)| *e == cnt_core::Event::Reset), "{changes:?}");
        assert_eq!(after_reset, [("BKP:radio/RxBytes".to_string(), 100)]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn elf_of_other_firmware_is_refused() {
        let (device, _) = start("counters/mismatch", vec![0x55; 20], RAM_USED, true);
        let (_, info) = connect(&device).await;
        let err = elf::resolve(Some(&fixture()), Some(&info.build_id), false).unwrap_err();
        assert!(err.to_string().contains("wrong names"), "{err}");
        assert!(err.to_string().contains(&"55".repeat(20)), "{err}");
        // Forced: used, with a warning
        let forced = elf::resolve(Some(&fixture()), Some(&info.build_id), true).unwrap();
        assert!(forced.warning.is_some());
        // Not in any ELF directory either
        assert_eq!(elf::find_in_dirs(&info.build_id, &[std::env::temp_dir()]), None);
    }

    /// Firmware without the build id note: nothing to check, the host says so.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn no_build_id_cant_be_checked() {
        let (device, _) = start("counters/no-id", Vec::new(), RAM_USED, true);
        let (_, info) = connect(&device).await;
        assert!(info.build_id.is_empty());
        let resolved = elf::resolve(Some(&fixture()), Some(&info.build_id), false).unwrap();
        assert!(resolved.warning.unwrap().contains("no build id"));
    }

    /// A device with more RAM counters than the ELF: other firmware, even if its build id was forced.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn used_words_must_cover_the_elf() {
        let (device, _) = start("counters/fewer", fixture_build_id(), 3, false);
        let (client, info) = connect(&device).await;
        let counters = Counters::load_elf(&fixture()).unwrap();
        let source = WwSource::new(Device(client), info);
        let err = source.check(&counters, &fixture()).unwrap_err();
        assert!(err.to_string().contains("RAM"), "{err}");
    }

    /// More words than one call returns: read in pages of 64.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pages_of_a_big_buffer() {
        let (device, buffers) = start("counters/pages", fixture_build_id(), 150, false);
        for (i, w) in buffers.ram.iter().enumerate() {
            w.store(i as u32 * 1000, Ordering::Relaxed);
        }
        let (client, info) = connect(&device).await;
        let words = tokio::task::spawn_blocking(move || {
            let mut source = WwSource::new(Device(client), info);
            let mut words = vec![0; 150];
            source.read(Storage::Ram, 0, &mut words)?;
            let mut past = vec![0; 2];
            let err = source.read(Storage::Ram, 149, &mut past).unwrap_err();
            assert!(err.to_string().contains("150"), "{err}");
            // No backup buffer on this device
            assert!(source.read(Storage::Bkp, 0, &mut past).is_err());
            anyhow::Ok(words)
        })
        .await
        .unwrap()
        .unwrap();
        assert_eq!(words, (0..150).map(|i| i * 1000).collect::<Vec<u32>>());
    }
}
