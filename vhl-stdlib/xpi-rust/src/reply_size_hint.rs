use vhl_stdlib::serdes::SerDesSize;
use crate::error::XpiError;

#[derive(Copy, Clone)]
pub enum ReplySizeHint {
    /// Reply can be sent right away, if it is obvious that a resource doesn't support particular
    /// operation, error is sent right away without further calls to dispatchers. Otherwise
    /// appropriate dispatcher is called, but it can also return an error, which can be bigger in
    /// size than actual Ok return type, so max(ok_return_ty, XpiError::max_size()) is used.
    Immediate {
        /// `max( Ok(&[nib; raw_size]).len_nibbles(), Err(XpiError::<max_code>).len_nibbles() )`
        max_size: SerDesSize,
        /// used to create result_nwr of correct size
        raw_size: SerDesSize,
        preliminary_result: Result<(), XpiError>,
    },
    /// Request is processed asynchronously, cannot reply right away. Appropriate task is spawned and
    /// is responsible to submit request
    Deferred,
}

impl ReplySizeHint {
    pub fn immediate(max: SerDesSize, raw: SerDesSize, preliminary: Result<(), XpiError>) -> Self {
        ReplySizeHint::Immediate {
            max_size: max,
            raw_size: raw,
            preliminary_result: preliminary,
        }
    }
}
