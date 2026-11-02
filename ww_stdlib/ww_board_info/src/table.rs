//! Answering [BoardInfo::REWORKS](crate::BoardInfo::REWORKS) and
//! [BoardInfo::set_reworks](crate::BoardInfo::set_reworks) from the firmware's own rework table (STD-98).
//!
//! The firmware keeps the titles in a [ReworkTable] (flash, never the record); the unit's record keeps a bit per
//! number, a [ReworkSet]. Both calls then answer the same on every board: [ReworkTable::apply] checks a request
//! (unknown numbers, conflicts, all or nothing), [ReworkTable::list] builds the list, with the applied numbers the
//! table doesn't have at the end, `title: None`.
//!
//! ```
//! use ww_board_info::{ReworkId, ReworkState, ReworkError, table::{ReworkDef, ReworkSet, ReworkTable}};
//!
//! const TABLE: ReworkTable = ReworkTable::new(
//!     &[ReworkDef::new(1, "analog buffer supply"), ReworkDef::new(5, "SD full swap"), ReworkDef::new(8, "SD short swap")],
//!     &[(ReworkId(5), ReworkId(8))],
//! );
//! let stored = ReworkSet::EMPTY.with(ReworkId(5), true);
//! let set = |id, applied| ReworkState { id: ReworkId(id), applied };
//! assert_eq!(TABLE.apply(stored, [set(7, true)]), Err(ReworkError::UnknownRework { id: ReworkId(7) }));
//! assert_eq!(
//!     TABLE.apply(stored, [set(8, true)]),
//!     Err(ReworkError::Conflict { id: ReworkId(8), with: ReworkId(5) })
//! );
//! let swapped = TABLE.apply(stored, [set(5, false), set(8, true)]).unwrap();
//! assert_eq!(swapped, ReworkSet::EMPTY.with(ReworkId(8), true));
//! assert_eq!(TABLE.known_ids().to_string(), "1, 5, 8");
//! ```

use core::fmt;

use wire_weaver::prelude::MaxLen;

use crate::{MAX_REWORKS, Rework, ReworkError, ReworkId, ReworkState};

/// Longest title a [Rework] carries.
pub const MAX_TITLE: usize = 48;

/// One rework the firmware knows: its number in the board revision's rework list and what it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReworkDef {
    pub id: ReworkId,
    pub title: &'static str,
}

impl ReworkDef {
    /// Panics (at compile time in a `const`) when the number doesn't fit a [ReworkSet] or the title is longer than
    /// [MAX_TITLE].
    pub const fn new(id: u16, title: &'static str) -> Self {
        assert!(
            (id as u32) < ReworkSet::BITS,
            "rework number doesn't fit a ReworkSet"
        );
        assert!(
            title.len() <= MAX_TITLE,
            "rework title longer than 48 bytes"
        );
        ReworkDef {
            id: ReworkId(id),
            title,
        }
    }
}

/// The reworks applied to a unit, a bit per number (0-63): what a record stores.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct ReworkSet(pub u64);

impl ReworkSet {
    pub const EMPTY: ReworkSet = ReworkSet(0);
    /// Numbers a set holds: 0 to `BITS - 1`.
    pub const BITS: u32 = u64::BITS;

    pub const fn contains(self, id: ReworkId) -> bool {
        (id.0 as u32) < Self::BITS && self.0 & (1 << id.0) != 0
    }

    /// The set with `id` applied or not; a number past [Self::BITS] leaves it as it is.
    pub const fn with(self, id: ReworkId, applied: bool) -> Self {
        if (id.0 as u32) >= Self::BITS {
            return self;
        }
        if applied {
            ReworkSet(self.0 | (1 << id.0))
        } else {
            ReworkSet(self.0 & !(1 << id.0))
        }
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The numbers in the set, lowest first.
    pub fn iter(self) -> impl Iterator<Item = ReworkId> {
        (0..Self::BITS as u16)
            .map(ReworkId)
            .filter(move |&id| self.contains(id))
    }
}

/// The reworks a firmware knows for the board revision it runs on, and the pairs that can't both be applied.
#[derive(Clone, Copy, Debug)]
pub struct ReworkTable<'a> {
    pub reworks: &'a [ReworkDef],
    pub conflicts: &'a [(ReworkId, ReworkId)],
}

