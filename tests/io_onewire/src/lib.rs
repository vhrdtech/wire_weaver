//! `ww_onewire` end to end: a mock bus with a few simulated `RomId`s on `ww_device::Server` through
//! `tests_common::start_device`, driven by the generated client through the real host event loop.
//!
//! The mock implements the actual Dallas/Maxim ROM search algorithm (not just a canned answer), so `search` and
//! `triplet` exercise the same bit-walking logic a real adapter would.

#[cfg(test)]
mod tests {
    use std::sync::{Arc, RwLock};
    use wire_weaver::prelude::*;
    use ww_onewire::{
        OneWireConfig, OneWireError, OneWireSpeed, OneWireTransactionOwned, RomId, SearchState,
    };

    /// Family code used by every simulated device in these tests.
    const FAMILY_A: u8 = 0x28;
    const FAMILY_C: u8 = 0x10;

    const ROM_A: RomId = RomId {
        bytes: [FAMILY_A, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0xAA],
    };
    const ROM_B: RomId = RomId {
        bytes: [FAMILY_A, 0x01, 0x02, 0x03, 0x04, 0x05, 0x07, 0xBB],
    };
    const ROM_C: RomId = RomId {
        bytes: [FAMILY_C, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x00],
    };

    fn bit_at(rom: &RomId, i: u8) -> bool {
        (rom.bytes[(i / 8) as usize] >> (i % 8)) & 1 == 1
    }

    fn set_bit(bytes: &mut [u8; 8], i: u8, v: bool) {
        let mask = 1u8 << (i % 8);
        if v {
            bytes[(i / 8) as usize] |= mask;
        } else {
            bytes[(i / 8) as usize] &= !mask;
        }
    }

    /// `id_bit`/`cmp_bit` the bus would present at bit `i` for the given still-possible `candidates`.
    fn triplet_bits(candidates: &[RomId], i: u8) -> (bool, bool) {
        let all_zero = candidates.iter().all(|r| !bit_at(r, i));
        let all_one = candidates.iter().all(|r| bit_at(r, i));
        match (all_zero, all_one) {
            (true, false) => (false, true),
            (false, true) => (true, false),
            _ => (false, false),
        }
    }

    /// One bit-level search session driven through `triplet`.
    struct TripletSession {
        bit_index: u8,
        accum: [u8; 8],
    }

    /// Bus state: devices present, last `configure`, and a running `triplet` session, if any.
    #[derive(Default)]
    struct Bus {
        roms: Vec<RomId>,
        configured: Option<OneWireConfig>,
        triplet_session: Option<TripletSession>,
    }

    impl Bus {
        fn with_roms(roms: Vec<RomId>) -> Self {
            Bus {
                roms,
                ..Default::default()
            }
        }
    }

    /// Classic Dallas/Maxim search: one full 64-bit walk from `last_discrepancy`/`last_rom`, skipping ids that
    /// don't match `family`, until a match is found or the bus runs out of ids.
    fn do_search(
        roms: &[RomId],
        mut last_discrepancy: u8,
        mut last_rom: Option<[u8; 8]>,
        family: Option<u8>,
    ) -> Option<(RomId, u8)> {
        if roms.is_empty() {
            return None;
        }
        loop {
            let mut rom = [0u8; 8];
            let mut last_zero = 0u8;
            let mut candidates: Vec<RomId> = roms.to_vec();
            for bit_index in 1..=64u8 {
                let i = bit_index - 1;
                if candidates.is_empty() {
                    return None;
                }
                let (id_bit, cmp_bit) = triplet_bits(&candidates, i);
                let chosen = if id_bit != cmp_bit {
                    id_bit
                } else if bit_index == last_discrepancy {
                    true
                } else if bit_index > last_discrepancy {
                    false
                } else {
                    last_rom.is_some_and(|lr| bit_at(&RomId { bytes: lr }, i))
                };
                if id_bit == cmp_bit && !chosen {
                    last_zero = bit_index;
                }
                set_bit(&mut rom, i, chosen);
                candidates.retain(|r| bit_at(r, i) == chosen);
            }
            last_discrepancy = last_zero;
            if family.is_none() || family == Some(rom[0]) {
                return Some((RomId { bytes: rom }, last_discrepancy));
            }
            if last_discrepancy == 0 {
                return None;
            }
            last_rom = Some(rom);
        }
    }

