use vhl_stdlib::serdes::{SerDesSize, SerializableError, SerializeVlu4};
use vhl_stdlib::serdes::vlu4::Vlu32;
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
    pub fn preliminary_ok(max: SerDesSize, raw: SerDesSize) -> Self {
        ReplySizeHint::Immediate {
            max_size: max,
            raw_size: raw,
            preliminary_result: Ok(()),
        }
    }

    pub fn immediate_error_xwfs(err: XpiError) -> Self {
        ReplySizeHint::Immediate {
            max_size: Vlu32(XpiError::max_code()).len_nibbles(),
            raw_size: SerDesSize::Sized(0),
            preliminary_result: Err(err),
        }
    }
}
