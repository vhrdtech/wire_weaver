//! A snapshot of an older model format against the unchanged source: fields its format doesn't have
//! (`Meta::effects`, API-58) are left out on both sides, also where the file has no meta at all.

use super::*;
use ww_snapshots::Resolver;
use ww_version::VersionTriplet;

const API: &str = r#"
    #[ww_api_root(wire = "w1.0")]
    pub trait Api {
        #[effect(actuate)]
        fn led_on();
        #[effect(edit, persist)]
        property!(rw brightness: u8);
        property!(ro uptime: u32);
    }
"#;

/// The source's bundle as a model format without effects saved it: no metas, which only held effects.
fn saved_without_effects(source: &ApiBundleOwned, format: VersionTriplet) -> String {
    let mut old = source.clone();
    old.ww_self_version = format;
    for level in crate::recorded::levels_mut(&mut old) {
        for item in level.items.iter_mut() {
            item.meta = None;
        }
    }
    ww_snapshots::file::render_ron(&old).unwrap()
}

fn compare_text(text: &str, source: &ApiBundleOwned) -> Report {
    let (read, recorded) = Recorded::parse(text).unwrap();
    compare_recorded(
        &Resolver::new(),
        &read,
        &recorded,
        source,
        &Recorded::default(),
        &[],
        &[],
    )
    .unwrap()
}

#[test]
fn effects_are_not_compared_against_a_format_without_them() {
    let source = load("0.1.0", API);
    let mut levels = source.clone();
    assert!(
        crate::recorded::levels_mut(&mut levels).any(|l| l.items.iter().any(|i| i.meta.is_some())),
        "the source has effects"
    );

    let report = compare_text(
        &saved_without_effects(&source, VersionTriplet::new(0, 4, 0)),
        &source,
    );
    assert_eq!(report.change, Change::None, "{report:#?}");
    assert!(
        report
            .unrecorded
            .iter()
            .any(|u| u.to_string() == "Meta::effects"),
        "{:?}",
        report.unrecorded
    );

    // the current format has effects: an item without them is a change
    let report = compare_text(&saved_without_effects(&source, ww_self::VERSION), &source);
    assert_ne!(report.change, Change::None, "{report:#?}");
}
