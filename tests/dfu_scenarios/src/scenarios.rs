//! One end-to-end test per scenario of `docs/std_library/ww_dfu.md`. Each runs over one link (a one-line `LINK`
//! choice), records the exchange on the device side and checks it against its golden transcript, which the docs
//! include (`just dfu-transcripts` regenerates them).

use wire_weaver::prelude::*;
use ww_dfu::{
    BootState, BootTarget, BulkStatus, ChunkState, Outcome, Profile, Role, Slot, SlotSwitch, Target,
};

use crate::device::{Device, Link};
use crate::flash::{Cut, Firmware, Flash};
use crate::host::app::AppClient;
use crate::host::lean_app::LeanAppClient;
use crate::host::updater::UpdaterClient;
use crate::plan::{CHUNK, Plan, chunk_hash, v1, v2};
use crate::record::check_or_update;
use crate::{connect, resume_bulk, send_bulk, send_steps};

/// Run a scenario on one thread, host and device side by side as tasks of one runtime: the device as single-threaded
/// as real firmware, and every run of a scenario the same interleaving of host and device.
fn run(scenario: impl Future<Output = ()>) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    tokio::task::LocalSet::new().block_on(&rt, scenario);
}

/// Start the device and wait until it runs `fw`.
///
/// Over the simulated USB everything is inside the runtime, so time is paused: the clock jumps ahead whenever all
/// tasks wait, timeouts don't depend on how loaded the machine is, and a timeout (the host's or the device's) only
/// fires when nothing else can happen. UDP and WebSocket go through real sockets the runtime can't see into, a
/// paused clock could jump past a datagram still in the kernel, so they keep the real clock.
async fn start(code: &str, link: Link, flash: Flash, fw: Firmware) -> Device {
    let _ = tracing_subscriber::fmt::try_init();
    if matches!(link, Link::UsbFullSpeed | Link::UsbHighSpeed) {
        tokio::time::pause();
    }
    let mut dev = Device::start(code, link, flash).await;
    assert_eq!(dev.booted().await, fw);
    dev
}

/// Plan steps `..to`, to send part of a plan with `send_steps!`.
fn first_steps(plan: &Plan, to: usize) -> Plan {
    Plan {
        steps: plan.steps[..to].to_vec(),
        ..plan.clone()
    }
}

fn header(plan: &Plan) -> MaxLen<Vec<u8>, { ww_dfu::MAX_HEADER_LEN }> {
    MaxLen::new(plan.header.clone()).unwrap()
}

/// 2C-APP-STEP: A/B with a copying bootloader, the app receives step by step.
#[test]
fn swap_shuttle_2c_app_step() {
    run(async {
        const CODE: &str = "2C-APP-STEP";
        const LINK: Link = Link::UsbFullSpeed;
        let mut dev = start(
            CODE,
            LINK,
            Flash::new(Profile::AB(SlotSwitch::Copy), &v1(), 0),
            Firmware::App,
        )
        .await;
        let host = connect!(AppClient, &dev.addr);

        let node = host.node().node().call().await.unwrap();
        assert_eq!(node.profile, Profile::AB(SlotSwitch::Copy));
        assert_eq!(node.app_version, Some(1));
        let mut stage = host.stage();
        let limits = stage.limits().call().await.unwrap();

        let plan = Plan::ab(&v1(), &v2());
        let session = stage
            .begin(Target::Live, header(&plan))
            .call()
            .await
            .unwrap();
        assert_eq!(
            (session.steps as usize, session.resumed),
            (plan.steps.len(), false)
        );
        send_steps!(stage, plan, 0, limits.max_piece);
        dev.rec
            .note("host resends step 5 as if its reply was lost: skipped");
        send_steps!(stage, first_steps(&plan, 6), 5, limits.max_piece);
        assert_eq!(stage.finish().call().await.unwrap(), BootState::Staged);
        host.boot().reboot(BootTarget::App).call().await.unwrap();
        drop(host);

        assert_eq!(dev.booted().await, Firmware::App);
        dev.rec.note("host reconnects to the same device");
        let host = connect!(AppClient, &dev.addr);
        let node = host.node().node().call().await.unwrap();
        assert_eq!(
            (node.app_version, node.boot_state),
            (Some(2), BootState::Trial)
        );
        host.boot().confirm().call().await.unwrap();
        let node = host.node().node().call().await.unwrap();
        assert_eq!(node.boot_state, BootState::Confirmed);
        assert_eq!(node.last_result.outcome, Outcome::Confirmed);
        check_or_update(CODE, &dev.rec.transcript(LINK.units()));
    })
}

