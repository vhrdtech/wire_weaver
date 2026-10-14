use std::time::Duration;

use wire_weaver_client::follow::{self, Follow, FollowError, Timeouts};
use wire_weaver_client::{ClientConfig, Commander, WwClient};
use ww_dfu::BootTarget;
use ww_picoboot::image::family;
use ww_picoboot::proto::{FLASH_BASE, REBOOT2_TYPE_FLASH_UPDATE, REBOOT2_TYPE_MASK};
use ww_picoboot::{Image, Stage};

use crate::{Mode, Report, app_image};

pub struct LeanAppClient {
    pub cmd: Commander,
}

impl WwClient for LeanAppClient {
    fn default_config() -> ClientConfig {
        ClientConfig::default().default_timeout(Duration::from_secs(2))
    }

    fn from_cmd(cmd: Commander) -> Self {
        Self { cmd }
    }
}

mod api_client {
    wire_weaver::ww_codegen!(
        dfu_lean_app_api :: DfuLeanApp for super::LeanAppClient,
        client = "std_client",
    );
}

async fn connect(serial: &str) -> LeanAppClient {
    LeanAppClient::config(|c| c.in_process_path(serial.to_string()))
        .connect()
        .await
        .expect("connect")
}

/// `image` as a UF2 file for the RP2350 (ARM, secure), as picotool writes it, with the RP2350-E10 dummy block first.
fn uf2(image: &[u8]) -> Vec<u8> {
    let mut blocks: Vec<(u32, u32, Vec<u8>)> =
        vec![(family::ABSOLUTE, 0x10ff_ff00, vec![0xEF; 256])];
    for (i, page) in image.chunks(256).enumerate() {
        blocks.push((
            family::RP2350_ARM_S,
            FLASH_BASE + i as u32 * 256,
            page.to_vec(),
        ));
    }
    let count = blocks.len() as u32;
    let mut file = vec![];
    for (no, (family, addr, data)) in blocks.into_iter().enumerate() {
        let mut b = vec![0u8; 512];
        for (at, v) in [
            (0, 0x0A32_4655),
            (4, 0x9E5D_5157),
            (8, 0x2000),
            (12, addr),
            (16, data.len() as u32),
            (20, no as u32),
            (24, count),
            (28, family),
            (508, 0x0AB1_6F30),
        ] {
            b[at..at + 4].copy_from_slice(&u32::to_le_bytes(v));
        }
        b[32..32 + data.len()].copy_from_slice(&data);
        file.extend(b);
    }
    file
}

/// The whole CLT-21 sequence: app v1 → reboot(Updater) → BOOTSEL under the same serial → flash v2 over PICOBOOT →
/// reboot → app v2 under the same serial.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn update_through_rp2350_bootsel() {
    let _ = tracing_subscriber::fmt::try_init();
    let mut dev = Report::start(0xE661_6408_4352_2C2F, &app_image(1, 20_000));
    assert_eq!(dev.booted().await, Mode::App(1));
    assert_eq!(dev.serial, "E661640843522C2F");

    // the host knows the device by its serial number from listing it, before anything else
    let mut bus = dev.bus();
    let serial = {
        use wire_weaver_client::follow::Bus;
        let listed = bus.list().await.unwrap();
        assert_eq!(listed.len(), 1);
        assert!(listed[0].api.is_some());
        listed[0].serials[0].clone()
    };

    let app = connect(&serial).await;
    let node = app.node().node().call().await.unwrap();
    assert_eq!(node.app_version, Some(1));
    app.boot()
        .reboot(BootTarget::Updater)
        .call()
        .await
        .unwrap()
        .unwrap();
    drop(app);

    let image = Image::from_uf2(&uf2(&app_image(2, 30_000)), &[family::RP2350_ARM_S]).unwrap();
    assert_eq!(image.len(), 30_000);
    let mut stages = vec![];
    let opener = bus.clone();
    let back = follow::rp2_update_with(
        &Follow::new(&serial),
        &mut bus,
        async |info| opener.open(info),
        &image,
        &mut |stage: Stage, done, total| {
            if done == total {
                stages.push(stage);
            }
        },
        Timeouts::default(),
    )
    .await
    .unwrap();
    assert_eq!(stages, [Stage::Erase, Stage::Write, Stage::Verify]);
    assert_eq!(back.serials, std::slice::from_ref(&serial));
    assert_eq!(back.api.as_ref().unwrap().gid, "dfu_lean_app_api");

    // the boot ROM was asked to boot the image just written, as picotool does
    let reboot = dev.sim.reboot_request().unwrap();
    assert_eq!(reboot.flags & REBOOT2_TYPE_MASK, REBOOT2_TYPE_FLASH_UPDATE);
    assert_eq!(dev.booted().await, Mode::Bootsel);
    assert_eq!(dev.booted().await, Mode::App(2));

    let app = connect(&serial).await;
    let node = app.node().node().call().await.unwrap();
    assert_eq!(node.app_version, Some(2));
}

