use super::*;
use crate::mock::{ALL, MockBoard, Net};
use crate::plan::{Plan, save_to};
use crate::select::select;

const INPUT_ONLY: PointCaps = PointCaps {
    drive_low: false,
    drive_high: false,
    weak_drive: false,
    ..ALL
};
const DRIVE_ONLY: PointCaps = PointCaps {
    read: false,
    pull_up: false,
    pull_down: false,
    drive_low: true,
    drive_high: true,
    weak_drive: false,
};

/// GPIO0 and GPIO1 shorted, GPIO2 pulled up, GPIO3 pulled down, GPIO4 floating, GPIO5 on GND, GPIO6 on the supply,
/// DET input only, EX0 / EX1 shift register outputs (never both high), EX0 wired to GPIO7, LED held by a function.
fn board() -> MockBoard {
    let mut b = MockBoard::default();
    let g0 = b.point("GPIO0", "mcu", ALL, Net::Open);
    b.shorted_to("GPIO1", "mcu", ALL, g0);
    b.point("GPIO2", "mcu", ALL, Net::PullUp);
    b.point("GPIO3", "mcu", ALL, Net::PullDown);
    b.point("GPIO4", "mcu", ALL, Net::Open);
    b.point("GPIO5", "mcu", ALL, Net::Gnd);
    b.point("GPIO6", "mcu", ALL, Net::Vdd);
    b.point("DET", "mcu", INPUT_ONLY, Net::PullUp);
    let ex0 = b.point("EX0", "chain", DRIVE_ONLY, Net::Open);
    let ex1 = b.point("EX1", "chain", DRIVE_ONLY, Net::Open);
    b.shorted_to("GPIO7", "mcu", ALL, ex0);
    let led = b.point("LED", "mcu", ALL, Net::Open);
    b.points[led as usize].held_by = Some("indication".into());
    b.interlocks.push((ex0, ex1, "one source at a time".into()));
    b
}

fn id(b: &MockBoard, name: &str) -> u16 {
    b.points.iter().position(|p| p.name == name).unwrap() as u16
}

async fn test_on(b: &mut MockBoard, parts: &[&str], watch: &[&str], plan: Option<&Plan>) -> Report {
    let points = b.host_points();
    let selected = select(&points, parts).unwrap();
    let watch = select(&points, watch).unwrap();
    let setup = Setup::new(&points, &selected, &watch, plan).unwrap();
    let mut steps = vec![];
    let report = run(b, &setup, std::future::pending::<()>(), |done, total| {
        steps.push((done, total))
    })
    .await
    .unwrap();
    // progress counts up to its total
    let &(done, total) = steps.last().unwrap();
    assert_eq!(done, total);
    assert!(steps.windows(2).all(|w| w[0].0 <= w[1].0));
    report
}

fn pulls(r: &Report, b: &MockBoard, name: &str) -> Check<PullClass> {
    r.point(id(b, name)).unwrap().pulls.clone()
}

#[tokio::test]
async fn pulls_and_rails() {
    let mut b = board();
    let r = test_on(&mut b, &["GPIO2..=GPIO6"], &[], None).await;
    assert_eq!(pulls(&r, &b, "GPIO2"), Check::Done(PullClass::PullUp));
    assert_eq!(pulls(&r, &b, "GPIO3"), Check::Done(PullClass::PullDown));
    assert_eq!(pulls(&r, &b, "GPIO4"), Check::Done(PullClass::Floating));
    assert_eq!(pulls(&r, &b, "GPIO5"), Check::Done(PullClass::HeldLow));
    assert_eq!(pulls(&r, &b, "GPIO6"), Check::Done(PullClass::HeldHigh));
    // a weak drive wins over an external pull, not over a rail
    for (name, gnd, vdd) in [
        ("GPIO2", false, false),
        ("GPIO3", false, false),
        ("GPIO5", true, false),
        ("GPIO6", false, true),
    ] {
        let p = r.point(id(&b, name)).unwrap();
        assert_eq!(p.gnd, Check::Done(gnd), "{name}");
        assert_eq!(p.vdd, Check::Done(vdd), "{name}");
    }
    assert_eq!(r.shorts(), 2);
    assert!(!r.passed());
    assert_eq!(b.releases, 1);
}

#[tokio::test]
async fn shorts_between_points() {
    let mut b = board();
    let r = test_on(&mut b, &["GPIO0,GPIO1,GPIO2,GPIO4"], &[], None).await;
    let (g0, g1) = (id(&b, "GPIO0"), id(&b, "GPIO1"));
    assert_eq!(
        r.connections,
        vec![Connection {
            a: g0,
            b: g1,
            expected: false
        }]
    );
    assert_eq!(r.point(g0).unwrap().between, Check::Done(()));
    // shorted together, but neither to a rail
    assert_eq!(r.point(g0).unwrap().gnd, Check::Done(false));
    assert_eq!(r.shorts(), 1);
    assert!(!r.passed());
}