/// 2C-APP-BULK: A/B with a copying bootloader, the app receives the plan in one go; the host restarts half way.
#[test]
fn swap_freight_2c_app_bulk() {
    run(async {
        const CODE: &str = "2C-APP-BULK";
        const LINK: Link = Link::Udp;
        let mut dev = start(
            CODE,
            LINK,
            Flash::new(Profile::AB(SlotSwitch::Copy), &v1(), 0),
            Firmware::App,
        )
        .await;
        let host = connect!(AppClient, &dev.addr);
        let node = host.node().node().call().await.unwrap();
        assert!(node.offers.stage_bulk);
        let max_piece = host.stage().limits().call().await.unwrap().max_piece;

        let plan = Plan::ab(&v1(), &v2());
        let total = plan.body.len() as u32;
        let mut bulk = host.bulk();
        let session = bulk
            .begin_bulk(Target::Live, header(&plan), total, 0)
            .call()
            .await
            .unwrap();
        assert_eq!(session.resume_from, 0);
        let half = 3 * max_piece;
        let sink = send_bulk!(bulk, plan, 0, half, max_piece);
        // saved with the host's own progress: the plan sink's session and the segments the device accepted
        let cursor = sink.cursor().unwrap();
        drop(sink);
        dev.rec.note("host restarts; the device keeps its session");
        host.disconnect().asynch().await.unwrap();
        drop(host);

        let host = connect!(AppClient, &dev.addr);
        let mut bulk = host.bulk();
        let session = bulk
            .begin_bulk(Target::Live, header(&plan), total, 0)
            .call()
            .await
            .unwrap();
        assert_eq!((session.resumed, session.resume_from), (true, half));
        let mut acks = bulk.plan_ack().call().await.unwrap();
        dev.rec
            .note("the plan sink continues its session where the device is");
        let (_sink, at) = resume_bulk!(bulk, cursor, plan, session.resume_from, total, max_piece);
        assert_eq!(at, 3, "segments the device has");
        let ack = acks.recv().await.unwrap();
        assert_eq!((ack.have, ack.status), (total, BulkStatus::Complete));
        assert_eq!(
            host.stage().finish().call().await.unwrap(),
            BootState::Staged
        );
        host.boot().reboot(BootTarget::App).call().await.unwrap();
        drop(acks);
        drop(_sink);
        drop(host);

        assert_eq!(dev.booted().await, Firmware::App);
        let host = connect!(AppClient, &dev.addr);
        host.boot().confirm().call().await.unwrap();
        let node = host.node().node().call().await.unwrap();
        assert_eq!(
            (node.app_version, node.boot_state),
            (Some(2), BootState::Confirmed)
        );
        check_or_update(CODE, &dev.rec.transcript(LINK.units()));
    })
}

/// 2R-APP-STEP: A/B with bank remap, the app receives a plain full image step by step.
#[test]
fn flip_shuttle_2r_app_step() {
    run(async {
        const CODE: &str = "2R-APP-STEP";
        const LINK: Link = Link::WebSocket;
        let mut dev = start(
            CODE,
            LINK,
            Flash::new(Profile::AB(SlotSwitch::Remap), &v1(), 0),
            Firmware::App,
        )
        .await;
        let host = connect!(AppClient, &dev.addr);
        let node = host.node().node().call().await.unwrap();
        assert_eq!(node.profile, Profile::AB(SlotSwitch::Remap));
        let mut stage = host.stage();
        let limits = stage.limits().call().await.unwrap();

        dev.rec
            .note("host sends a plain full image: ERASE and WRITE only, no manifest needed");
        let plan = Plan::full(&v2());
        stage
            .begin(Target::Live, header(&plan))
            .call()
            .await
            .unwrap();
        send_steps!(stage, plan, 0, limits.max_piece);
        assert_eq!(stage.finish().call().await.unwrap(), BootState::Staged);
        host.boot().reboot(BootTarget::App).call().await.unwrap();
        drop(host);

        assert_eq!(dev.booted().await, Firmware::App);
        let host = connect!(AppClient, &dev.addr);
        assert_eq!(
            host.boot().boot_state().call().await.unwrap(),
            BootState::Trial
        );
        host.boot().confirm().call().await.unwrap();
        let node = host.node().node().call().await.unwrap();
        assert_eq!(
            (node.app_version, node.boot_state),
            (Some(2), BootState::Confirmed)
        );
        assert_eq!(dev.flash.lock().unwrap().run_bank, 1);
        check_or_update(CODE, &dev.rec.transcript(LINK.units()));
    })
}