    mod server {
        use super::*;

        pub struct OneWireServer {
            pub bus: Arc<RwLock<Bus>>,
        }

        impl OneWireServer {
            fn onewire_transaction(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                t: ww_onewire::OneWireTransaction<'_>,
            ) -> RpcResult<Result<MaxLen<RefVec<'_, u8>, { ww_onewire::OW_MAX }>, OneWireError>>
            {
                let mut bus = self.bus.write().unwrap();
                if bus.roms.is_empty() {
                    return Ready(Err(OneWireError::NoPresence));
                }
                if let Some(rom) = t.rom
                    && !bus.roms.contains(&rom)
                {
                    return Ready(Err(OneWireError::NoPresence));
                }
                let write = t.write.as_slice();
                let read_len = t.read_len as usize;
                if write.len() + read_len > ww_onewire::OW_MAX {
                    return Ready(Err(OneWireError::InvalidArgument));
                }
                // `Search ROM` (0xF0) with no read starts a bit-level search session for `triplet`.
                if write == [0xF0] && read_len == 0 {
                    bus.triplet_session = Some(TripletSession {
                        bit_index: 0,
                        accum: [0; 8],
                    });
                }
                // Mock scratchpad: echoes the write back, zero-padded/truncated to `read_len`.
                let mut reply = vec![0u8; read_len];
                for (r, w) in reply.iter_mut().zip(write.iter()) {
                    *r = *w;
                }
                Ready(Ok(MaxLen::new(RefVec::Slice {
                    slice: Box::leak(reply.into_boxed_slice()),
                })
                .unwrap()))
            }

            fn onewire_reset(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<Result<bool, OneWireError>> {
                let mut bus = self.bus.write().unwrap();
                bus.triplet_session = None;
                Ready(Ok(!bus.roms.is_empty()))
            }

            fn onewire_search(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                from: Option<SearchState>,
                family: Option<u8>,
            ) -> RpcResult<Result<ww_onewire::SearchPage<'_>, OneWireError>> {
                let bus = self.bus.read().unwrap();
                if bus.roms.is_empty() {
                    return Ready(Err(OneWireError::NoPresence));
                }
                let (last_discrepancy, last_rom) = match from {
                    Some(s) => (s.last_discrepancy, Some(s.last_rom.bytes)),
                    None => (0, None),
                };
                let result = do_search(&bus.roms, last_discrepancy, last_rom, family);
                drop(bus);
                match result {
                    Some((rom, new_last_discrepancy)) => {
                        let next = if new_last_discrepancy == 0 {
                            None
                        } else {
                            Some(SearchState {
                                last_rom: rom,
                                last_discrepancy: new_last_discrepancy,
                            })
                        };
                        Ready(Ok(ww_onewire::SearchPage {
                            roms: MaxLen::new(RefVec::Slice {
                                slice: Box::leak(vec![rom].into_boxed_slice()),
                            })
                            .unwrap(),
                            next,
                        }))
                    }
                    None => Ready(Ok(ww_onewire::SearchPage {
                        roms: MaxLen::new(RefVec::new()).unwrap(),
                        next: None,
                    })),
                }
            }

            fn onewire_capabilities(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<ww_onewire::OneWireCapabilities<'_>> {
                const PINS: &[MaxLen<&str, 16>] =
                    &[MaxLen::new_unchecked("A"), MaxLen::new_unchecked("B")];
                Ready(ww_onewire::OneWireCapabilities {
                    pins: MaxLen::new(RefVec::Slice { slice: PINS }).unwrap(),
                    overdrive: true,
                    strong_pullup: true,
                })
            }

