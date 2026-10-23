//! The live view of every point (`ww pins tui`, TOOL-21) without the terminal: where each point is shown
//! ([Layout], from a [Board] preset or per group), the cursor moving over it ([Cursor]), and what the view knows of
//! each point and does to it ([Live]: read all, set one, release all), over [Pins] so it runs on a mock board too.

use crate::board::{Board, Pad};
use crate::{Drive, Level, PinError, Pins, Point, PointState, Pull, refusal};

/// One place in a panel: a connector's pad (with the point on it, if the device has one) or a point on no pad.
#[derive(Clone, Debug, PartialEq)]
pub struct Slot {
    /// `None` for a point listed apart (no preset, or on no connector of it).
    pub pad: Option<Pad>,
    pub point: Option<u16>,
}

/// A connector of the preset, or a list of points: rows of slots, `None` where a grid has no pad.
#[derive(Clone, Debug, PartialEq)]
pub struct Panel {
    pub title: String,
    pub rows: Vec<Vec<Option<Slot>>>,
}

impl Panel {
    fn width(&self) -> usize {
        self.rows.iter().map(Vec::len).max().unwrap_or(0)
    }
}

/// Panels in columns, left to right; each column's panels top to bottom.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub panels: Vec<Panel>,
    pub columns: Vec<Vec<usize>>,
}

impl Layout {
    /// With a preset: its connectors in board order, then the points on none of them (`not on a connector`).
    /// Without one: a panel per group, in the order the device lists them.
    pub fn new(board: Option<&Board>, points: &[Point]) -> Self {
        let mut panels = vec![];
        let mut columns = vec![];
        let mut shown = vec![false; points.len()];
        if let Some(board) = board {
            for c in &board.connectors {
                let rows = c
                    .rows
                    .iter()
                    .map(|row| {
                        row.iter()
                            .map(|pad| {
                                pad.as_ref().map(|pad| {
                                    let point = board.point_for(points, pad).map(|p| p.id);
                                    if let Some(id) = point {
                                        shown[id as usize] = true;
                                    }
                                    Slot {
                                        pad: Some(pad.clone()),
                                        point,
                                    }
                                })
                            })
                            .collect()
                    })
                    .collect();
                panels.push(Panel {
                    title: c.label.clone(),
                    rows,
                });
            }
            columns = board.columns();
            let rest: Vec<&Point> = points.iter().filter(|p| !shown[p.id as usize]).collect();
            if !rest.is_empty() {
                columns.push(vec![panels.len()]);
                panels.push(list("not on a connector".into(), rest));
            }
        } else {
            let mut groups: Vec<&str> = vec![];
            for p in points {
                if !groups.contains(&p.group.as_str()) {
                    groups.push(&p.group);
                }
            }
            for g in groups {
                columns.push(vec![panels.len()]);
                let title = if g.is_empty() { "points" } else { g };
                panels.push(list(
                    title.into(),
                    points.iter().filter(|p| p.group == g).collect(),
                ));
            }
        }
        Layout { panels, columns }
    }

    /// Column of a panel.
    fn column_of(&self, panel: usize) -> usize {
        self.columns
            .iter()
            .position(|c| c.contains(&panel))
            .unwrap_or(0)
    }

    /// The first place with a point on it.
    pub fn first(&self) -> Option<Cursor> {
        for col in &self.columns {
            for &panel in col {
                for (row, cells) in self.panels[panel].rows.iter().enumerate() {
                    for (c, slot) in cells.iter().enumerate() {
                        if slot.as_ref().is_some_and(|s| s.point.is_some()) {
                            return Some(Cursor { panel, row, col: c });
                        }
                    }
                }
            }
        }
        None
    }

    pub fn slot(&self, at: Cursor) -> Option<&Slot> {
        self.panels
            .get(at.panel)?
            .rows
            .get(at.row)?
            .get(at.col)?
            .as_ref()
    }

    /// Where `point` is shown.
    pub fn find(&self, point: u16) -> Option<Cursor> {
        self.panels.iter().enumerate().find_map(|(panel, p)| {
            p.rows.iter().enumerate().find_map(|(row, cells)| {
                cells
                    .iter()
                    .position(|s| s.as_ref().is_some_and(|s| s.point == Some(point)))
                    .map(|col| Cursor { panel, row, col })
            })
        })
    }