#[tokio::test]
async fn same_net_is_expected_and_missing_is_a_fault() {
    let mut b = board();
    let plan = Plan::parse("[shorts]\nsame_net = [[\"GPIO0\", \"GPIO1\"], [\"GPIO2\", \"GPIO4\"]]")
        .unwrap();
    let r = test_on(&mut b, &[], &[], Some(&plan)).await;
    // the points the plan names are tested
    assert_eq!(r.points.len(), 4);
    assert!(r.connections.iter().all(|c| c.expected));
    assert_eq!(r.shorts(), 0);
    assert_eq!(r.missing, vec![(id(&b, "GPIO2"), id(&b, "GPIO4"))]);
    assert!(!r.passed());
}

#[tokio::test]
async fn what_a_point_cannot_do_is_not_checked() {
    let mut b = board();
    let r = test_on(&mut b, &["DET,EX0,EX1,GPIO7,GPIO4"], &[], None).await;
    let det = r.point(id(&b, "DET")).unwrap();
    assert_eq!(det.pulls, Check::Done(PullClass::PullUp));
    assert!(matches!(det.gnd, Check::NotChecked(_)));
    assert!(matches!(det.vdd, Check::NotChecked(_)));
    let ex1 = r.point(id(&b, "EX1")).unwrap();
    assert_eq!(ex1.pulls, Check::NotChecked("can't be read".into()));
    // the shift register output drives GPIO7: found from the outputs' side, the interlock never refused
    assert_eq!(
        r.connections,
        vec![Connection {
            a: id(&b, "EX0"),
            b: id(&b, "GPIO7"),
            expected: false
        }]
    );
    assert!(ex1.notes.is_empty(), "{:?}", ex1.notes);
    assert_eq!(ex1.between, Check::Done(()));
}

#[tokio::test]
async fn refusals_are_shown_not_fatal() {
    let mut b = board();
    // EX1 high from the start: EX0 high is refused by the interlock
    let ex1 = id(&b, "EX1");
    b.states[ex1 as usize] = Some(PointState::Output {
        level: Level::High,
        drive: Drive::Normal,
    });
    b.interlocks.clear();
    let ex0 = id(&b, "EX0");
    b.interlocks.push((ex0, ex1, "one VREF source".into()));
    let points = b.host_points();
    // EX1 isn't tested, so it stays high and EX0 is refused
    let setup = Setup::new(
        &points,
        &select(&points, &["EX0,GPIO7,LED"]).unwrap(),
        &[],
        None,
    )
    .unwrap();
    let r = run(&mut b, &setup, std::future::pending::<()>(), |_, _| {})
        .await
        .unwrap();
    let led = r.point(id(&b, "LED")).unwrap();
    assert_eq!(
        led.pulls,
        Check::Refused("pull-up: in use by indication; pull-down: in use by indication".into())
    );
    assert_eq!(
        led.gnd,
        Check::Refused("drive high (weak): in use by indication".into())
    );
    let ex0 = r.point(ex0).unwrap();
    assert!(
        ex0.notes
            .iter()
            .any(|n| n == "drive high: refused: one VREF source"),
        "{:?}",
        ex0.notes
    );
    assert!(matches!(ex0.between, Check::Refused(_)));
    assert_eq!(b.releases, 1);
}

#[tokio::test]
async fn watched_points_are_only_read() {
    let mut b = board();
    let r = test_on(&mut b, &["GPIO0"], &["GPIO1,DET"], None).await;
    let (g0, g1, det) = (id(&b, "GPIO0"), id(&b, "GPIO1"), id(&b, "DET"));
    assert!(!b.was_set(g1) && !b.was_set(det));
    assert_eq!(r.point(det).unwrap().level, Some(Check::Done(Level::High)));
    assert_eq!(r.point(g1).unwrap().level, Some(Check::Done(Level::Low)));
    // GPIO1 sees GPIO0 drive high (its level at rest is low)
    assert_eq!(r.connections.len(), 1);
    assert_eq!((r.connections[0].a, r.connections[0].b), (g0, g1));
}

#[tokio::test]
async fn plan_allow_only_narrows() {
    let mut b = board();
    let plan = Plan::parse(
        "[[point]]\nname = \"GPIO4\"\nallow = [\"read\", \"pull_up\", \"pull_down\"]\n\
         [[point]]\nname = \"DET\"\nallow = [\"read\", \"drive_low\"]\n",
    )
    .unwrap();
    let r = test_on(&mut b, &[], &[], Some(&plan)).await;
    let g4 = id(&b, "GPIO4");
    // pulls without driving first, no rail tests
    assert_eq!(r.point(g4).unwrap().pulls, Check::Done(PullClass::Floating));
    assert!(matches!(r.point(g4).unwrap().gnd, Check::NotChecked(_)));
    assert!(
        b.sets
            .iter()
            .all(|(p, s)| *p != g4 || matches!(s, PointState::Input { .. }))
    );
    // allow can't make an input-only pin drive
    let det = r.point(id(&b, "DET")).unwrap();
    assert!(!det.target.caps.drive_low);
    assert!(!det.target.caps.pull_up);
}

