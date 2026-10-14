//! One end-to-end test per scenario of `docs/std_library/ww_dfu.md`. Each runs over one link (a one-line `LINK`
//! choice), records the exchange on the device side and checks it against its golden transcript, which the docs
//! include (`just dfu-transcripts` regenerates them).

use wire_weaver::prelude::*;
use ww_dfu::{BootState, BootTarget, Outcome, Profile, SlotSwitch, Target};

use crate::device::{Device, Link};
use crate::flash::{Firmware, Flash};
use crate::connect;
use crate::host::app::AppClient;
use crate::plan::{Plan, v1, v2};
use crate::record::check_or_update;
use crate::send_steps;

fn header(plan: &Plan) -> MaxLen<Vec<u8>, { ww_dfu::MAX_HEADER_LEN }> {
    MaxLen::new(plan.header.clone()).unwrap()
}

/// 2C-APP-STEP: A/B with a copying bootloader, the app receives step by step.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn swap_shuttle_2c_app_step() {
    const CODE: &str = "2C-APP-STEP";
    const LINK: Link = Link::UsbFullSpeed;
    let _ = tracing_subscriber::fmt::try_init();
    let mut dev = Device::start(
        CODE,
        LINK,
        Flash::new(Profile::AB(SlotSwitch::Copy), &v1(), 0),
    );
    assert_eq!(dev.booted().await, Firmware::App);
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
        .unwrap()
        .unwrap();
    assert_eq!((session.steps as usize, session.resumed), (plan.steps.len(), false));
    send_steps!(stage, plan, 0, limits.max_piece);
    dev.rec.note("host resends step 5 as if its reply was lost: skipped");
    send_steps!(stage, Plan { steps: plan.steps[..6].to_vec(), ..plan.clone() }, 5, limits.max_piece);
    assert_eq!(stage.finish().call().await.unwrap(), Ok(BootState::Staged));
    host.boot()
        .reboot(BootTarget::App)
        .call()
        .await
        .unwrap()
        .unwrap();
    drop(stage);
    drop(host);

    assert_eq!(dev.booted().await, Firmware::App);
    dev.rec.note("host reconnects to the same device");
    let host = connect!(AppClient, &dev.addr);
    let node = host.node().node().call().await.unwrap();
    assert_eq!((node.app_version, node.boot_state), (Some(2), BootState::Trial));
    host.boot().confirm().call().await.unwrap().unwrap();
    let node = host.node().node().call().await.unwrap();
    assert_eq!(node.boot_state, BootState::Confirmed);
    assert_eq!(node.last_result.outcome, Outcome::Confirmed);
    check_or_update(CODE, &dev.rec.transcript(LINK.units()));
}