    /// The cursor moved one step; across a panel's edge into the next panel that way.
    pub fn step(&self, at: Cursor, dir: Dir) -> Cursor {
        let panel = &self.panels[at.panel];
        let column = self.column_of(at.panel);
        let in_col = &self.columns[column];
        let pos = in_col.iter().position(|&p| p == at.panel).unwrap_or(0);
        let enter = |panel: usize, row: Option<usize>, col: Option<usize>| {
            let p = &self.panels[panel];
            let row = row.unwrap_or(0).min(p.rows.len().saturating_sub(1));
            let col = col.unwrap_or(0).min(p.width().saturating_sub(1));
            Cursor { panel, row, col }
        };
        match dir {
            Dir::Up if at.row > 0 => Cursor {
                row: at.row - 1,
                ..at
            },
            Dir::Up if pos > 0 => {
                let up = in_col[pos - 1];
                enter(up, Some(usize::MAX), Some(at.col))
            }
            Dir::Down if at.row + 1 < panel.rows.len() => Cursor {
                row: at.row + 1,
                ..at
            },
            Dir::Down if pos + 1 < in_col.len() => enter(in_col[pos + 1], None, Some(at.col)),
            Dir::Left if at.col > 0 => Cursor {
                col: at.col - 1,
                ..at
            },
            Dir::Left if column > 0 => {
                let next = self.panel_beside(column - 1, at);
                enter(next, Some(at.row), Some(usize::MAX))
            }
            Dir::Right if at.col + 1 < panel.width() => Cursor {
                col: at.col + 1,
                ..at
            },
            Dir::Right if column + 1 < self.columns.len() => {
                let next = self.panel_beside(column + 1, at);
                enter(next, Some(at.row), None)
            }
            _ => at,
        }
    }

    /// The next panel in the list (`Tab`), or the previous one.
    pub fn next_panel(&self, at: Cursor, back: bool) -> Cursor {
        let order: Vec<usize> = self.columns.iter().flatten().copied().collect();
        let pos = order.iter().position(|&p| p == at.panel).unwrap_or(0);
        let n = order.len();
        let panel = order[if back {
            (pos + n - 1) % n
        } else {
            (pos + 1) % n
        }];
        let p = &self.panels[panel];
        let first = p
            .rows
            .iter()
            .enumerate()
            .find_map(|(row, cells)| {
                cells
                    .iter()
                    .position(|s| s.as_ref().is_some_and(|s| s.point.is_some()))
                    .map(|col| (row, col))
            })
            .unwrap_or((0, 0));
        Cursor {
            panel,
            row: first.0,
            col: first.1,
        }
    }

    /// The panel of `column` at the same height as `at`'s panel: the one with as many panels above it, clamped.
    fn panel_beside(&self, column: usize, at: Cursor) -> usize {
        let from = &self.columns[self.column_of(at.panel)];
        let pos = from.iter().position(|&p| p == at.panel).unwrap_or(0);
        let to = &self.columns[column];
        to[pos.min(to.len() - 1)]
    }
}

fn list(title: String, points: Vec<&Point>) -> Panel {
    Panel {
        title,
        rows: points
            .into_iter()
            .map(|p| {
                vec![Some(Slot {
                    pad: None,
                    point: Some(p.id),
                })]
            })
            .collect(),
    }
}

/// Where the cursor is: panel, row and column in it.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Cursor {
    pub panel: usize,
    pub row: usize,
    pub col: usize,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

/// What the view knows of one point.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PointLive {
    /// Pad level at the last read.
    pub level: Option<Level>,
    /// Why the last read didn't give one.
    pub read_refused: Option<String>,
    /// What the view set it to since the last release.
    pub set: Option<PointState>,
}

/// What a key asks for on the point under the cursor, or on all.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// Input with this pull.
    Input(Pull),
    /// Drive a level, weakly when the view's weak drive is on.
    Drive(Level),
    /// Drive the opposite of what it drives, or of the level it reads.
    Toggle,
    /// Weak drive on or off, for the drives that follow.
    Weak,
    /// Read every point now.
    ReadAll,
    /// Every point back to its function.
    Release,
}

/// The view's state, see the [module](self) documentation.
#[derive(Clone, Debug)]
pub struct Live {
    pub points: Vec<Point>,
    pub state: Vec<PointLive>,
    /// Drives are weak (`Drive::Weak`).
    pub weak: bool,
    /// The last thing to tell: a refusal or what a command did; `true` for a refusal.
    pub message: Option<(bool, String)>,
    /// Full reads done.
    pub reads: u64,
}

impl Live {
    pub fn new(points: Vec<Point>) -> Self {
        let state = vec![PointLive::default(); points.len()];
        Live {
            points,
            state,
            weak: false,
            message: None,
            reads: 0,
        }
    }

    fn name(&self, id: u16) -> &str {
        self.points
            .get(id as usize)
            .map_or("?", |p| p.name.as_str())
    }

