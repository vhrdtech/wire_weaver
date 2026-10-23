//! Board presets: where a device's points sit on the board's connectors, for the live view (`ww pins tui`, TOOL-21).
//!
//! A preset is the board description of boardmap (embedded_bedrock's `bedrock_board`) read as it is: ww takes the
//! keys below and leaves the rest (parts, power tree, solder bridges) to boardmap. The header pads, their numbers,
//! positions and nets come from the geometry JSON the description names, so the view shows each connector as it
//! is on the board. A preset written for ww alone may leave the geometry out and list the nets of each connector.
//!
//! ```toml
//! [board]
//! name = "NUCLEO-G0B1RE"
//! geometry = "nucleo_g0b1re.geometry.json"   # relative to this file; pads of the connectors below
//!
//! [[view.connector]]
//! ref = "CN7"                                # footprint in the geometry
//! label = "CN7 ST morpho"
//!
//! [[view.connector]]                         # without geometry: nets by pin number, rows of `columns`
//! ref = "J1"
//! label = "J1 expansion"
//! columns = 2
//! nets = ["PA0", "PA1", "GND", "3V3"]
//!
//! [aliases]                                  # shown next to a net's name
//! PA5 = "D13 SCK"
//!
//! [pins]                                     # what an MCU pin does on the board, by net name
//! PA13 = { role = "taken", short = "SWDIO", note = "ST-LINK SWDIO through SB13 and CN4" }
//! ```
//!
//! A pad shows the device's point whose name is the pad's net (`PA5`), or failing that its alias or the alias's
//! first word (`D13`). Points on no pad are listed apart, so nothing the device serves is hidden.

use crate::Point;
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// What an MCU pin does on the board, boardmap's `[pins]` roles.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Reaches a header and nothing else.
    Free,
    /// Has a board job by default (an LED, a button, a UART to the debugger) that a solder bridge or jumper removes.
    Shared,
    /// Not usable for the application (debug port, reset).
    Taken,
    /// A fixed function: crystal, VBAT, VREF.
    Function,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Free => "free",
            Role::Shared => "shared",
            Role::Taken => "taken",
            Role::Function => "function",
        }
    }
}

/// One entry of `[pins]`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct PinInfo {
    pub role: Role,
    /// A few characters for the header label: `SWDIO`.
    #[serde(default)]
    pub short: String,
    /// The full story.
    #[serde(default)]
    pub note: String,
}

/// One pad of a connector.
#[derive(Clone, Debug, PartialEq)]
pub struct Pad {
    /// As printed: `1`.
    pub number: String,
    /// The net on it as the board file names it, `None` for an unconnected pad.
    pub net: Option<String>,
    /// Nets a fitted solder bridge joins to it (`SB*` footprints whose value says `ON`, as boardmap reads them):
    /// a pad on `NetCN7_38` is `PB12` with SB3 on.
    pub joined: Vec<String>,
}

impl Pad {
    /// The net's name to show: its own, or when the board file made it up (`NetCN7_38`) the best joined one, an
    /// MCU pin's name first.
    pub fn name(&self) -> Option<&str> {
        let net = self.net.as_deref()?;
        if !is_anonymous(net) {
            return Some(net);
        }
        self.joined
            .iter()
            .filter(|n| !is_anonymous(n))
            .min_by_key(|n| (!is_mcu_pin(n), n.len()))
            .map(String::as_str)
            .or(Some(net))
    }

    /// Its own net, then the joined ones.
    pub fn nets(&self) -> impl Iterator<Item = &str> {
        self.net
            .as_deref()
            .into_iter()
            .chain(self.joined.iter().map(String::as_str))
    }
}

/// A net name the board file made up (`NetCN7_29`, `unconnected-...`), not a label from the schematic.
pub fn is_anonymous(net: &str) -> bool {
    net.is_empty() || net.starts_with("Net") || net.to_ascii_lowercase().starts_with("unconnected")
}

/// `PA0`..`PK15`: boardmap's default pattern for MCU pin nets.
fn is_mcu_pin(net: &str) -> bool {
    let b = net.as_bytes();
    b.len() >= 3 && b[0] == b'P' && (b'A'..=b'K').contains(&b[1]) && b[2..].iter().all(u8::is_ascii_digit)
}

/// A connector, its pads in rows (top of the board first) and columns (left first), as seen from the top.
#[derive(Clone, Debug, PartialEq)]
pub struct Connector {
    pub r#ref: String,
    pub label: String,
    /// `rows[r][c]`, `None` where the grid has no pad.
    pub rows: Vec<Vec<Option<Pad>>>,
    /// Where its pads are on the board, `[x_min, y_min, x_max, y_max]` in mm (y down), when the geometry gives
    /// it: connectors are shown in board order.
    pub span: Option<[f64; 4]>,
}

