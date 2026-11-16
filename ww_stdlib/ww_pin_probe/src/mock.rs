//! A simulated board for tests: points on nets, nets with external pulls or shorted to a rail, points held by a
//! function, pairs of outputs that must never both be high. Answers like a firmware serving `PinDiag` would.

use crate::{Drive, Level, PinDiagErrorOwned, PinError, Pins, Point, PointCaps, PointState, Pull};
use wire_weaver::prelude::CommonErrorOwned;

/// What the board puts on a net besides the points.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Net {
    Open,
    PullUp,
    PullDown,
    /// Shorted to GND.
    Gnd,
    /// Shorted to the supply.
    Vdd,
}

/// One point of the mock board.
#[derive(Clone, Debug)]
pub struct MockPoint {
    pub name: String,
    pub group: String,
    pub caps: PointCaps,
    /// Index into [MockBoard::nets].
    pub net: usize,
    /// Held by a function: `set` answers `InUse`.
    pub held_by: Option<String>,
}

/// The board, see the [module](self) documentation.
#[derive(Clone, Debug, Default)]
pub struct MockBoard {
    pub points: Vec<MockPoint>,
    pub nets: Vec<Net>,
    /// Pairs of points never both driving high, refused with the rule's name.
    pub interlocks: Vec<(u16, u16, String)>,
    pub states: Vec<Option<PointState>>,
    /// Every `set` that went through, in order.
    pub sets: Vec<(u16, PointState)>,
    /// Calls answered so far, and after how many calls the link fails (`None`: never).
    pub calls: usize,
    pub fail_after: Option<usize>,
    pub releases: usize,
}

/// Every cap.
pub const ALL: PointCaps = PointCaps {
    read: true,
    pull_up: true,
    pull_down: true,
    drive_low: true,
    drive_high: true,
    weak_drive: true,
};

impl MockBoard {
    /// Adds a point on a net of its own with `net` on it; returns its id.
    pub fn point(&mut self, name: &str, group: &str, caps: PointCaps, net: Net) -> u16 {
        self.nets.push(net);
        self.on_net(name, group, caps, self.nets.len() - 1)
    }

    /// Adds a point on the net of an earlier point (shorted to it); returns its id.
    pub fn shorted_to(&mut self, name: &str, group: &str, caps: PointCaps, other: u16) -> u16 {
        let net = self.points[other as usize].net;
        self.on_net(name, group, caps, net)
    }

    fn on_net(&mut self, name: &str, group: &str, caps: PointCaps, net: usize) -> u16 {
        self.points.push(MockPoint {
            name: name.into(),
            group: group.into(),
            caps,
            net,
            held_by: None,
        });
        self.states.push(None);
        (self.points.len() - 1) as u16
    }

    /// The points as a host sees them.
    pub fn host_points(&self) -> Vec<Point> {
        self.points
            .iter()
            .enumerate()
            .map(|(id, p)| Point {
                id: id as u16,
                name: p.name.clone(),
                group: p.group.clone(),
                caps: p.caps,
            })
            .collect()
    }

    /// Whether `point` was ever set.
    pub fn was_set(&self, point: u16) -> bool {
        self.sets.iter().any(|(p, _)| *p == point)
    }

    /// The level of a net: a rail wins, then a normal driver over a weak one (low between equals), then the
    /// external pull, then internal pulls (low between equals); floating reads low.
    pub fn net_level(&self, net: usize) -> Level {
        match self.nets[net] {
            Net::Gnd => return Level::Low,
            Net::Vdd => return Level::High,
            _ => {}
        }
        let on_net = || {
            self.points
                .iter()
                .zip(&self.states)
                .filter(move |(p, _)| p.net == net)
                .filter_map(|(_, s)| *s)
        };
        let mut best: Option<(bool, Level)> = None;
        for s in on_net() {
            if let PointState::Output { level, drive } = s {
                let strong = drive != Drive::Weak;
                best = match best {
                    Some((s, l)) if s && !strong => Some((s, l)),
                    Some((s, Level::Low)) if s == strong => Some((s, Level::Low)),
                    _ => Some((strong, level)),
                };
            }
        }
        if let Some((_, level)) = best {
            return level;
        }
        match self.nets[net] {
            Net::PullUp => return Level::High,
            Net::PullDown => return Level::Low,
            _ => {}
        }
        let pulls = |pull| {
            on_net()
                .filter(|s| *s == PointState::Input { pull })
                .count()
        };
        if pulls(Pull::Up) > pulls(Pull::Down) {
            Level::High
        } else {
            Level::Low
        }
    }

    fn call(&mut self) -> Result<(), PinError> {
        self.calls += 1;
        match self.fail_after {
            Some(n) if self.calls > n => Err(PinError::Failed("link lost".into())),
            _ => Ok(()),
        }
    }

    fn high(&self, point: u16) -> bool {
        matches!(
            self.states[point as usize],
            Some(PointState::Output {
                level: Level::High,
                ..
            })
        )
    }
}

fn refused(e: PinDiagErrorOwned) -> Result<(), PinError> {
    Err(PinError::Refused(e))
}

impl Pins for MockBoard {
    async fn set(&mut self, point: u16, state: PointState) -> Result<(), PinError> {
        self.call()?;
        let Some(p) = self.points.get(point as usize) else {
            return refused(PinDiagErrorOwned::NoSuchPoint);
        };
        let capable = match state {
            PointState::Input { pull: Pull::None } => p.caps.read,
            PointState::Input { pull: Pull::Up } => p.caps.pull_up,
            PointState::Input { pull: Pull::Down } => p.caps.pull_down,
            PointState::Output {
                level: Level::Low, ..
            } => p.caps.drive_low,
            PointState::Output {
                level: Level::High, ..
            } => p.caps.drive_high,
            _ => false,
        };
        if !capable {
            return refused(PinDiagErrorOwned::NotCapable);
        }
        if let Some(by) = &p.held_by {
            return refused(PinDiagErrorOwned::Common(CommonErrorOwned::InUse {
                by: wire_weaver::prelude::MaxLen::new_unchecked(by.clone()),
            }));
        }
        if matches!(
            state,
            PointState::Output {
                level: Level::High,
                ..
            }
        ) {
            for (a, b, rule) in &self.interlocks {
                let other = if *a == point {
                    *b
                } else if *b == point {
                    *a
                } else {
                    continue;
                };
                if self.high(other) {
                    return refused(PinDiagErrorOwned::Refused {
                        rule: ww_pin_diag::InternedOwned::Text(rule.clone()),
                    });
                }
            }
        }
        self.states[point as usize] = Some(state);
        self.sets.push((point, state));
        Ok(())
    }

    async fn read(&mut self, point: u16) -> Result<Level, PinError> {
        self.call()?;
        match self.points.get(point as usize) {
            None => Err(PinError::Refused(PinDiagErrorOwned::NoSuchPoint)),
            Some(p) if !p.caps.read => Err(PinError::Refused(PinDiagErrorOwned::NotCapable)),
            Some(p) => Ok(self.net_level(p.net)),
        }
    }

    async fn release_all(&mut self) -> Result<(), PinError> {
        // a link that failed stays failed, except for this one call: the tests check it is made
        self.releases += 1;
        self.states.iter_mut().for_each(|s| *s = None);
        Ok(())
    }
}