impl<'a> ReworkTable<'a> {
    pub const fn new(reworks: &'a [ReworkDef], conflicts: &'a [(ReworkId, ReworkId)]) -> Self {
        ReworkTable { reworks, conflicts }
    }

    pub fn get(&self, id: ReworkId) -> Option<&'a ReworkDef> {
        self.reworks.iter().find(|r| r.id == id)
    }

    /// Every number of the table.
    pub fn known(&self) -> ReworkSet {
        self.reworks
            .iter()
            .fold(ReworkSet::EMPTY, |s, r| s.with(r.id, true))
    }

    /// The applied numbers this firmware doesn't know (written by a newer one): while there is one, the board runs
    /// on its safe features.
    pub fn unknown(&self, applied: ReworkSet) -> ReworkSet {
        ReworkSet(applied.0 & !self.known().0)
    }

    /// The numbers the table knows, `1-5, 7, 8`, for messages.
    pub fn known_ids(&self) -> IdRanges {
        IdRanges(self.known())
    }

    /// The set after `changes`, every change or none: a number the table doesn't have is
    /// [ReworkError::UnknownRework] (also to undo it: the firmware can't judge it), a result with both reworks of a
    /// conflicting pair [ReworkError::Conflict]. Applied numbers the table doesn't know stay as they are.
    pub fn apply(
        &self,
        applied: ReworkSet,
        changes: impl IntoIterator<Item = ReworkState>,
    ) -> Result<ReworkSet, ReworkError<'static>> {
        let mut result = applied;
        let mut changed = ReworkSet::EMPTY;
        for change in changes {
            if self.get(change.id).is_none() {
                return Err(ReworkError::UnknownRework { id: change.id });
            }
            result = result.with(change.id, change.applied);
            changed = changed.with(change.id, true);
        }
        for &(a, b) in self.conflicts {
            if result.contains(a) && result.contains(b) {
                // name the one this call applied first
                let (id, with) = if changed.contains(b) && !changed.contains(a) {
                    (b, a)
                } else {
                    (a, b)
                };
                return Err(ReworkError::Conflict { id, with });
            }
        }
        Ok(result)
    }

    /// The `REWORKS` list of a unit with `applied`: the table in its order, then the applied numbers it doesn't
    /// have, `title: None`; at most [MAX_REWORKS].
    pub fn list(&self, applied: ReworkSet) -> impl Iterator<Item = Rework<'a>> + use<'a> {
        let known = self.reworks.iter().map(move |r| Rework {
            id: r.id,
            title: Some(MaxLen::new_unchecked(r.title)),
            applied: applied.contains(r.id),
        });
        let unknown = self.unknown(applied).iter().map(|id| Rework {
            id,
            title: None,
            applied: true,
        });
        known.chain(unknown).take(MAX_REWORKS)
    }
}

/// Rework numbers written as ranges, `1-5, 7, 8`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdRanges(pub ReworkSet);