    /// Reads every point that can be read. A refusal is kept per point; `Err` when the link failed.
    pub async fn read_all(&mut self, pins: &mut impl Pins) -> Result<(), String> {
        for p in &self.points {
            let s = &mut self.state[p.id as usize];
            if !p.caps.read {
                s.level = None;
                s.read_refused = Some("can't be read".into());
                continue;
            }
            match pins.read(p.id).await {
                Ok(level) => {
                    s.level = Some(level);
                    s.read_refused = None;
                }
                Err(PinError::Refused(e)) => {
                    s.level = None;
                    s.read_refused = Some(refusal(&e));
                }
                Err(PinError::Failed(e)) => return Err(e),
            }
        }
        self.reads += 1;
        Ok(())
    }

    /// Does `cmd`, on `point` where it takes one. A refusal goes into [Live::message]; `Err` when the link failed.
    pub async fn command(
        &mut self,
        pins: &mut impl Pins,
        point: Option<u16>,
        cmd: Command,
    ) -> Result<(), String> {
        let state = match cmd {
            Command::ReadAll => return self.read_all(pins).await,
            Command::Release => return self.release(pins).await,
            Command::Weak => {
                self.weak = !self.weak;
                let how = if self.weak { "weak" } else { "normal" };
                self.message = Some((false, format!("drives are {how} from now on")));
                return Ok(());
            }
            Command::Input(pull) => PointState::Input { pull },
            Command::Drive(level) => self.output(level),
            Command::Toggle => {
                let Some(id) = point else {
                    return self.no_point();
                };
                let s = &self.state[id as usize];
                let now = match s.set {
                    Some(PointState::Output { level, .. }) => Some(level),
                    _ => s.level,
                };
                let level = match now {
                    Some(Level::High) => Level::Low,
                    _ => Level::High,
                };
                self.output(level)
            }
        };
        let Some(id) = point else {
            return self.no_point();
        };
        match pins.set(id, state).await {
            Ok(()) => {
                self.state[id as usize].set = Some(state);
                self.message = Some((false, format!("{}: {}", self.name(id), describe(state))));
                // the pad as it is now, not at the next refresh
                if self.points[id as usize].caps.read {
                    match pins.read(id).await {
                        Ok(level) => self.state[id as usize].level = Some(level),
                        Err(PinError::Refused(_)) => {}
                        Err(PinError::Failed(e)) => return Err(e),
                    }
                }
                Ok(())
            }
            Err(PinError::Refused(e)) => {
                self.message = Some((
                    true,
                    format!("{} {}: {}", self.name(id), describe(state), refusal(&e)),
                ));
                Ok(())
            }
            Err(PinError::Failed(e)) => Err(e),
        }
    }

    /// Every point back to its function.
    pub async fn release(&mut self, pins: &mut impl Pins) -> Result<(), String> {
        pins.release_all().await.map_err(|e| e.to_string())?;
        let n = self.state.iter().filter(|s| s.set.is_some()).count();
        self.state.iter_mut().for_each(|s| s.set = None);
        self.message = Some((false, format!("released, {n} point(s) were set")));
        Ok(())
    }

    /// Points set since the last release.
    pub fn held(&self) -> usize {
        self.state.iter().filter(|s| s.set.is_some()).count()
    }

    fn output(&self, level: Level) -> PointState {
        let drive = if self.weak {
            Drive::Weak
        } else {
            Drive::Normal
        };
        PointState::Output { level, drive }
    }

    fn no_point(&mut self) -> Result<(), String> {
        self.message = Some((true, "no point of the device here".into()));
        Ok(())
    }
}

