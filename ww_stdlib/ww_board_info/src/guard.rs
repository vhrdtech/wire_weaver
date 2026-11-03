//! The rework guard (STD-101): a firmware never runs a board with a rework it doesn't know.
//!
//! At boot the firmware calls [decide] with the reworks it knows for the board revision it runs on (read from its
//! image record, [crate::image::ImageReworks::known], so the record the flasher reads is the one the firmware uses),
//! the reworks the record has applied, the stored
//! allowance and its own [ImageId]. The answer is one of:
//!
//! * [Guard::Normal]: every applied rework is known, run the board.
//! * [Guard::Guarded]: an applied rework is unknown and no allowance for this image covers it. Serve identity, info,
//!   updates and reading the record only; keep every output in its safe state.
//! * [Guard::Allowed]: unknown reworks, but the allowance for this image covers them: run the board.
//!
//! [Decision::erase_allowance] says when the stored allowance is for another image: erase it, so it can't come back
//! when that image is flashed again by a tool that doesn't clear it. The allowance itself is two `u64`
//! ([StoredAllowance]), stored next to the record.
//!
//! ```
//! use ww_board_info::{ImageId, ReworkId, guard::{decide, Guard, StoredAllowance}, table::{ReworkDef, ReworkSet, ReworkTable}};
//!
//! const TABLE: ReworkTable = ReworkTable::new(&[ReworkDef::new(1, "analog buffer supply")], &[]);
//! let me = ImageId([1, 2, 3, 4, 5, 6, 7, 8]);
//! let applied = ReworkSet::EMPTY.with(ReworkId(1), true).with(ReworkId(9), true);
//!
//! let d = decide(TABLE.known(), applied, StoredAllowance::NONE, Some(me));
//! assert_eq!(d.guard, Guard::Guarded { unknown: ReworkSet::EMPTY.with(ReworkId(9), true) });
//!
//! let allowance = StoredAllowance::new(me, ReworkSet::EMPTY.with(ReworkId(9), true));
//! assert!(matches!(decide(TABLE.known(), applied, allowance, Some(me)).guard, Guard::Allowed { .. }));
//!
//! // the same allowance is void for any other image, and that image erases it
//! let other = decide(TABLE.known(), applied, allowance, Some(ImageId([9; 8])));
//! assert!(matches!(other.guard, Guard::Guarded { .. }) && other.erase_allowance);
//! ```

use wire_weaver::prelude::{MaxLen, RefVec};

use crate::table::ReworkSet;
#[cfg(doc)]
use crate::table::ReworkTable;
use crate::{Allowance, GuardState, GuardSummary, ImageId, MAX_REWORKS, ReworkId};

/// An allowance as a board stores it: two `u64` next to the record.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct StoredAllowance {
    /// [ImageId] as a little-endian `u64`; 0: no allowance.
    pub image: u64,
    /// The unknown reworks it covers.
    pub reworks: ReworkSet,
}

impl StoredAllowance {
    pub const NONE: StoredAllowance = StoredAllowance {
        image: 0,
        reworks: ReworkSet::EMPTY,
    };

    pub const fn new(image: ImageId, reworks: ReworkSet) -> Self {
        StoredAllowance {
            image: u64::from_le_bytes(image.0),
            reworks,
        }
    }

    pub const fn is_none(&self) -> bool {
        self.image == 0
    }

    /// The image it is for, `None` when there is no allowance.
    pub const fn image_id(&self) -> Option<ImageId> {
        if self.image == 0 {
            None
        } else {
            Some(ImageId(self.image.to_le_bytes()))
        }
    }

    /// It is for `image` and covers every number of `unknown`.
    pub fn covers(&self, image: Option<ImageId>, unknown: ReworkSet) -> bool {
        match (self.image_id(), image) {
            (Some(stored), Some(image)) => stored == image && unknown.0 & !self.reworks.0 == 0,
            _ => false,
        }
    }
}

/// How the board runs, see the [module](self) docs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Guard {
    Normal,
    Guarded { unknown: ReworkSet },
    Allowed { unknown: ReworkSet },
}

impl Guard {
    /// The board's functions may run ([Guard::Normal] or [Guard::Allowed]).
    pub const fn runs_board(&self) -> bool {
        !matches!(self, Guard::Guarded { .. })
    }

    pub const fn state(&self) -> GuardState {
        match self {
            Guard::Normal => GuardState::Normal,
            Guard::Guarded { .. } => GuardState::Guarded,
            Guard::Allowed { .. } => GuardState::Allowed,
        }
    }