impl fmt::Display for IdRanges {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut ids = self.0.iter().map(|id| id.0).peekable();
        let mut first = true;
        while let Some(start) = ids.next() {
            let mut end = start;
            while ids.peek() == Some(&(end + 1)) {
                end = ids.next().unwrap_or(end);
            }
            if !first {
                f.write_str(", ")?;
            }
            first = false;
            match end - start {
                0 => write!(f, "{start}")?,
                1 => write!(f, "{start}, {end}")?,
                _ => write!(f, "{start}-{end}")?,
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;
    use std::{string::ToString, vec::Vec};

    /// B200A's list (`b200_board::provisional`): 6 is a deviation, not a rework; 5 and 8 rewire the SD lines two ways.
    const B200A: ReworkTable = ReworkTable::new(
        &[
            ReworkDef::new(1, "analog buffer supply"),
            ReworkDef::new(2, "595 ~OE pull-up"),
            ReworkDef::new(3, "1.8 V LDO (TLV70218)"),
            ReworkDef::new(4, "button pull-down"),
            ReworkDef::new(5, "SD full swap: CK 2, CMD 3, D0-D3 4-7"),
            ReworkDef::new(7, "I2C low path bypass, no 5 V I2C"),
            ReworkDef::new(8, "SD short swap: D0-D3 2-5, CMD 6, CK 7"),
        ],
        &[(ReworkId(5), ReworkId(8))],
    );

    fn set(ids: &[u16]) -> ReworkSet {
        ids.iter()
            .fold(ReworkSet::EMPTY, |s, &id| s.with(ReworkId(id), true))
    }

    fn change(id: u16, applied: bool) -> ReworkState {
        ReworkState {
            id: ReworkId(id),
            applied,
        }
    }

    #[test]
    fn set_and_undo() {
        let r = B200A.apply(set(&[1, 2]), [change(7, true), change(1, false)]);
        assert_eq!(r, Ok(set(&[2, 7])));
        assert_eq!(B200A.apply(set(&[2]), []), Ok(set(&[2])));
    }

    #[test]
    fn unknown_is_refused_and_nothing_changes() {
        for id in [6, 9, 63, 64, 1000] {
            assert_eq!(
                B200A.apply(set(&[1]), [change(7, true), change(id, true)]),
                Err(ReworkError::UnknownRework { id: ReworkId(id) })
            );
        }
        // undoing a number the firmware doesn't know is refused too
        assert_eq!(
            B200A.apply(set(&[1, 9]), [change(9, false)]),
            Err(ReworkError::UnknownRework { id: ReworkId(9) })
        );
    }

    #[test]
    fn conflicts_name_the_new_one() {
        assert_eq!(
            B200A.apply(set(&[5]), [change(8, true)]),
            Err(ReworkError::Conflict {
                id: ReworkId(8),
                with: ReworkId(5)
            })
        );
        assert_eq!(
            B200A.apply(set(&[8]), [change(5, true)]),
            Err(ReworkError::Conflict {
                id: ReworkId(5),
                with: ReworkId(8)
            })
        );
        assert_eq!(
            B200A.apply(set(&[5]), [change(5, false), change(8, true)]),
            Ok(set(&[8]))
        );
    }

    #[test]
    fn unknown_applied_numbers_stay_and_are_listed() {
        let stored = set(&[1, 7, 9, 40]);
        assert_eq!(B200A.unknown(stored), set(&[9, 40]));
        assert_eq!(
            B200A.apply(stored, [change(2, true)]),
            Ok(set(&[1, 2, 7, 9, 40]))
        );
        let list: Vec<_> = B200A
            .list(stored)
            .map(|r| (r.id.0, r.title.map(|t| *t), r.applied))
            .collect();
        assert_eq!(list.len(), 9);
        assert_eq!(list[0], (1, Some("analog buffer supply"), true));
        assert!(!list[1].2);
        assert_eq!(list[5], (7, Some("I2C low path bypass, no 5 V I2C"), true));
        assert_eq!(&list[7..], [(9, None, true), (40, None, true)]);
    }

    #[test]
    fn list_is_bounded() {
        let defs: Vec<_> = (0..20).map(|id| ReworkDef::new(id, "x")).collect();
        let table = ReworkTable::new(&defs, &[]);
        assert_eq!(table.list(set(&[30, 31])).count(), MAX_REWORKS);
    }

    #[test]
    fn ranges() {
        assert_eq!(B200A.known_ids().to_string(), "1-5, 7, 8");
        assert_eq!(IdRanges(set(&[])).to_string(), "");
        assert_eq!(IdRanges(set(&[3])).to_string(), "3");
        assert_eq!(
            IdRanges(set(&[0, 1, 2, 10, 11, 63])).to_string(),
            "0-2, 10, 11, 63"
        );
    }
}
