//! Plan files: what a good board shows, and what a test may do on it.
//!
//! ```toml
//! target = "diag.pins"                 # where PinDiag sits in the API, when the device has it more than once
//!
//! [[point]]
//! names = ["GPIO3", "GPIO4..=GPIO7"]   # names, ids, ranges and globs, as on the command line
//! expect = "pull_up"                   # pull_up, pull_down, floating, held_high, held_low
//! [[point]]
//! name = "GPIO0"
//! allow = ["read"]                     # read, pull_up, pull_down, drive_low, drive_high: only narrows the caps
//!
//! [shorts]
//! among = ["GPIO0..=GPIO8"]            # the points tested against each other (default: all tested points)
//! same_net = [["GPIO26", "GPIO34"]]    # one net on the board: connected is right, not connected a fault
//! ```
//!
//! For `expect`, an entry naming the point exactly wins over one matching it by a range or glob, and a later entry
//! over an earlier one of the same kind. `allow` lists of every matching entry apply together. [save] writes what
//! a board shows into `expect`, so the next run with the plan is a pass/fail test.

use crate::probe::{Check, Report};
use crate::select::select;
use crate::{Point, PointCaps};
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::path::Path;
use toml_edit::{Array, ArrayOfTables, DocumentMut, Item, Table, value};

/// What the pull test says about a point.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PullClass {
    /// High with the internal pull-down too: an external pull-up.
    PullUp,
    /// Low with the internal pull-up too: an external pull-down.
    PullDown,
    /// Follows the internal pull.
    Floating,
    /// High whatever is done, also when driven low: shorted to the supply.
    HeldHigh,
    /// Low whatever is done, also when driven high: shorted to GND.
    HeldLow,
    /// Low with the pull-up and high with the pull-down: something on the board drives it.
    Odd,
}

impl PullClass {
    /// As in a plan file: `pull_up`.
    pub fn as_str(&self) -> &'static str {
        match self {
            PullClass::PullUp => "pull_up",
            PullClass::PullDown => "pull_down",
            PullClass::Floating => "floating",
            PullClass::HeldHigh => "held_high",
            PullClass::HeldLow => "held_low",
            PullClass::Odd => "odd",
        }
    }
}

/// For people: `pull-up`.
impl Display for PullClass {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            PullClass::PullUp => "pull-up",
            PullClass::PullDown => "pull-down",
            PullClass::Floating => "floating",
            PullClass::HeldHigh => "held high",
            PullClass::HeldLow => "held low",
            PullClass::Odd => "odd",
        })
    }
}

/// What a plan's `allow` may list.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Read,
    PullUp,
    PullDown,
    DriveLow,
    DriveHigh,
}

/// A plan file, see the [module](self) documentation.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    /// Path of the `PinDiag` in the device's API: `diag.pins`.
    pub target: Option<String>,
    #[serde(default, rename = "point")]
    pub points: Vec<PlanPoint>,
    #[serde(default)]
    pub shorts: Shorts,
}

/// One `[[point]]` entry.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanPoint {
    pub name: Option<String>,
    #[serde(default)]
    pub names: Vec<String>,
    pub expect: Option<PullClass>,
    pub allow: Option<Vec<Action>>,
}

/// The `[shorts]` table.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shorts {
    pub among: Option<Vec<String>>,
    #[serde(default)]
    pub same_net: Vec<Vec<String>>,
}

/// A plan applied to a device's points.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Resolved {
    /// Every point the plan names, in order: what is tested when the command line names none.
    pub mentioned: Vec<u16>,
    pub expect: HashMap<u16, PullClass>,
    /// What the plan allows per point, all of [PointCaps] where it says nothing.
    pub allow: HashMap<u16, PointCaps>,
    pub among: Option<Vec<u16>>,
    pub same_net: Vec<Vec<u16>>,
}

impl PlanPoint {
    fn parts(&self) -> impl Iterator<Item = &str> {
        self.name
            .iter()
            .chain(&self.names)
            .flat_map(|n| n.split(','))
            .map(str::trim)
            .filter(|n| !n.is_empty())
    }
}

