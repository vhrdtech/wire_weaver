use ww_api::prelude::*;
use ww_si::Index;
use ww_si::units::Step;

use crate::BootState;

/// Why a request was refused.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    self_describing,
    ww_repr = unib32
)]
pub enum Error {
    /// This node doesn't do that (a target it doesn't accept, revert without a previous image).
    Unsupported,
    /// Not in this boot state; the state is given.
    WrongState(BootState),
    /// Another session is in progress; `Stage::abort` it first.
    Busy,
    /// No session in progress.
    NoSession,
    /// The plan header is malformed (magic, format, reserved bytes).
    BadHeader,
    /// The plan is for another flash geometry.
    WrongGeometry,
    /// The image's security counter is below the node's [NodeInfo::security_floor].
    BelowFloor,
    /// The release signature over the descriptor doesn't verify.
    BadSignature,
    /// A step, offset or chunk points outside the slot. Nothing was written.
    OutOfRange,
    /// A step arrived ahead of its turn; the node expects this step index next.
    OutOfOrder(Index<Step, u32>),
    /// A different step arrived under an index the node already has.
    StepMismatch,
    /// A piece is longer than [StageLimits::max_piece].
    PieceTooBig,
    /// The plan doesn't fit the plan area ([Target::Stored]).
    PlanTooBig,
    /// A COPY source doesn't have the hash its step says. Nothing was written.
    SourceMismatch,
    /// A written chunk doesn't have the hash its step says.
    HashMismatch,
    /// `Stage::finish` before all steps are in.
    Incomplete,
    /// The finished image doesn't match its descriptor.
    VerifyFailed,
    /// The flash reported an error.
    Flash,
    /// A read faulted on ECC (a cut write or erase); the chunk needs an erase.
    Ecc,
    /// An implementation-specific error: the vendor's own code, no unit.
    Custom(u32),
}

ww_api::api_error!(Error);
