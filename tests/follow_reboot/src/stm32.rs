//! The same sequence as [crate::Report] for an STM32: the app reboots into the system bootloader
//! (`reboot(Updater)`), which comes back as a simulated `0483:df11` under the same serial number
//! ([ww_stm32_dfu::rom_serial] of the chip's unique id), takes a new image over DfuSe, jumps to flash and boots it.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::mpsc;
use wire_weaver_client::follow::Bus;
use wire_weaver_client::{ApiInfo, DeviceInfo, UsbLocation};
use ww_dfu::BootTarget;
use ww_stm32_dfu::proto::{ROM_DFU_PID, ST_VID};
use ww_stm32_dfu::sim::SimStm32Dfu;

use crate::{APP_PID, APP_VID, App, Mode, REENUMERATE, bootable_with, run_app};

pub const FLASH: u32 = 0x0800_0000;

pub struct Stm32Report {
    pub serial: String,
    pub sim: SimStm32Dfu,
    mode: Arc<Mutex<Mode>>,
    boots: mpsc::UnboundedReceiver<Mode>,
    _stop: mpsc::UnboundedSender<()>,
}

impl Stm32Report {
    /// Start a device with unique id `uid` and `flash` at the start of flash.
    pub fn start(uid: [u32; 3], flash: &[u8]) -> Stm32Report {
        let sim = SimStm32Dfu::h72x(uid);
        sim.preload(FLASH, flash);
        let serial = sim.serial();
        let mode = Arc::new(Mutex::new(Mode::Off));
        let (boots_tx, boots) = mpsc::unbounded_channel();
        let (stop_tx, stop_rx) = mpsc::unbounded_channel();
        let (t_sim, t_mode, t_serial) = (sim.clone(), mode.clone(), serial.clone());
        std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(device_thread(t_serial, t_sim, t_mode, boots_tx, stop_rx))
        });
        Stm32Report {
            serial,
            sim,
            mode,
            boots,
            _stop: stop_tx,
        }
    }

    pub fn bus(&self) -> Stm32Bus {
        Stm32Bus {
            serial: self.serial.clone(),
            mode: self.mode.clone(),
            sim: self.sim.clone(),
        }
    }

    /// Wait for the next boot, returns what runs.
    pub async fn booted(&mut self) -> Mode {
        tokio::time::timeout(Duration::from_secs(5), self.boots.recv())
            .await
            .expect("device boots")
            .expect("device thread runs")
    }
}

async fn device_thread(
    serial: String,
    sim: SimStm32Dfu,
    mode: Arc<Mutex<Mode>>,
    boots: mpsc::UnboundedSender<Mode>,
    mut stop: mpsc::UnboundedReceiver<()>,
) {
    let mut to_rom = false;
    loop {
        tokio::time::sleep(REENUMERATE).await;
        // no intact image: the system bootloader stays in DFU, as a blank chip does
        let image = if to_rom {
            None
        } else {
            bootable_with(|a, n| sim.memory(a, n), FLASH)
        };
        if let Some(version) = image {
            *mode.lock().unwrap() = Mode::App(version);
            _ = boots.send(Mode::App(version));
            let Some(into) = run_app(App::new(version), &serial, &mut stop).await else {
                return;
            };
            *mode.lock().unwrap() = Mode::Off;
            if into == BootTarget::App {
                continue;
            }
            tokio::time::sleep(REENUMERATE).await;
        }
        sim.reset();
        *mode.lock().unwrap() = Mode::Bootsel;
        _ = boots.send(Mode::Bootsel);
        // in DFU until the host leaves it
        while sim.left().is_none() {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(5)) => {}
                _ = stop.recv() => return,
            }
        }
        *mode.lock().unwrap() = Mode::Off;
        to_rom = false;
    }
}

#[derive(Clone)]
pub struct Stm32Bus {
    serial: String,
    mode: Arc<Mutex<Mode>>,
    sim: SimStm32Dfu,
}

impl Stm32Bus {
    /// Open the DFU interface of the bootloader `info`.
    pub fn open(&self, info: &DeviceInfo) -> Result<SimStm32Dfu, ww_stm32_dfu::Error> {
        let in_dfu = *self.mode.lock().unwrap() == Mode::Bootsel;
        if in_dfu && info.serials.contains(&self.serial) {
            Ok(self.sim.clone())
        } else {
            Err(ww_stm32_dfu::Error::Other(format!("{info} is gone")))
        }
    }
}

impl Bus for Stm32Bus {
    async fn list(&mut self) -> anyhow::Result<Vec<DeviceInfo>> {
        let usb = |vid, pid| {
            Some(UsbLocation {
                bus_id: "1".into(),
                port_chain: vec![3],
                vid,
                pid,
            })
        };
        let mode = *self.mode.lock().unwrap();
        Ok(match mode {
            Mode::Off => vec![],
            Mode::App(_) => vec![DeviceInfo {
                location: format!("sim {APP_VID:04x}:{APP_PID:04x}"),
                manufacturer: "vhrd.tech".into(),
                product: "sim B135".into(),
                serials: vec![self.serial.clone()],
                user_label: String::new(),
                api: Some(ApiInfo {
                    gid: "dfu_lean_app_api".into(),
                    version: semver::Version::new(0, 1, 0),
                    signature: wire_weaver_client::ww_version::ApiHashOwned { hash: vec![] },
                }),
                usb: usb(APP_VID, APP_PID),
            }],
            Mode::Bootsel => vec![DeviceInfo {
                location: format!("sim {ST_VID:04x}:{ROM_DFU_PID:04x}"),
                manufacturer: "STMicroelectronics".into(),
                product: "STM32  BOOTLOADER".into(),
                serials: vec![self.serial.clone()],
                user_label: String::new(),
                api: None,
                usb: usb(ST_VID, ROM_DFU_PID),
            }],
        })
    }
}