            fn onewire_configure(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                config: OneWireConfig,
            ) -> RpcResult<Result<(), OneWireError>> {
                if config.pin > 1 {
                    return Ready(Err(OneWireError::InvalidArgument));
                }
                self.bus.write().unwrap().configured = Some(config);
                Ready(Ok(()))
            }

            fn onewire_triplet(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                direction: bool,
            ) -> RpcResult<Result<ww_onewire::Triplet, OneWireError>> {
                let mut bus = self.bus.write().unwrap();
                let Bus {
                    roms,
                    triplet_session,
                    ..
                } = &mut *bus;
                let Some(session) = triplet_session.as_mut() else {
                    return Ready(Err(OneWireError::WrongMode));
                };
                if session.bit_index >= 64 {
                    return Ready(Err(OneWireError::WrongMode));
                }
                let i = session.bit_index;
                let prefix = RomId {
                    bytes: session.accum,
                };
                let candidates: Vec<RomId> = roms
                    .iter()
                    .copied()
                    .filter(|r| (0..i).all(|b| bit_at(r, b) == bit_at(&prefix, b)))
                    .collect();
                let (id_bit, cmp_bit) = triplet_bits(&candidates, i);
                let chosen = if id_bit != cmp_bit { id_bit } else { direction };
                set_bit(&mut session.accum, i, chosen);
                session.bit_index += 1;
                let result = ww_onewire::Triplet {
                    id_bit,
                    cmp_bit,
                    direction: chosen,
                };
                if session.bit_index >= 64 {
                    *triplet_session = None;
                }
                Ready(Ok(result))
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                io_onewire_api :: IoOneWireTest for super::OneWireServer,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
                // debug_to_file = "../../target/tests_io_onewire_server.rs"
            );
        }

        impl tests_common::TestProcessEvents for OneWireServer {
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

    mod client {
        use wire_weaver_client::{ClientConfig, Commander, WwClient};

        pub struct OneWireClient {
            pub cmd: Commander,
        }

        impl WwClient for OneWireClient {
            fn default_config() -> ClientConfig {
                ClientConfig::default()
            }

            fn from_cmd(cmd: Commander) -> Self {
                Self { cmd }
            }
        }

        mod api_client {
            wire_weaver::ww_codegen!(
                io_onewire_api :: IoOneWireTest for super::OneWireClient,
                client = "std_client",
                // debug_to_file = "../../target/tests_io_onewire_client.rs"
            );
        }
    }