/// The ROM's REBOOT2 is the one picotool sends after `load -x`: FLASH_UPDATE at the image's base.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reboot2_boots_the_written_image() {
    use ww_picoboot::sim::SimRp2350;
    let sim = SimRp2350::new(1, 64 * 1024);
    let mut bus = OneBootsel(sim.clone());
    let image = Image::from_bin(FLASH_BASE, app_image(3, 5000)).unwrap();
    // the device never comes back as a WireWeaver device here: the update itself succeeds, following it times out
    let err = follow::rp2_update_with(
        &Follow::new(sim.serial()),
        &mut bus,
        async |_| Ok(sim.clone()),
        &image,
        &mut (),
        Timeouts {
            to_bootsel: Duration::from_secs(1),
            to_app: Duration::from_millis(300),
        },
    )
    .await
    .unwrap_err();
    assert!(
        matches!(
            err,
            FollowError::Timeout {
                what: "no WireWeaver device",
                ..
            }
        ),
        "{err}"
    );
    assert!(err.to_string().contains("RP2350 Boot"), "{err}");
    let reboot = sim.reboot_request().unwrap();
    assert_eq!(reboot.flags & REBOOT2_TYPE_MASK, REBOOT2_TYPE_FLASH_UPDATE);
    assert_eq!(reboot.param0, FLASH_BASE);
    assert_eq!(sim.flash(FLASH_BASE, 5000), app_image(3, 5000));
}

/// A device that reboots into the app instead of BOOTSEL (old firmware ignoring the target, a typo in the host) is
/// reported with what is there instead.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_bootsel_reports_what_is_there() {
    let mut dev = Report::start(0x42, &app_image(1, 1000));
    assert_eq!(dev.booted().await, Mode::App(1));
    let mut bus = dev.bus();
    let follow = Follow::new(&dev.serial);
    let err = follow
        .back_in_bootsel(&mut bus, Duration::from_millis(300))
        .await
        .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("no BOOTSEL device within"), "{msg}");
    assert!(msg.contains("api=dfu_lean_app_api@0.1.0"), "{msg}");
}

/// Rebooting the app into itself: `gone` tells the old run from the new one.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn follow_reboot_into_the_same_app() {
    let mut dev = Report::start(0x43, &app_image(5, 1000));
    assert_eq!(dev.booted().await, Mode::App(5));
    let mut bus = dev.bus();
    let app = connect(&dev.serial).await;
    app.boot()
        .reboot(BootTarget::App)
        .call()
        .await
        .unwrap()
        .unwrap();
    drop(app);
    let follow = Follow::new(dev.serial.to_lowercase());
    follow.gone(&mut bus, Duration::from_secs(2)).await.unwrap();
    let back = follow
        .back_in_wire_weaver(&mut bus, Duration::from_secs(2))
        .await
        .unwrap();
    assert_eq!(back.serials, [dev.serial.clone()]);
    assert_eq!(dev.booted().await, Mode::App(5));
    let app = connect(&dev.serial).await;
    assert_eq!(app.node().node().call().await.unwrap().app_version, Some(5));
}

/// A bus with one BOOTSEL device that never leaves.
struct OneBootsel(ww_picoboot::sim::SimRp2350);

impl follow::Bus for OneBootsel {
    async fn list(&mut self) -> anyhow::Result<Vec<wire_weaver_client::DeviceInfo>> {
        Ok(vec![wire_weaver_client::DeviceInfo {
            location: "sim".into(),
            manufacturer: "Raspberry Pi".into(),
            product: "RP2350 Boot".into(),
            serials: vec![self.0.serial()],
            user_label: String::new(),
            api: None,
            usb: Some(wire_weaver_client::UsbLocation {
                bus_id: "1".into(),
                port_chain: vec![1],
                vid: ww_picoboot::proto::RPI_VID,
                pid: ww_picoboot::proto::RP2350_BOOTSEL_PID,
            }),
        }])
    }
}