    /// The applied reworks the firmware doesn't know.
    pub const fn unknown(&self) -> ReworkSet {
        match self {
            Guard::Normal => ReworkSet::EMPTY,
            Guard::Guarded { unknown } | Guard::Allowed { unknown } => *unknown,
        }
    }
}

/// What [decide] found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Decision {
    pub guard: Guard,
    /// The stored allowance is for another image: erase it now.
    pub erase_allowance: bool,
}

/// The boot decision. `known`: the reworks the image knows for the board revision ([ReworkTable::known]; in
/// firmware [crate::image::ImageReworks::known], empty for a revision the image doesn't list). `own` is the running image's id ([ImageId::from_build_id] of `ww_firmware_info::build_id()`);
/// `None` (no build-id note) never matches an allowance, so such an image stays guarded on an unknown rework.
pub fn decide(
    known: ReworkSet,
    applied: ReworkSet,
    allowance: StoredAllowance,
    own: Option<ImageId>,
) -> Decision {
    let unknown = ReworkSet(applied.0 & !known.0);
    let for_us = own.is_some() && allowance.image_id() == own;
    let guard = if unknown.is_empty() {
        Guard::Normal
    } else if allowance.covers(own, unknown) {
        Guard::Allowed { unknown }
    } else {
        Guard::Guarded { unknown }
    };
    Decision {
        guard,
        erase_allowance: !allowance.is_none() && !for_us,
    }
}

/// The allowance a request to [BoardInfo::allow_unknown_reworks](crate::BoardInfo::allow_unknown_reworks) stores,
/// or the number that can't be in a record (past [ReworkSet::BITS]). An empty list stores an allowance that covers
/// nothing, which no image needs: callers clear instead.
pub fn allowance_from_request(
    image: ImageId,
    reworks: impl IntoIterator<Item = ReworkId>,
) -> Result<StoredAllowance, ReworkId> {
    let mut set = ReworkSet::EMPTY;
    for id in reworks {
        if id.0 as u32 >= ReworkSet::BITS {
            return Err(id);
        }
        set = set.with(id, true);
    }
    Ok(StoredAllowance::new(image, set))
}

/// [BoardInfo::GUARD](crate::BoardInfo::GUARD) as served: keep one from boot and answer with [Self::summary].
#[derive(Clone, Copy, Debug)]
pub struct GuardReport {
    pub guard: Guard,
    pub allowance: StoredAllowance,
    pub own: Option<ImageId>,
    unknown: [ReworkId; MAX_REWORKS],
    allowed: [ReworkId; MAX_REWORKS],
}

impl GuardReport {
    /// From the boot [Decision] and the allowance still stored after it (none if it was erased).
    pub fn new(guard: Guard, allowance: StoredAllowance, own: Option<ImageId>) -> Self {
        GuardReport {
            guard,
            allowance,
            own,
            unknown: ids(guard.unknown()),
            allowed: ids(allowance.reworks),
        }
    }

    pub fn summary(&self) -> GuardSummary<'_> {
        let unknown_len = self.guard.unknown().iter().take(MAX_REWORKS).count();
        let allowed_len = self.allowance.reworks.iter().take(MAX_REWORKS).count();
        GuardSummary {
            state: self.guard.state(),
            unknown: MaxLen::new_unchecked(RefVec::Slice {
                slice: &self.unknown[..unknown_len],
            }),
            allowance: self.allowance.image_id().map(|image| Allowance {
                image,
                reworks: MaxLen::new_unchecked(RefVec::Slice {
                    slice: &self.allowed[..allowed_len],
                }),
                for_running_image: Some(image) == self.own,
            }),
        }
    }
}