/// 2R-APP-BULK: A/B with bank remap, the app receives the plan in one go; the new image doesn't confirm and is
/// reverted.
#[test]
fn flip_freight_2r_app_bulk() {
    run(async {
        const CODE: &str = "2R-APP-BULK";
        const LINK: Link = Link::UsbHighSpeed;
        let mut dev = start(
            CODE,
            LINK,
            Flash::new(Profile::AB(SlotSwitch::Remap), &v1(), 0),
            Firmware::App,
        )
        .await;
        let host = connect!(AppClient, &dev.addr);
        host.node().node().call().await.unwrap();
        let max_piece = host.stage().limits().call().await.unwrap().max_piece;

        let plan = Plan::ab(&v1(), &v2());
        let total = plan.body.len() as u32;
        let mut bulk = host.bulk();
        bulk.begin_bulk(Target::Live, header(&plan), total, 0)
            .call()
            .await
            .unwrap();
        let mut acks = bulk.plan_ack().call().await.unwrap();
        let sink = send_bulk!(bulk, plan, 0, total, max_piece);
        let ack = acks.recv().await.unwrap();
        assert_eq!(ack.status, BulkStatus::Complete);
        assert_eq!(
            host.stage().finish().call().await.unwrap(),
            BootState::Staged
        );
        host.boot().reboot(BootTarget::App).call().await.unwrap();
        drop(acks);
        drop(sink);
        drop(host);

        assert_eq!(dev.booted().await, Firmware::App);
        let host = connect!(AppClient, &dev.addr);
        let node = host.node().node().call().await.unwrap();
        assert_eq!(
            (node.app_version, node.boot_state),
            (Some(2), BootState::Trial)
        );
        dev.rec
            .note("the new image fails its own checks: gives up the trial instead of confirming");
        host.boot().revert().call().await.unwrap();
        host.boot().reboot(BootTarget::App).call().await.unwrap();
        drop(host);

        assert_eq!(dev.booted().await, Firmware::App);
        let host = connect!(AppClient, &dev.addr);
        let node = host.node().node().call().await.unwrap();
        assert_eq!(
            (node.app_version, node.boot_state),
            (Some(1), BootState::Reverted)
        );
        assert_eq!(
            (node.last_result.outcome, node.last_result.version),
            (Outcome::Reverted, 2)
        );
        check_or_update(CODE, &dev.rec.transcript(LINK.units()));
    })
}

/// 1S-APP-STEP: one slot, the app stores the plan step by step, the bootloader applies it; the power fails while it
/// does.
#[test]
fn parcel_shuttle_1s_app_step() {
    run(async {
        const CODE: &str = "1S-APP-STEP";
        const LINK: Link = Link::UsbFullSpeed;
        let mut dev = start(
            CODE,
            LINK,
            Flash::new(Profile::InPlace, &v1(), 1024),
            Firmware::App,
        )
        .await;
        let host = connect!(AppClient, &dev.addr);
        let node = host.node().node().call().await.unwrap();
        assert_eq!(node.profile, Profile::InPlace);

        dev.rec
            .note("host doesn't know this manifest digest: reads the manifest");
        let mut inventory = host.inventory();
        let layout = inventory.layout(Slot::Active).call().await.unwrap();
        let chunks = inventory
            .chunks(Slot::Active, 0, layout.chunk_count)
            .call()
            .await
            .unwrap();
        let expected: Vec<ChunkState> = v1()
            .chunks(CHUNK)
            .map(|c| {
                if crate::plan::is_blank(c) {
                    ChunkState::Blank
                } else {
                    ChunkState::Hash(chunk_hash(c))
                }
            })
            .collect();
        assert_eq!(*chunks, expected);

        let mut stage = host.stage();
        let limits = stage.limits().call().await.unwrap();
        assert!(limits.stored && limits.plan_area >= 512);
        let plan = Plan::in_place(&v1(), &v2());
        stage
            .begin(Target::Stored, header(&plan))
            .call()
            .await
            .unwrap();
        send_steps!(stage, plan, 0, limits.max_piece);
        assert_eq!(stage.finish().call().await.unwrap(), BootState::PlanStored);
        dev.flash.lock().unwrap().cut = Some(Cut::Apply(5));
        host.boot().reboot(BootTarget::App).call().await.unwrap();
        drop(host);

        assert_eq!(dev.booted().await, Firmware::App);
        let host = connect!(AppClient, &dev.addr);
        let node = host.node().node().call().await.unwrap();
        assert_eq!(
            (node.app_version, node.boot_state),
            (Some(2), BootState::Confirmed)
        );
        assert_eq!(
            (node.last_result.outcome, node.last_result.version),
            (Outcome::Confirmed, 2)
        );
        check_or_update(CODE, &dev.rec.transcript(LINK.units()));
    })
}