    async fn connect(
        name: &str,
        roms: Vec<RomId>,
    ) -> (
        client::OneWireClient,
        Arc<RwLock<Bus>>,
        tests_common::TestDevice,
    ) {
        let _ = tracing_subscriber::fmt::try_init();
        let bus = Arc::new(RwLock::new(Bus::with_roms(roms)));
        let server = server::OneWireServer { bus: bus.clone() };
        let device = tests_common::start_device(
            name,
            server,
            io_onewire_api::IO_ONE_WIRE_TEST_FULL_GID,
            server::api_impl::api_hash(),
        );
        let client = client::OneWireClient::config(|c| c.in_process_path(device.path()))
            .connect()
            .await
            .expect("connect");
        (client, bus, device)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reset_and_transaction() {
        let (client, _bus, _device) = connect("onewire/reset_and_transaction", vec![ROM_A]).await;
        let mut onewire = client.onewire();

        assert!(onewire.reset().call().await.unwrap());

        let reply = onewire
            .transaction(OneWireTransactionOwned {
                rom: Some(ROM_A),
                write: MaxLen::new(vec![0x44]).unwrap(),
                read_len: 1,
                strong_pullup: None,
            })
            .call()
            .await
            .unwrap();
        assert_eq!(&*reply, &[0x44]);

        // Selecting a ROM id that is not on the bus is refused.
        let err = onewire
            .transaction(OneWireTransactionOwned {
                rom: Some(ROM_B),
                write: MaxLen::new(vec![]).unwrap(),
                read_len: 0,
                strong_pullup: None,
            })
            .call()
            .await;
        assert_eq!(err.unwrap_err().device(), Some(&OneWireError::NoPresence));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reset_no_devices() {
        let (client, _bus, _device) = connect("onewire/reset_no_devices", vec![]).await;
        assert!(!client.onewire().reset().call().await.unwrap());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn search_pages_and_filters_by_family() {
        let (client, _bus, _device) = connect("onewire/search", vec![ROM_A, ROM_B, ROM_C]).await;
        let mut onewire = client.onewire();

        let mut found = Vec::new();
        let mut from = None;
        loop {
            let page = onewire.search(from, None).call().await.unwrap();
            found.extend(page.roms.iter().copied());
            match page.next {
                Some(next) => from = Some(next),
                None => break,
            }
        }
        found.sort_by_key(|r| r.bytes);
        let mut expected = vec![ROM_A, ROM_B, ROM_C];
        expected.sort_by_key(|r| r.bytes);
        assert_eq!(found, expected);

        // Filtered by family: only the two FAMILY_A ids come back.
        let mut found = Vec::new();
        let mut from = None;
        loop {
            let page = onewire.search(from, Some(FAMILY_A)).call().await.unwrap();
            found.extend(page.roms.iter().copied());
            match page.next {
                Some(next) => from = Some(next),
                None => break,
            }
        }
        found.sort_by_key(|r| r.bytes);
        let mut expected = vec![ROM_A, ROM_B];
        expected.sort_by_key(|r| r.bytes);
        assert_eq!(found, expected);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn search_no_devices() {
        let (client, _bus, _device) = connect("onewire/search_empty", vec![]).await;
        let err = client.onewire().search(None, None).call().await;
        assert!(matches!(
            err.unwrap_err().device(),
            Some(OneWireError::NoPresence)
        ));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn capabilities_and_configure() {
        let (client, bus, _device) = connect("onewire/capabilities", vec![ROM_A]).await;
        let mut onewire = client.onewire();

        let caps = onewire.capabilities().call().await.unwrap();
        assert_eq!(caps.pins.len(), 2);
        assert!(caps.overdrive);
        assert!(caps.strong_pullup);

        onewire
            .configure(OneWireConfig {
                enabled: true,
                pin: 1,
                speed: OneWireSpeed::Overdrive,
            })
            .call()
            .await
            .unwrap();
        assert_eq!(bus.read().unwrap().configured.unwrap().pin, 1);

        let err = onewire
            .configure(OneWireConfig {
                enabled: true,
                pin: 7,
                speed: OneWireSpeed::Standard,
            })
            .call()
            .await;
        assert_eq!(
            err.unwrap_err().device(),
            Some(&OneWireError::InvalidArgument)
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn triplet_driven_search_finds_the_only_device() {
        let (client, _bus, _device) = connect("onewire/triplet", vec![ROM_A]).await;
        let mut onewire = client.onewire();

        assert!(onewire.reset().call().await.unwrap());
        // Send the `Search ROM` command to start a bit-level search session.
        onewire
            .transaction(OneWireTransactionOwned {
                rom: None,
                write: MaxLen::new(vec![0xF0]).unwrap(),
                read_len: 0,
                strong_pullup: None,
            })
            .call()
            .await
            .unwrap();

        let mut found = [0u8; 8];
        for i in 0..64u8 {
            let t = onewire.triplet(false).call().await.unwrap();
            // With a single device on the bus every bit agrees, no fork.
            assert_eq!(t.id_bit, t.direction);
            assert_ne!(t.id_bit, t.cmp_bit);
            if t.direction {
                found[(i / 8) as usize] |= 1 << (i % 8);
            }
        }
        assert_eq!(found, ROM_A.bytes);

        // 65th call without a new reset/search command is refused.
        let err = onewire.triplet(false).call().await;
        assert!(matches!(
            err.unwrap_err().device(),
            Some(OneWireError::WrongMode)
        ));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn triplet_without_search_command_is_refused() {
        let (client, _bus, _device) = connect("onewire/triplet_wrong_mode", vec![ROM_A]).await;
        let err = client.onewire().triplet(false).call().await;
        assert!(matches!(
            err.unwrap_err().device(),
            Some(OneWireError::WrongMode)
        ));
    }
}