/// A board preset, see the [module](self) documentation.
#[derive(Clone, Debug, Default)]
pub struct Board {
    pub name: String,
    pub connectors: Vec<Connector>,
    pub aliases: BTreeMap<String, String>,
    pub pins: BTreeMap<String, PinInfo>,
}

#[derive(Deserialize)]
struct Desc {
    board: DescBoard,
    #[serde(default)]
    view: DescView,
    #[serde(default)]
    aliases: BTreeMap<String, String>,
    #[serde(default)]
    pins: BTreeMap<String, PinInfo>,
}

#[derive(Deserialize)]
struct DescBoard {
    name: String,
    geometry: Option<String>,
}

#[derive(Default, Deserialize)]
struct DescView {
    #[serde(default)]
    connector: Vec<DescConnector>,
}

#[derive(Deserialize)]
struct DescConnector {
    r#ref: String,
    #[serde(default)]
    label: String,
    /// ww's own: nets by pin number, without geometry.
    nets: Option<Vec<String>>,
    #[serde(default = "one")]
    columns: usize,
}

fn one() -> usize {
    1
}

#[derive(Deserialize)]
struct Geometry {
    footprints: BTreeMap<String, Footprint>,
}

#[derive(Deserialize)]
struct Footprint {
    #[serde(default)]
    value: String,
    pads: Vec<GeoPad>,
}

#[derive(Deserialize)]
struct GeoPad {
    number: String,
    at: [f64; 2],
    net: Option<String>,
}

impl Board {
    /// Reads a preset and the geometry it names.
    pub fn load(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let dir = path.parent().unwrap_or(Path::new("."));
        Self::parse(&text, dir).with_context(|| format!("board preset {}", path.display()))
    }