/// 1S-APP-BULK: one slot, the app stores the plan it receives in one go and keeps working, the bootloader applies it.
#[test]
fn parcel_freight_1s_app_bulk() {
    run(async {
        const CODE: &str = "1S-APP-BULK";
        const LINK: Link = Link::Udp;
        let mut dev = start(
            CODE,
            LINK,
            Flash::new(Profile::InPlace, &v1(), 1024),
            Firmware::App,
        )
        .await;
        let host = connect!(AppClient, &dev.addr);
        host.node().node().call().await.unwrap();
        let max_piece = host.stage().limits().call().await.unwrap().max_piece;

        let plan = Plan::in_place(&v1(), &v2());
        let total = plan.body.len() as u32;
        let mut bulk = host.bulk();
        bulk.begin_bulk(Target::Stored, header(&plan), total, 0)
            .call()
            .await
            .unwrap();
        let mut acks = bulk.plan_ack().call().await.unwrap();
        let half = 4 * max_piece;
        let sink = send_bulk!(bulk, plan, 0, half, max_piece);
        drop(sink);
        dev.rec
            .note("half way: the app keeps doing its job and answers anything else");
        let node = host.node().node().call().await.unwrap();
        assert_eq!(
            (node.app_version, node.boot_state),
            (Some(1), BootState::Confirmed)
        );
        let _sink = send_bulk!(bulk, plan, half, total, max_piece);
        let ack = acks.recv().await.unwrap();
        assert_eq!((ack.have, ack.status), (total, BulkStatus::Complete));
        assert_eq!(
            host.stage().finish().call().await.unwrap(),
            BootState::PlanStored
        );
        host.boot().reboot(BootTarget::App).call().await.unwrap();
        drop(acks);
        drop(_sink);
        drop(host);

        assert_eq!(dev.booted().await, Firmware::App);
        let host = connect!(AppClient, &dev.addr);
        let node = host.node().node().call().await.unwrap();
        assert_eq!(
            (node.app_version, node.boot_state),
            (Some(2), BootState::Confirmed)
        );
        check_or_update(CODE, &dev.rec.transcript(LINK.units()));
    })
}