/// `drive high`, `input, pull-up`, `drive low (weak)`.
pub fn describe(state: PointState) -> String {
    match state {
        PointState::Input { pull } => match pull {
            Pull::None => "input".into(),
            Pull::Up => "input, pull-up".into(),
            Pull::Down => "input, pull-down".into(),
            Pull::Custom(c) => format!("input, pull {c}"),
        },
        PointState::Output { level, drive } => {
            let l = match level {
                Level::High => "high",
                Level::Low => "low",
            };
            match drive {
                Drive::Normal => format!("drive {l}"),
                Drive::Weak => format!("drive {l} (weak)"),
                Drive::Unknown(d) => format!("drive {l} (drive {d})"),
            }
        }
        PointState::Unknown(s) => format!("state {s}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::{ALL, MockBoard, Net};
    use std::path::Path;

    fn board() -> MockBoard {
        let mut b = MockBoard::default();
        b.point("PA5", "mcu", ALL, Net::Open);
        b.point("PC13", "mcu", ALL, Net::PullUp);
        b.point("PB12", "mcu", ALL, Net::Open);
        b.point("EX0", "chain", ALL, Net::Open);
        b.points[1].held_by = Some("button".into());
        b
    }

    fn nucleo() -> Board {
        Board::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/nucleo_g0b1re.toml"))
            .unwrap()
    }

    #[test]
    fn layout_by_preset_and_by_group() {
        let b = board();
        let points = b.host_points();
        let l = Layout::new(Some(&nucleo()), &points);
        assert_eq!(l.panels.len(), 9);
        assert_eq!(l.panels[8].title, "not on a connector");
        assert_eq!(l.columns.last().unwrap(), &vec![8]);
        // PB12 on CN7-38 (through SB3) and PA5 on CN10-11 and CN5-6
        assert_eq!(
            l.find(2),
            Some(Cursor {
                panel: 0,
                row: 18,
                col: 1
            })
        );
        let at = l.find(0).unwrap();
        assert_eq!(l.panels[at.panel].title, "CN10 ST morpho");
        let l = Layout::new(None, &points);
        let titles: Vec<&str> = l.panels.iter().map(|p| p.title.as_str()).collect();
        assert_eq!(titles, ["mcu", "chain"]);
        assert_eq!(
            l.first(),
            Some(Cursor {
                panel: 0,
                row: 0,
                col: 0
            })
        );
    }

    #[test]
    fn cursor_moves_across_panels() {
        let b = board();
        let l = Layout::new(Some(&nucleo()), &b.host_points());
        let cn7 = Cursor {
            panel: 0,
            row: 0,
            col: 0,
        };
        assert_eq!(l.step(cn7, Dir::Up), cn7);
        assert_eq!(l.step(cn7, Dir::Left), cn7);
        let right = l.step(cn7, Dir::Right);
        assert_eq!(right, Cursor { col: 1, ..cn7 });
        // past CN7's right column into CN11, the next column of the board
        let cn11 = l.step(right, Dir::Right);
        assert_eq!(l.panels[cn11.panel].title, "CN11 SWD out");
        // CN6 above CN8 in one column
        let cn6 = l.columns[2][0];
        let bottom = Cursor {
            panel: cn6,
            row: l.panels[cn6].rows.len() - 1,
            col: 0,
        };
        let down = l.step(bottom, Dir::Down);
        assert_eq!(l.panels[down.panel].title, "CN8 A0–A5");
        assert_eq!(l.step(down, Dir::Up), bottom);
        let tab = l.next_panel(cn7, false);
        assert_eq!(l.panels[tab.panel].title, "CN11 SWD out");
        assert_eq!(l.next_panel(cn7, true).panel, 8);
    }

    #[tokio::test]
    async fn drive_read_refuse_release() {
        let mut b = board();
        let mut live = Live::new(b.host_points());
        live.read_all(&mut b).await.unwrap();
        assert_eq!(live.state[1].level, Some(Level::High));
        assert_eq!(live.state[0].level, Some(Level::Low));
        live.command(&mut b, Some(0), Command::Drive(Level::High))
            .await
            .unwrap();
        assert_eq!(live.state[0].level, Some(Level::High));
        assert_eq!(live.message, Some((false, "PA5: drive high".into())));
        live.command(&mut b, Some(0), Command::Toggle)
            .await
            .unwrap();
        assert_eq!(
            live.state[0].set,
            Some(PointState::Output {
                level: Level::Low,
                drive: Drive::Normal
            })
        );
        live.command(&mut b, Some(1), Command::Drive(Level::Low))
            .await
            .unwrap();
        assert_eq!(
            live.message,
            Some((true, "PC13 drive low: in use by button".into()))
        );
        assert_eq!(live.state[1].set, None);
        live.command(&mut b, None, Command::Weak).await.unwrap();
        live.command(&mut b, Some(2), Command::Input(Pull::Up))
            .await
            .unwrap();
        live.command(&mut b, Some(3), Command::Drive(Level::High))
            .await
            .unwrap();
        assert_eq!(
            b.sets.last(),
            Some(&(
                3,
                PointState::Output {
                    level: Level::High,
                    drive: Drive::Weak
                }
            ))
        );
        assert_eq!(live.held(), 3);
        live.command(&mut b, None, Command::Release).await.unwrap();
        assert_eq!(live.held(), 0);
        assert_eq!(b.releases, 1);
        assert_eq!(
            live.message,
            Some((false, "released, 3 point(s) were set".into()))
        );
        b.fail_after = Some(b.calls);
        assert_eq!(live.read_all(&mut b).await, Err("link lost".into()));
    }
}