    /// A preset from its text; `dir` is where its geometry file is looked for.
    pub fn parse(text: &str, dir: &Path) -> Result<Self> {
        let desc: Desc = toml::from_str(text)?;
        let needs_geometry = desc.view.connector.iter().any(|c| c.nets.is_none());
        let geometry = match (&desc.board.geometry, needs_geometry) {
            (Some(file), true) => {
                let path: PathBuf = dir.join(file);
                let text = std::fs::read_to_string(&path)
                    .with_context(|| format!("reading the geometry {}", path.display()))?;
                Some(
                    serde_json::from_str::<Geometry>(&text)
                        .with_context(|| format!("parsing the geometry {}", path.display()))?,
                )
            }
            (None, true) => bail!(
                "a connector has no `nets` and [board] names no `geometry` to take its pads from"
            ),
            (_, false) => None,
        };
        let bridges = geometry.as_ref().map(Bridges::new).unwrap_or_default();
        let mut connectors = vec![];
        for c in desc.view.connector {
            let label = if c.label.is_empty() {
                c.r#ref.clone()
            } else {
                c.label
            };
            let connector = match c.nets {
                Some(nets) => from_nets(c.r#ref, label, &nets, c.columns)?,
                None => {
                    let fp = geometry
                        .as_ref()
                        .and_then(|g| g.footprints.get(&c.r#ref))
                        .with_context(|| format!("no footprint {} in the geometry", c.r#ref))?;
                    from_footprint(c.r#ref, label, fp, &bridges)
                }
            };
            connectors.push(connector);
        }
        Ok(Board {
            name: desc.board.name,
            connectors,
            aliases: desc.aliases,
            pins: desc.pins,
        })
    }

    /// Which point a pad shows: the one named like one of its nets (its own first, then the joined ones), else
    /// like a net's alias or the alias's first word.
    pub fn point_for<'p>(&self, points: &'p [Point], pad: &Pad) -> Option<&'p Point> {
        let by = |name: &str| points.iter().find(|p| p.name == name);
        pad.nets().find_map(by).or_else(|| {
            pad.nets().find_map(|net| {
                let alias = self.aliases.get(net)?;
                by(alias).or_else(|| by(alias.split_whitespace().next()?))
            })
        })
    }

    /// The alias of a pad's net, of its own or a joined one.
    pub fn alias(&self, pad: &Pad) -> Option<&str> {
        pad.nets().find_map(|n| self.aliases.get(n)).map(String::as_str)
    }

    /// The `[pins]` entry of a pad's net, of its own or a joined one.
    pub fn pin_info(&self, pad: &Pad) -> Option<&PinInfo> {
        pad.nets().find_map(|n| self.pins.get(n))
    }

    /// Connectors in groups of those one above the other on the board, groups left to right: the order a view
    /// lays them out in. Without positions, each connector on its own, as the preset lists them.
    pub fn columns(&self) -> Vec<Vec<usize>> {
        let mut order: Vec<usize> = (0..self.connectors.len()).collect();
        if self.connectors.iter().any(|c| c.span.is_none()) {
            return order.into_iter().map(|i| vec![i]).collect();
        }
        let span = |i: usize| self.connectors[i].span.unwrap();
        order.sort_by(|&a, &b| span(a)[0].total_cmp(&span(b)[0]));
        let mut columns: Vec<(f64, Vec<usize>)> = vec![];
        for i in order {
            let [x0, _, x1, _] = span(i);
            match columns.last_mut() {
                // pads overlapping across (half a millimetre to spare): one above the other, as the Arduino sockets
                Some((right, col)) if x0 < *right + 0.5 => {
                    col.push(i);
                    *right = right.max(x1);
                }
                _ => columns.push((x1, vec![i])),
            }
        }
        columns
            .into_iter()
            .map(|(_, mut col)| {
                col.sort_by(|&a, &b| span(a)[1].total_cmp(&span(b)[1]));
                col
            })
            .collect()
    }
}

fn from_nets(r#ref: String, label: String, nets: &[String], columns: usize) -> Result<Connector> {
    if columns == 0 {
        bail!("{ref}: columns = 0");
    }
    let rows = nets
        .chunks(columns)
        .enumerate()
        .map(|(r, chunk)| {
            chunk
                .iter()
                .enumerate()
                .map(|(c, net)| {
                    Some(Pad {
                        number: (r * columns + c + 1).to_string(),
                        net: (!net.is_empty()).then(|| net.clone()),
                        joined: vec![],
                    })
                })
                .collect()
        })
        .collect();
    Ok(Connector {
        r#ref,
        label,
        rows,
        span: None,
    })
}

/// The pads on a grid of their distinct positions; a connector wider than tall is turned upright (pad 1 on top),
/// as a terminal has more lines to spare than columns.
fn from_footprint(r#ref: String, label: String, fp: &Footprint, bridges: &Bridges) -> Connector {
    let pads: Vec<&GeoPad> = fp.pads.iter().filter(|p| !p.number.is_empty()).collect();
    // positions on a 0.1 mm grid, so rounding noise doesn't split a row
    let key = |v: f64| (v * 10.0).round() as i64;
    let distinct = |f: &dyn Fn(&GeoPad) -> f64| {
        let mut v: Vec<i64> = pads.iter().map(|p| key(f(p))).collect();
        v.sort();
        v.dedup();
        v
    };
    let xs = distinct(&|p| p.at[0]);
    let ys = distinct(&|p| p.at[1]);
    let upright = xs.len() <= ys.len();
    let (rows_k, cols_k) = if upright { (&ys, &xs) } else { (&xs, &ys) };
    let mut rows: Vec<Vec<Option<Pad>>> = vec![vec![None; cols_k.len()]; rows_k.len()];
    for p in &pads {
        let (rk, ck) = if upright {
            (key(p.at[1]), key(p.at[0]))
        } else {
            (key(p.at[0]), key(p.at[1]))
        };
        let r = rows_k.binary_search(&rk).unwrap();
        let c = cols_k.binary_search(&ck).unwrap();
        rows[r][c] = Some(Pad {
            number: p.number.clone(),
            net: p.net.clone(),
            joined: p.net.as_deref().map(|n| bridges.joined(n)).unwrap_or_default(),
        });
    }
    if !upright {
        // pad 1 on top
        let first = rows
            .iter()
            .position(|r| r.iter().flatten().any(|p| p.number == "1"));
        if first.is_some_and(|f| f > rows.len() / 2) {
            rows.reverse();
        }
    }
    let span = pads.iter().fold(
        [f64::MAX, f64::MAX, f64::MIN, f64::MIN],
        |[x0, y0, x1, y1], p| {
            [
                x0.min(p.at[0]),
                y0.min(p.at[1]),
                x1.max(p.at[0]),
                y1.max(p.at[1]),
            ]
        },
    );
    Connector {
        r#ref,
        label,
        rows,
        span: (!pads.is_empty()).then_some(span),
    }
}

/// Nets joined by fitted solder bridges.
#[derive(Default)]
struct Bridges {
    parent: BTreeMap<String, String>,
}

impl Bridges {
    fn new(geo: &Geometry) -> Self {
        let mut b = Bridges::default();
        for (reference, fp) in &geo.footprints {
            if !reference.starts_with("SB") || !fp.value.to_ascii_uppercase().contains("ON") {
                continue;
            }
            let nets: Vec<&str> = fp.pads.iter().filter_map(|p| p.net.as_deref()).collect();
            for pair in nets.windows(2) {
                let (x, y) = (b.root(pair[0]), b.root(pair[1]));
                if x != y {
                    b.parent.insert(x, y);
                }
            }
        }
        b
    }

    fn root(&self, net: &str) -> String {
        let mut n = net;
        while let Some(p) = self.parent.get(n) {
            n = p;
        }
        n.to_string()
    }

    /// The other nets in `net`'s group, sorted.
    fn joined(&self, net: &str) -> Vec<String> {
        let root = self.root(net);
        let mut all: Vec<String> = self
            .parent
            .keys()
            .chain(self.parent.values())
            .filter(|n| *n != net && self.root(n) == root)
            .cloned()
            .collect();
        all.sort();
        all.dedup();
        all
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::ALL;

    fn nucleo() -> Board {
        Board::load(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/nucleo_g0b1re.toml"),
        )
        .unwrap()
    }

    fn net(c: &Connector, r: usize, col: usize) -> (&str, &str) {
        let p = c.rows[r][col].as_ref().unwrap();
        (p.number.as_str(), p.name().unwrap_or("-"))
    }

    #[test]
    fn nucleo_preset_as_boardmap_has_it() {
        let b = nucleo();
        assert_eq!(b.name, "NUCLEO-G0B1RE");
        assert_eq!(b.connectors.len(), 8);
        let cn7 = &b.connectors[0];
        assert_eq!(cn7.label, "CN7 ST morpho");
        assert_eq!((cn7.rows.len(), cn7.rows[0].len()), (19, 2));
        assert_eq!(net(cn7, 0, 0), ("1", "PC10"));
        assert_eq!(net(cn7, 0, 1), ("2", "PC11"));
        // NetCN7_38 through SB3 (on); CN7-25 stays apart, SB23 is off
        assert_eq!(net(cn7, 18, 1), ("38", "PB12"));
        assert_eq!(net(cn7, 12, 0), ("25", "NetCN7_25"));
        // the Arduino socket has D8 at the bottom, as on the board
        let cn5 = b.connectors.iter().find(|c| c.r#ref == "CN5").unwrap();
        assert_eq!((cn5.rows.len(), cn5.rows[0].len()), (10, 1));
        assert_eq!(net(cn5, 9, 0), ("1", "PA9"));
        assert_eq!(b.pins["PA13"].role, Role::Taken);
        let names: Vec<Vec<&str>> = b
            .columns()
            .iter()
            .map(|col| {
                col.iter()
                    .map(|&i| b.connectors[i].r#ref.as_str())
                    .collect()
            })
            .collect();
        assert_eq!(
            names,
            [
                vec!["CN7"],
                vec!["CN11"],
                vec!["CN6", "CN8"],
                vec!["CN4"],
                vec!["CN5", "CN9"],
                vec!["CN10"]
            ]
        );
    }

    #[test]
    fn points_by_net_or_alias() {
        let b = nucleo();
        let p = |id: u16, name: &str| Point {
            id,
            name: name.into(),
            group: "mcu".into(),
            caps: ALL,
        };
        let points = vec![p(0, "PA5"), p(1, "D12"), p(2, "PB12")];
        let pad = |net: &str| Pad {
            number: "1".into(),
            net: Some(net.into()),
            joined: vec![],
        };
        assert_eq!(b.point_for(&points, &pad("PA5")).unwrap().id, 0);
        // PA6 = "D12 MISO": by the alias's first word
        assert_eq!(b.point_for(&points, &pad("PA6")).unwrap().id, 1);
        assert!(b.point_for(&points, &pad("PA7")).is_none());
        let cn7_38 = b.connectors[0].rows[18][1].as_ref().unwrap();
        assert_eq!(b.point_for(&points, cn7_38).unwrap().id, 2);
    }

    #[test]
    fn preset_without_geometry() {
        let b = Board::parse(
            r#"
            [board]
            name = "test"
            [[view.connector]]
            ref = "J1"
            columns = 2
            nets = ["A", "B", "", "GND", "C"]
            "#,
            Path::new("."),
        )
        .unwrap();
        let j1 = &b.connectors[0];
        assert_eq!(j1.label, "J1");
        assert_eq!(j1.rows.len(), 3);
        assert_eq!(net(j1, 1, 0), ("3", "-"));
        assert_eq!(net(j1, 2, 0), ("5", "C"));
        assert_eq!(b.columns(), [vec![0]]);
        let err = Board::parse(
            "[board]\nname = \"x\"\n[[view.connector]]\nref = \"J1\"",
            Path::new("."),
        )
        .unwrap_err();
        assert!(err.to_string().contains("geometry"), "{err}");
    }
}
