//! A trait embedded with `ww_impl!` that moves to another crate with the same shape: the API model names a trait by
//! crate and name, so the attached trait's identity changed, a wire break that needs a new line, never a revision.

use super::*;
use crate::compat::tests::add_known;

/// `ind_dep` 0.1.0: the status LED trait an API embeds.
const IND_DEP: &str = r#"
    use ww_api::prelude::*;

    #[ww_trait]
    #[effect(edit)]
    pub trait Indication {
        property!(rw on: bool);
    }
"#;

/// The API line embedding `ind_dep`'s trait.
const FROM_DEP: &str = r#"
    #[ww_api_root(wire = "w1.0")]
    #[effect(edit)]
    pub trait Api {
        fn read() -> u32;
        ww_impl!(indication: ind_dep::Indication);
    }
"#;

/// The same line with a local stand-in of the same shape and name.
const LOCAL: &str = r#"
    #[ww_api_root(wire = "w1.0")]
    #[effect(edit)]
    pub trait Api {
        fn read() -> u32;
        ww_impl!(indication: Indication);
    }

    #[ww_trait]
    #[effect(edit)]
    pub trait Indication {
        property!(rw on: bool);
    }
"#;

#[test]
fn attached_trait_from_another_crate_needs_a_new_line() {
    let (line, dep) = dep_crate("ind_dep", IND_DEP);
    add_known(dep);
    let old = load_with("0.1.0", FROM_DEP, &line);
    let new = load_with("0.1.0", LOCAL, &line);
    let report = compare(&old, &new).unwrap();
    assert_eq!(report.change, Change::Breaking, "{report:#?}");
    assert_eq!(report.wire_bump, WireBump::Line, "{report:#?}");
    assert_eq!(report.minimal_wire(), Some(WireVersion::new(2, 0)));
    assert!(
        report
            .wire_reasons
            .iter()
            .any(|r| r.contains("trait ind_dep::Indication changed")),
        "{:#?}",
        report.wire_reasons
    );
}
