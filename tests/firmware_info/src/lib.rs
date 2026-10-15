//! `ww_firmware_info` end to end: a mock server on `ww_device::Server` through `tests_common::start_device`, the
//! generated client through the real host event loop.

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use tests_common::TestDevice;
    use wire_weaver::prelude::*;
    use ww_firmware_info::{
        BuildProfile, CortexMFault, Fault, GitInfo, ResetCause, ResetRecord, RiscVFault, Version,
    };

    const COMMIT: [u8; 20] = [
        0xa1, 0xb2, 0xc3, 0xd4, 0xe5, 0xf6, 0x07, 0x18, 0x29, 0x3a, 0x4b, 0x5c, 0x6d, 0x7e, 0x8f,
        0x90, 0xa1, 0xb2, 0xc3, 0xd4,
    ];
    const BUILD_ID: [u8; 20] = [7; 20];
    const STACK: [u32; 3] = [0x0800_1234, 0x0800_5678, 0x2000_0100];

    /// What the mock keeps of a crash, cleared by `clear`.
    struct Crash {
        fault: Fault,
    }

    mod server {
        use super::*;
        use tests_common::TestProcessEvents;
        use wire_weaver::BlockingEventOut;

        pub struct Server {
            pub crash: Arc<Mutex<Option<Crash>>>,
            pub boots: Arc<Mutex<u32>>,
        }

        #[allow(non_snake_case)]
        impl Server {
            fn reset_last_reset(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<ResetRecord<'_>> {
                let crash = self.crash.lock().unwrap();
                Ready(ResetRecord {
                    cause: if crash.is_some() {
                        ResetCause::Fault
                    } else {
                        ResetCause::PowerOn
                    },
                    build_id: MaxLen::new_unchecked(RefVec::new_bytes(&BUILD_ID)),
                    uptime: Some(ww_si::Second::milliseconds(1500u32)),
                    fault: crash.as_ref().map(|c| c.fault.clone()),
                    stack: MaxLen::new_unchecked(RefVec::Slice { slice: &STACK }),
                    message: MaxLen::new_unchecked(if crash.is_some() { "HardFault" } else { "" }),
                    boots_since_crash: 0,
                })
            }

            fn reset_boot_count(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<u32> {
                Ready(*self.boots.lock().unwrap())
            }

            fn reset_clear(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<()> {
                *self.crash.lock().unwrap() = None;
                *self.boots.lock().unwrap() = 0;
                Ready(())
            }
        }

        /// Served without handlers (API-36): the summary and record are Rust consts, the build id is known at link
        /// time only.
        impl api_impl::FirmwareInfoTestConsts for Server {
            const FIRMWARE_FIRMWARE: ww_firmware_info::FirmwareSummary<'static> =
                ww_firmware_info::FirmwareSummary {
                    name: MaxLen::new_unchecked("b200_usb_io"),
                    version: Version::full(0, 5, 0, Some("alpha.1"), None),
                    git: Some(GitInfo {
                        commit: MaxLen::new_unchecked(RefVec::new_bytes(&COMMIT)),
                        dirty: true,
                    }),
                    built: ww_firmware_info::DateTime::from_ymd_hms_utc_opt(
                        2026, 10, 3, 18, 20, 0, 0,
                    ),
                    profile: BuildProfile::Other(MaxLen::new_unchecked("size")),
                };

            fn firmware_build_id(&self) -> MaxLen<RefVec<'_, u8>, 32> {
                MaxLen::new_unchecked(RefVec::new_bytes(&BUILD_ID))
            }

            const FIRMWARE_BUILD_RECORD: MaxLen<RefVec<'static, u8>, 512> =
                MaxLen::new_unchecked(RefVec::new_bytes(b"\xB1\xD0record"));
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                firmware_info_api :: FirmwareInfoTest for super::Server,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
                const_model = "BUILD_ID=fn",
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
                firmware_info_api :: FirmwareInfoTest for super::StdClient,
                client = "std_client",
            );
        }
    }

    use std_client::StdClient;

    fn start(path: &str, fault: Option<Fault>) -> (TestDevice, Arc<Mutex<Option<Crash>>>) {
        let _ = tracing_subscriber::fmt::try_init();
        let crash = Arc::new(Mutex::new(fault.map(|fault| Crash { fault })));
        let srv = server::Server {
            crash: crash.clone(),
            boots: Arc::new(Mutex::new(3)),
        };
        let device = tests_common::start_device(
            path,
            srv,
            firmware_info_api::FIRMWARE_INFO_TEST_FULL_GID,
            server::api_impl::api_hash(),
        );
        (device, crash)
    }

    async fn connect(device: &TestDevice) -> StdClient {
        StdClient::config(|c| c.in_process_path(device.path()))
            .connect()
            .await
            .expect("connect")
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn summary_and_build_id() {
        let (device, _) = start("firmware_info/summary", None);
        let client = connect(&device).await;

        let fw = client.firmware().read_FIRMWARE().read().await.unwrap();
        assert_eq!(fw.name.as_str(), "b200_usb_io");
        assert_eq!(
            (fw.version.major.0, fw.version.minor.0, fw.version.patch.0),
            (0, 5, 0)
        );
        assert_eq!(fw.version.pre.as_deref(), Some("alpha.1"));
        let git = fw.git.unwrap();
        assert_eq!(git.commit.as_slice(), COMMIT);
        assert!(git.dirty);
        assert_eq!(
            fw.built,
            ww_firmware_info::DateTime::from_ymd_hms_utc_opt(2026, 10, 3, 18, 20, 0, 0)
        );
        assert_eq!(
            fw.profile,
            ww_firmware_info::BuildProfileOwned::Other(MaxLen::new_unchecked("size".to_string()))
        );

        let id = client.firmware().read_BUILD_ID().read().await.unwrap();
        assert_eq!(id.as_slice(), BUILD_ID);
        let record = client.firmware().read_BUILD_RECORD().read().await.unwrap();
        assert_eq!(record.as_slice(), b"\xB1\xD0record");
    }

    /// `build.rs` ran `ww_firmware_info::build::emit()`, `firmware_summary!()` reads what it set.
    #[test]
    fn firmware_summary_macro() {
        // a Rust const, as in a firmware's consts impl
        const FW: ww_firmware_info::FirmwareSummary<'static> =
            ww_firmware_info::firmware_summary!();
        let fw = FW;
        assert_eq!(*fw.name, "firmware_info");
        assert_eq!(fw.version, Version::new(0, 1, 0));
        // built in a git checkout: 40 hex digits became 20 bytes
        assert_eq!(fw.git.map(|g| g.commit.len()), Some(20));
        assert!(fw.built.is_some());
        assert_eq!(fw.profile, BuildProfile::Debug);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn power_on_has_no_fault() {
        let (device, _) = start("firmware_info/power_on", None);
        let client = connect(&device).await;
        let r = client.reset().last_reset().call().await.unwrap();
        assert_eq!(r.cause, ResetCause::PowerOn);
        assert!(r.fault.is_none());
        assert_eq!(r.build_id.as_slice(), BUILD_ID);
        assert_eq!(r.stack.as_slice(), STACK);
        assert_eq!(client.reset().boot_count().call().await.unwrap(), 3);
    }

    async fn fault_roundtrip(path: &str, fault: Fault) {
        let (device, crash) = start(path, Some(fault.clone()));
        let client = connect(&device).await;
        let r = client.reset().last_reset().call().await.unwrap();
        assert_eq!(r.cause, ResetCause::Fault);
        assert_eq!(r.message.as_str(), "HardFault");
        assert_eq!(r.fault.clone(), Some(fault));
        assert!(r.uptime.is_some());

        client.reset().clear().call().await.unwrap();
        assert!(crash.lock().unwrap().is_none());
        assert_eq!(client.reset().boot_count().call().await.unwrap(), 0);
        let r = client.reset().last_reset().call().await.unwrap();
        assert!(r.fault.is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cortex_m_fault_and_clear() {
        let fault = Fault::CortexM(CortexMFault {
            pc: 0x0800_1234,
            lr: 0x0800_5679,
            xpsr: 0x6100_0000,
            sp: 0x2000_3ff0,
            cfsr: 0x0000_0100,
            hfsr: 0x4000_0000,
            mmfar: 0,
            bfar: 0xdead_beef,
            exc_return: 0xffff_fffd,
        });
        fault_roundtrip("firmware_info/cortex_m", fault).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn riscv_fault_and_clear() {
        let fault = Fault::RiscV(RiscVFault {
            mepc: 0x1000_0040,
            mcause: 5,
            mtval: 0x2000_0000,
            ra: 0x1000_0100,
            sp: 0x2008_0000,
        });
        fault_roundtrip("firmware_info/riscv", fault).await;
    }
}
