//! Which points a list like `GPIO0,P8,GPIO2..=GPIO7,chain.*` means.
//!
//! Each comma-separated part is, tried in this order:
//!
//! * a point's name: `GPIO0`, `EX13` (a name wins over the forms below, so a point named `P8` is that point);
//! * a range of names with the same prefix: `GPIO2..=GPIO7` (inclusive) or `GPIO2..GPIO8` (end excluded); names
//!   in between that the device doesn't have are skipped;
//! * a glob with `*` and `?`: on names (`GPIO1*`), or `group.name` on a group and the names in it (`chain.*`,
//!   `mcu.GPIO?`);
//! * a point id: `P8`, also in a range `P2..=P7`.
//!
//! A part that matches nothing is an error, so a typo doesn't shrink a test. The ids come out in the order given,
//! each once.

use crate::Point;
use anyhow::{Result, bail};

/// Point ids for `parts`, each of which may hold several comma-separated parts.
pub fn select<S: AsRef<str>>(points: &[Point], parts: &[S]) -> Result<Vec<u16>> {
    let mut ids = vec![];
    for part in parts
        .iter()
        .flat_map(|p| p.as_ref().split(','))
        .map(str::trim)
        .filter(|p| !p.is_empty())
    {
        for id in select_one(points, part)? {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    Ok(ids)
}

/// Whether `part` (one part, no commas) means the point `p`.
pub fn matches(points: &[Point], part: &str, p: &Point) -> bool {
    select_one(points, part).is_ok_and(|ids| ids.contains(&p.id))
}

fn select_one(points: &[Point], part: &str) -> Result<Vec<u16>> {
    let by_name: Vec<u16> = points
        .iter()
        .filter(|p| p.name == part)
        .map(|p| p.id)
        .collect();
    if !by_name.is_empty() {
        return Ok(by_name);
    }
    if let Some((from, to, inclusive)) = split_range(part) {
        return range(points, part, from, to, inclusive);
    }
    if part.contains(['*', '?']) {
        let ids: Vec<u16> = match part.split_once('.') {
            Some((group, name)) => points
                .iter()
                .filter(|p| glob(group, &p.group) && glob(name, &p.name))
                .map(|p| p.id)
                .collect(),
            None => points
                .iter()
                .filter(|p| glob(part, &p.name))
                .map(|p| p.id)
                .collect(),
        };
        if ids.is_empty() {
            bail!("'{part}' matches no point ('ww pins list' shows them)");
        }
        return Ok(ids);
    }
    if let Some(id) = point_id(part) {
        if points.iter().any(|p| p.id == id) {
            return Ok(vec![id]);
        }
        bail!("no point {part}: the device has {} points", points.len());
    }
    if points.iter().any(|p| p.group == part) {
        bail!("'{part}' is a group: '{part}.*' takes all its points");
    }
    bail!("no point named '{part}' ('ww pins list' shows them)")
}

fn split_range(part: &str) -> Option<(&str, &str, bool)> {
    if let Some((a, b)) = part.split_once("..=") {
        return Some((a, b, true));
    }
    part.split_once("..").map(|(a, b)| (a, b, false))
}

fn range(points: &[Point], part: &str, from: &str, to: &str, inclusive: bool) -> Result<Vec<u16>> {
    let named = |n: &str| points.iter().any(|p| p.name == n);
    let (ids, what) = if named(from) || named(to) {
        let ((prefix, a, width), (prefix_to, b, _)) = match (numbered(from), numbered(to)) {
            (Some(a), Some(b)) => (a, b),
            _ => bail!("range '{part}': both ends need a number at the end, as in GPIO2..=GPIO7"),
        };
        if prefix != prefix_to {
            bail!("range '{part}': both ends need the same prefix ('{prefix}', '{prefix_to}')");
        }
        let end = if inclusive { b } else { b.saturating_sub(1) };
        let mut ids = vec![];
        if inclusive || b > 0 {
            for n in a..=end {
                let name = format!("{prefix}{n:0width$}");
                ids.extend(points.iter().filter(|p| p.name == name).map(|p| p.id));
            }
        }
        (ids, "names")
    } else {
        let (Some(a), Some(b)) = (point_id(from), point_id(to)) else {
            bail!("range '{part}': no point named '{from}' or '{to}'");
        };
        let ids = points
            .iter()
            .map(|p| p.id)
            .filter(|&id| id >= a && if inclusive { id <= b } else { id < b })
            .collect();
        (ids, "ids")
    };
    if ids.is_empty() {
        bail!("range '{part}' holds no point ({what})");
    }
    Ok(ids)
}

/// `GPIO07` → (`GPIO`, 7, 2): prefix, number, width for zero padding (0 without a leading zero).
fn numbered(name: &str) -> Option<(&str, u32, usize)> {
    let digits = name.len() - name.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    if digits == 0 {
        return None;
    }
    let (prefix, number) = name.split_at(name.len() - digits);
    let width = if number.len() > 1 && number.starts_with('0') {
        number.len()
    } else {
        0
    };
    Some((prefix, number.parse().ok()?, width))
}

/// `P8` → 8.
fn point_id(part: &str) -> Option<u16> {
    let n = part.strip_prefix('P')?;
    if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    n.parse().ok()
}

/// `*` any run of characters, `?` one character.
fn glob(pattern: &str, text: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pattern.chars().collect(), text.chars().collect());
    // classic two-pointer match with backtracking to the last `*`
    let (mut pi, mut ti, mut star, mut mark) = (0, 0, None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PointCaps;

    const CAPS: PointCaps = PointCaps {
        read: true,
        pull_up: true,
        pull_down: true,
        drive_low: true,
        drive_high: true,
        weak_drive: true,
    };

    fn board() -> Vec<Point> {
        let names = [
            ("GPIO0", "mcu"),
            ("GPIO2", "mcu"),
            ("GPIO3", "mcu"),
            ("GPIO5", "mcu"),
            ("GPIO10", "mcu"),
            ("P1", "mcu"),
            ("EX00", "chain"),
            ("EX01", "chain"),
            ("EX02", "chain"),
        ];
        names
            .iter()
            .enumerate()
            .map(|(id, (name, group))| Point {
                id: id as u16,
                name: name.to_string(),
                group: group.to_string(),
                caps: CAPS,
            })
            .collect()
    }

    fn sel(parts: &str) -> Result<Vec<u16>> {
        select(&board(), &[parts])
    }

    #[test]
    fn names_ids_and_order() {
        assert_eq!(sel("GPIO5, GPIO0,GPIO5").unwrap(), vec![3, 0]);
        // P0 is no name: an id; P1 is a name, of point 5
        assert_eq!(sel("P0,P1").unwrap(), vec![0, 5]);
        assert_eq!(select(&board(), &["GPIO0", "P8"]).unwrap(), vec![0, 8]);
    }

    #[test]
    fn ranges() {
        // GPIO4 doesn't exist and is skipped, GPIO10 is past the end
        assert_eq!(sel("GPIO2..=GPIO5").unwrap(), vec![1, 2, 3]);
        assert_eq!(sel("GPIO2..GPIO5").unwrap(), vec![1, 2]);
        // zero padded names
        assert_eq!(sel("EX00..=EX02").unwrap(), vec![6, 7, 8]);
        assert_eq!(sel("P6..=P7").unwrap(), vec![6, 7]);
        assert_eq!(sel("P6..P7").unwrap(), vec![6]);
        assert!(sel("GPIO2..=EX02").is_err());
        assert!(sel("P20..=P30").is_err());
    }

    #[test]
    fn globs() {
        assert_eq!(sel("chain.*").unwrap(), vec![6, 7, 8]);
        assert_eq!(sel("GPIO?").unwrap(), vec![0, 1, 2, 3]);
        assert_eq!(sel("GPIO1*").unwrap(), vec![4]);
        assert_eq!(sel("*.EX0?").unwrap(), vec![6, 7, 8]);
        assert!(sel("chain.GPIO*").is_err());
    }

    #[test]
    fn typos_are_errors() {
        let e = sel("GPIO0,GPOI5").unwrap_err().to_string();
        assert!(e.contains("GPOI5"), "{e}");
        let e = sel("chain").unwrap_err().to_string();
        assert!(e.contains("chain.*"), "{e}");
        assert!(sel("P99").is_err());
    }

    #[test]
    fn glob_matcher() {
        assert!(glob("*", ""));
        assert!(glob("a*b*c", "aXXbYc"));
        assert!(!glob("a*b", "ab_"));
        assert!(glob("?x", "ax"));
    }
}