/// 1L-UPD-STEP: one slot, the app reboots into the updater, which receives and applies step by step; the power fails
/// half way and the update resumes.
#[test]
fn pit_stop_shuttle_1l_upd_step() {
    run(async {
        const CODE: &str = "1L-UPD-STEP";
        const LINK: Link = Link::UsbFullSpeed;
        let mut dev = start(
            CODE,
            LINK,
            Flash::new(Profile::InPlace, &v1(), 0),
            Firmware::LeanApp,
        )
        .await;
        let app = connect!(LeanAppClient, &dev.addr);
        let node = app.node().node().call().await.unwrap();
        assert!(!node.offers.stage);
        app.boot().reboot(BootTarget::Updater).call().await.unwrap();
        drop(app);

        assert_eq!(dev.booted().await, Firmware::Updater);
        dev.rec
            .note("host reconnects to the same device: the updater answers");
        let host = connect!(UpdaterClient, &dev.addr);
        let node = host.node().node().call().await.unwrap();
        assert_eq!((node.role, node.app_version), (Role::Bootloader, Some(1)));
        let mut stage = host.stage();
        let limits = stage.limits().call().await.unwrap();
        assert_eq!(limits.max_piece, 32);
        let plan = Plan::in_place(&v1(), &v2());
        stage
            .begin(Target::Live, header(&plan))
            .call()
            .await
            .unwrap();
        dev.flash.lock().unwrap().cut = Some(Cut::Download(7));
        send_steps!(stage, first_steps(&plan, 7), 0, limits.max_piece);
        let crate::plan::Step::Write { dst, hash, data } = &plan.steps[7] else {
            panic!("step 7 is a WRITE");
        };
        stage
            .write(7, *dst, *hash, 0, MaxLen::new(data[..32].to_vec()).unwrap())
            .call()
            .await
            .unwrap();
        assert!(
            stage
                .write(
                    7,
                    *dst,
                    *hash,
                    32,
                    MaxLen::new(data[32..].to_vec()).unwrap()
                )
                .call()
                .await
                .is_err(),
            "no reply: the power is gone"
        );
        drop(host);

        assert_eq!(dev.booted().await, Firmware::Updater);
        dev.rec
            .note("host: the call fails (link lost), reconnects when the device is back");
        let host = connect!(UpdaterClient, &dev.addr);
        let node = host.node().node().call().await.unwrap();
        assert_eq!(
            (node.app_version, node.boot_state),
            (None, BootState::Updating)
        );
        let mut stage = host.stage();
        let session = stage
            .begin(Target::Live, header(&plan))
            .call()
            .await
            .unwrap();
        assert_eq!((session.resumed, session.resume_from), (true, 7));
        send_steps!(stage, plan, 7, limits.max_piece);
        assert_eq!(stage.finish().call().await.unwrap(), BootState::Confirmed);
        host.boot().reboot(BootTarget::App).call().await.unwrap();
        drop(host);

        assert_eq!(dev.booted().await, Firmware::LeanApp);
        let app = connect!(LeanAppClient, &dev.addr);
        let node = app.node().node().call().await.unwrap();
        assert_eq!(
            (node.app_version, node.boot_state),
            (Some(2), BootState::Confirmed)
        );
        check_or_update(CODE, &dev.rec.transcript(LINK.units()));
    })
}

/// 1L-UPD-BULK: one slot, the app reboots into the updater, which receives the plan in one go and applies it as it
/// arrives.
#[test]
fn pit_stop_freight_1l_upd_bulk() {
    run(async {
        const CODE: &str = "1L-UPD-BULK";
        const LINK: Link = Link::WebSocket;
        let mut dev = start(
            CODE,
            LINK,
            Flash::new(Profile::InPlace, &v1(), 0),
            Firmware::LeanApp,
        )
        .await;
        let app = connect!(LeanAppClient, &dev.addr);
        app.node().node().call().await.unwrap();
        app.boot().reboot(BootTarget::Updater).call().await.unwrap();
        drop(app);

        assert_eq!(dev.booted().await, Firmware::Updater);
        let host = connect!(UpdaterClient, &dev.addr);
        host.node().node().call().await.unwrap();
        let max_piece = host.stage().limits().call().await.unwrap().max_piece;
        let plan = Plan::in_place(&v1(), &v2());
        let total = plan.body.len() as u32;
        let mut bulk = host.bulk();
        bulk.begin_bulk(Target::Live, header(&plan), total, 0)
            .call()
            .await
            .unwrap();
        let mut acks = bulk.plan_ack().call().await.unwrap();
        let sink = send_bulk!(bulk, plan, 0, total, max_piece);
        let ack = acks.recv().await.unwrap();
        assert_eq!((ack.have, ack.status), (total, BulkStatus::Complete));
        assert_eq!(
            host.stage().finish().call().await.unwrap(),
            BootState::Confirmed
        );
        host.boot().reboot(BootTarget::App).call().await.unwrap();
        drop(acks);
        drop(sink);
        drop(host);

        assert_eq!(dev.booted().await, Firmware::LeanApp);
        let app = connect!(LeanAppClient, &dev.addr);
        let node = app.node().node().call().await.unwrap();
        assert_eq!(
            (node.app_version, node.last_result.outcome),
            (Some(2), Outcome::Confirmed)
        );
        check_or_update(CODE, &dev.rec.transcript(LINK.units()));
    })
}