impl Plan {
    pub fn load(path: &Path) -> Result<Plan> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        Plan::parse(&text).with_context(|| format!("in {}", path.display()))
    }

    pub fn parse(text: &str) -> Result<Plan> {
        let plan: Plan = toml::from_str(text)?;
        for (i, p) in plan.points.iter().enumerate() {
            if p.parts().next().is_none() {
                bail!("[[point]] number {} has no name or names", i + 1);
            }
        }
        Ok(plan)
    }

    /// The plan for this device's points; a name it doesn't have is an error.
    pub fn resolve(&self, points: &[Point]) -> Result<Resolved> {
        let mut r = Resolved::default();
        let add = |ids: &[u16], r: &mut Resolved| {
            for &id in ids {
                if !r.mentioned.contains(&id) {
                    r.mentioned.push(id);
                }
            }
        };
        // exact names first, so they win over patterns: (exact, entry index) orders the entries
        let mut expect: HashMap<u16, (bool, usize, PullClass)> = HashMap::new();
        for (k, entry) in self.points.iter().enumerate() {
            let parts: Vec<&str> = entry.parts().collect();
            let ids = select(points, &parts).context("in [[point]]")?;
            add(&ids, &mut r);
            if let Some(class) = entry.expect {
                for p in points.iter().filter(|p| ids.contains(&p.id)) {
                    let exact = parts.contains(&p.name.as_str());
                    let rank = (exact, k, class);
                    let e = expect.entry(p.id).or_insert(rank);
                    if (rank.0, rank.1) >= (e.0, e.1) {
                        *e = rank;
                    }
                }
            }
            if let Some(allow) = &entry.allow {
                let mask = PointCaps {
                    read: allow.contains(&Action::Read),
                    pull_up: allow.contains(&Action::PullUp),
                    pull_down: allow.contains(&Action::PullDown),
                    drive_low: allow.contains(&Action::DriveLow),
                    drive_high: allow.contains(&Action::DriveHigh),
                    weak_drive: true,
                };
                for &id in &ids {
                    let a = r.allow.entry(id).or_insert(ALL);
                    *a = crate::probe::narrow(*a, mask);
                }
            }
        }
        r.expect = expect.into_iter().map(|(id, (_, _, c))| (id, c)).collect();
        if let Some(among) = &self.shorts.among {
            let ids = select(points, among).context("in [shorts] among")?;
            add(&ids, &mut r);
            r.among = Some(ids);
        }
        for net in &self.shorts.same_net {
            let ids = select(points, net).context("in [shorts] same_net")?;
            add(&ids, &mut r);
            r.same_net.push(ids);
        }
        Ok(r)
    }
}

const ALL: PointCaps = PointCaps {
    read: true,
    pull_up: true,
    pull_down: true,
    drive_low: true,
    drive_high: true,
    weak_drive: true,
};

/// What [save] changed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Saved {
    /// Points whose `expect` was written or changed.
    pub written: Vec<u16>,
    /// Points with an `expect` already right.
    pub unchanged: Vec<u16>,
    /// Points tested without a pull class to write (not checked, refused): left as they were.
    pub skipped: Vec<u16>,
}

/// Writes the pull class every tested point showed into `expect` of the plan at `path`, created with `target` when
/// it doesn't exist. Comments and the rest of an existing plan stay. A point named exactly by an entry gets its
/// `expect` there (taken out of a `names` list whose `expect` differs, into its own entry); others get a new
/// `[[point]]` entry.
pub fn save(path: &Path, target: &str, points: &[Point], report: &Report) -> Result<Saved> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).with_context(|| format!("Failed to read {}", path.display())),
    };
    let (text, saved) =
        save_to(&text, target, points, report).with_context(|| format!("in {}", path.display()))?;
    std::fs::write(path, text).with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(saved)
}

/// [save] on the plan's text: the new text.
pub fn save_to(
    text: &str,
    target: &str,
    points: &[Point],
    report: &Report,
) -> Result<(String, Saved)> {
    // checks it is a plan before changing it
    Plan::parse(text)?;
    let mut doc: DocumentMut = text.parse()?;
    if !doc.contains_key("target") && !target.is_empty() {
        doc.insert("target", value(target));
    }
    if !doc.contains_key("point") {
        doc.insert("point", Item::ArrayOfTables(ArrayOfTables::new()));
    }
    let Some(entries) = doc["point"].as_array_of_tables_mut() else {
        bail!("'point' must be [[point]] entries");
    };
    let mut saved = Saved::default();
    for p in report.points.iter().filter(|p| !p.target.watch) {
        let id = p.target.point.id;
        let Check::Done(class) = p.pulls else {
            saved.skipped.push(id);
            continue;
        };
        let Some(point) = points.iter().find(|q| q.id == id) else {
            continue;
        };
        let name = point.name.as_str();
        match write_expect(entries, name, class) {
            true => saved.written.push(id),
            false => saved.unchanged.push(id),
        }
    }
    Ok((doc.to_string(), saved))
}