#[tokio::test]
async fn released_when_cancelled_or_failed() {
    let mut b = board();
    let points = b.host_points();
    let setup = Setup::new(&points, &[], &[], None).unwrap();
    let r = run(&mut b, &setup, std::future::ready(()), |_, _| {}).await;
    assert!(matches!(r, Err(Stopped::Cancelled)));
    assert_eq!(b.releases, 1);

    b.fail_after = Some(b.calls + 5);
    let r = run(&mut b, &setup, std::future::pending::<()>(), |_, _| {}).await;
    match r {
        Err(Stopped::Failed { error, released }) => {
            assert_eq!(error, PinError::Failed("link lost".into()));
            assert!(released);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(b.releases, 2);
    assert!(b.states.iter().all(Option::is_none));
}

#[tokio::test]
async fn save_then_the_plan_is_the_test() {
    let mut b = board();
    let points = b.host_points();
    let r = test_on(&mut b, &["GPIO2,GPIO3,GPIO4,DET,EX0"], &[], None).await;
    let (text, saved) = save_to("", "diag.pins", &points, &r).unwrap();
    assert_eq!(saved.written.len(), 4);
    assert_eq!(saved.skipped, vec![id(&b, "EX0")]);
    assert!(text.starts_with("target = \"diag.pins\""), "{text}");
    let plan = Plan::parse(&text).unwrap();
    assert_eq!(plan.points.len(), 4);

    // the same board passes, saving again changes nothing
    let r = test_on(&mut b, &[], &[], Some(&plan)).await;
    assert!(r.passed(), "{:?}", r.mismatches());
    let (again, saved) = save_to(&text, "diag.pins", &points, &r).unwrap();
    assert_eq!(again, text);
    assert_eq!(saved.unchanged.len(), 4);

    // a board without GPIO2's pull-up fails
    let g2 = id(&b, "GPIO2");
    b.nets[b.points[g2 as usize].net] = Net::Open;
    let r = test_on(&mut b, &[], &[], Some(&plan)).await;
    assert_eq!(
        r.mismatches(),
        vec![Mismatch {
            id: g2,
            expected: PullClass::PullUp,
            found: Ok(PullClass::Floating)
        }]
    );
    assert!(!r.passed());
}

#[tokio::test]
async fn save_keeps_the_plan_and_splits_lists() {
    let mut b = board();
    let points = b.host_points();
    let text = r#"# rePort rev B
target = "diag.pins"

[[point]]
names = ["GPIO2", "GPIO4"] # both pulled up on rev A
expect = "pull_up"
allow = ["read", "pull_up", "pull_down", "drive_low", "drive_high"]

[shorts]
among = ["GPIO2", "GPIO4"]
"#;
    let plan = Plan::parse(text).unwrap();
    let r = test_on(&mut b, &[], &[], Some(&plan)).await;
    assert_eq!(r.mismatches().len(), 1);
    let (saved_text, saved) = save_to(text, "", &points, &r).unwrap();
    assert_eq!(saved.unchanged, vec![id(&b, "GPIO2")]);
    assert_eq!(saved.written, vec![id(&b, "GPIO4")]);
    assert!(saved_text.starts_with("# rePort rev B\n"), "{saved_text}");
    assert!(saved_text.contains("among = [\"GPIO2\", \"GPIO4\"]"));
    let plan = Plan::parse(&saved_text).unwrap();
    assert_eq!(plan.points[0].names, vec!["GPIO2"]);
    assert_eq!(plan.points[1].name.as_deref(), Some("GPIO4"));
    assert_eq!(plan.points[1].expect, Some(PullClass::Floating));
    // GPIO4 keeps what it was allowed
    assert!(plan.points[1].allow.is_some());
    let r = test_on(&mut b, &[], &[], Some(&plan)).await;
    assert!(r.passed(), "{:?}", r.mismatches());
}

#[tokio::test]
async fn an_expectation_not_checked_fails() {
    let mut b = board();
    let plan = Plan::parse("[[point]]\nname = \"LED\"\nexpect = \"floating\"").unwrap();
    let r = test_on(&mut b, &[], &[], Some(&plan)).await;
    let m = r.mismatches();
    assert_eq!(m.len(), 1);
    assert!(m[0].found.is_err());
    assert!(!r.passed());
}