fn ids(set: ReworkSet) -> [ReworkId; MAX_REWORKS] {
    let mut out = [ReworkId(0); MAX_REWORKS];
    for (slot, id) in out.iter_mut().zip(set.iter()) {
        *slot = id;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::{ReworkDef, ReworkTable};

    const TABLE: ReworkTable = ReworkTable::new(
        &[
            ReworkDef::new(1, "a"),
            ReworkDef::new(5, "b"),
            ReworkDef::new(8, "c"),
        ],
        &[],
    );
    const ME: ImageId = ImageId([0xa1, 2, 3, 4, 5, 6, 7, 8]);
    const OTHER: ImageId = ImageId([0xb2, 2, 3, 4, 5, 6, 7, 8]);

    fn set(ids: &[u16]) -> ReworkSet {
        ids.iter()
            .fold(ReworkSet::EMPTY, |s, &id| s.with(ReworkId(id), true))
    }

    #[test]
    fn known_reworks_run_normally() {
        let d = decide(TABLE.known(), set(&[1, 8]), StoredAllowance::NONE, Some(ME));
        assert_eq!(d.guard, Guard::Normal);
        assert!(!d.erase_allowance);
        assert!(d.guard.runs_board());
    }

    #[test]
    fn unknown_guards_without_allowance() {
        let d = decide(TABLE.known(), set(&[1, 9]), StoredAllowance::NONE, Some(ME));
        assert_eq!(d.guard, Guard::Guarded { unknown: set(&[9]) });
        assert!(!d.guard.runs_board());
    }

    #[test]
    fn allowance_must_cover_every_unknown() {
        let a = StoredAllowance::new(ME, set(&[9]));
        assert_eq!(
            decide(TABLE.known(), set(&[9]), a, Some(ME)).guard,
            Guard::Allowed { unknown: set(&[9]) }
        );
        // a second unknown rework, recorded later by a newer firmware, isn't covered
        assert_eq!(
            decide(TABLE.known(), set(&[9, 10]), a, Some(ME)).guard,
            Guard::Guarded {
                unknown: set(&[9, 10])
            }
        );
        // covering more than needed is fine
        let wide = StoredAllowance::new(ME, set(&[9, 10, 11]));
        assert!(
            decide(TABLE.known(), set(&[10]), wide, Some(ME))
                .guard
                .runs_board()
        );
    }

    #[test]
    fn allowance_for_another_image_is_void_and_erased() {
        let a = StoredAllowance::new(OTHER, set(&[9]));
        let d = decide(TABLE.known(), set(&[9]), a, Some(ME));
        assert_eq!(d.guard, Guard::Guarded { unknown: set(&[9]) });
        assert!(d.erase_allowance);
        // erased also when nothing is unknown: it must not wait for its image to be flashed again
        assert!(decide(TABLE.known(), set(&[1]), a, Some(ME)).erase_allowance);
        // our own is kept
        let own = StoredAllowance::new(ME, set(&[9]));
        assert!(!decide(TABLE.known(), set(&[1]), own, Some(ME)).erase_allowance);
    }

    #[test]
    fn image_without_build_id_stays_guarded() {
        let a = StoredAllowance::new(ME, set(&[9]));
        let d = decide(TABLE.known(), set(&[9]), a, None);
        assert_eq!(d.guard, Guard::Guarded { unknown: set(&[9]) });
        assert!(d.erase_allowance);
        assert_eq!(ImageId::from_build_id(&[1, 2, 3]), None);
        assert_eq!(ImageId::from_build_id(&[0; 20]), None);
        assert_eq!(
            ImageId::from_build_id(&[0xa1, 2, 3, 4, 5, 6, 7, 8, 9]),
            Some(ME)
        );
    }

    #[test]
    fn stored_round_trip() {
        let a = StoredAllowance::new(ME, set(&[9, 63]));
        assert_eq!(a.image_id(), Some(ME));
        assert_eq!(StoredAllowance::NONE.image_id(), None);
        assert_eq!(
            allowance_from_request(ME, [ReworkId(9), ReworkId(63)]),
            Ok(a)
        );
        assert_eq!(
            allowance_from_request(ME, [ReworkId(9), ReworkId(64)]),
            Err(ReworkId(64))
        );
    }

    #[test]
    fn summary() {
        let a = StoredAllowance::new(OTHER, set(&[9]));
        let r = GuardReport::new(
            Guard::Guarded {
                unknown: set(&[9, 12]),
            },
            a,
            Some(ME),
        );
        let s = r.summary();
        assert_eq!(s.state, GuardState::Guarded);
        let unknown: std::vec::Vec<_> = s.unknown.iter().map(|id| id.unwrap().0).collect();
        assert_eq!(unknown, [9, 12]);
        let allowance = s.allowance.unwrap();
        assert_eq!(allowance.image, OTHER);
        assert!(!allowance.for_running_image);
        let r = GuardReport::new(Guard::Normal, StoredAllowance::NONE, Some(ME));
        assert!(r.summary().allowance.is_none());
        assert!(r.summary().unknown.is_empty());
    }
}