/// `expect = class` for the point `name`; `false` when it was already so.
fn write_expect(entries: &mut ArrayOfTables, name: &str, class: PullClass) -> bool {
    let is = |t: &Table, key: &str| t.get(key).and_then(Item::as_str) == Some(class.as_str());
    // an entry of its own
    for t in entries.iter_mut() {
        if t.get("name").and_then(Item::as_str) == Some(name) {
            if is(t, "expect") {
                return false;
            }
            t.insert("expect", value(class.as_str()));
            return true;
        }
    }
    // in a names list: right already, or taken out of it
    let mut allow = None;
    for t in entries.iter_mut() {
        let Some(names) = t.get_mut("names").and_then(Item::as_array_mut) else {
            continue;
        };
        let Some(k) = names.iter().position(|n| n.as_str() == Some(name)) else {
            continue;
        };
        if is(t, "expect") {
            return false;
        }
        let names = t["names"].as_array_mut().expect("checked above");
        names.remove(k);
        allow = t.get("allow").cloned();
        break;
    }
    let mut t = Table::new();
    t.insert("name", value(name));
    t.insert("expect", value(class.as_str()));
    if let Some(allow) = allow {
        t.insert("allow", allow);
    }
    entries.push(t);
    // entries whose names list went empty would match nothing and fail the next run
    entries.retain(|t| {
        t.contains_key("name")
            || t.get("names")
                .and_then(Item::as_array)
                .is_some_and(|n: &Array| !n.is_empty())
    });
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board() -> Vec<Point> {
        ["GPIO0", "GPIO1", "GPIO2", "GPIO3", "EX0"]
            .iter()
            .enumerate()
            .map(|(id, name)| Point {
                id: id as u16,
                name: name.to_string(),
                group: if name.starts_with("EX") {
                    "chain"
                } else {
                    "mcu"
                }
                .into(),
                caps: ALL,
            })
            .collect()
    }

    #[test]
    fn resolves_expect_allow_and_shorts() {
        let plan = Plan::parse(
            r#"
            target = "diag.pins"
            [[point]]
            names = ["GPIO0..=GPIO3"]
            expect = "pull_up"
            [[point]]
            name = "GPIO2"
            expect = "floating"
            [[point]]
            names = ["GPIO*"]
            expect = "pull_down"
            allow = ["read", "pull_up"]
            [[point]]
            name = "GPIO1"
            allow = ["read", "drive_low", "pull_up"]
            [shorts]
            among = ["mcu.*"]
            same_net = [["GPIO0", "GPIO3"]]
            "#,
        )
        .unwrap();
        assert_eq!(plan.target.as_deref(), Some("diag.pins"));
        let r = plan.resolve(&board()).unwrap();
        assert_eq!(r.mentioned, vec![0, 1, 2, 3]);
        // the later pattern entry wins over the earlier range, the exact name over both
        assert_eq!(r.expect[&0], PullClass::PullDown);
        assert_eq!(r.expect[&2], PullClass::Floating);
        assert!(!r.allow[&1].drive_low);
        assert!(r.allow[&1].pull_up);
        assert!(!r.allow[&0].pull_down);
        assert_eq!(r.among, Some(vec![0, 1, 2, 3]));
        assert_eq!(r.same_net, vec![vec![0, 3]]);
    }

    #[test]
    fn mistakes_are_errors() {
        assert!(Plan::parse("[[point]]\nexpect = \"pull_up\"").is_err());
        assert!(Plan::parse("[[point]]\nname = \"A\"\nexpect = \"pulled\"").is_err());
        assert!(Plan::parse("[[point]]\nname = \"A\"\nallow = [\"write\"]").is_err());
        assert!(Plan::parse("[shorts]\nsame_nets = []").is_err());
        let plan = Plan::parse("[[point]]\nname = \"GPIO9\"").unwrap();
        assert!(plan.resolve(&board()).is_err());
    }
}
