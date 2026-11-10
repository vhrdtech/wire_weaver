//! Conversions of the API's [common](thermo_api::common) types, the ones more than one line uses unchanged: one Rust
//! type each, so one conversion serves every line that has it. The `ww_si` fields inside them are the model's types
//! already.

use crate::error::{DeviceError, Error, Result};
use crate::model;
use thermo_api::common as api;
use wire_weaver::ww_version::WireVersion;

/// Lines 2 and 3.
pub(crate) fn sample(s: api::Sample) -> Result<model::Sample> {
    Ok(model::Sample {
        seq: s.seq,
        input: s.input,
        temp: s.temp,
    })
}

/// Lines 2 and 3.
pub(crate) fn mode(m: api::Mode) -> Result<model::Mode> {
    Ok(match m {
        api::Mode::Idle => model::Mode::Idle,
        api::Mode::Continuous => model::Mode::Continuous,
        api::Mode::Unknown(code) => return Err(Error::UnknownVariant { what: "Mode", code }),
    })
}

/// Lines 2 and 3, which have no `Mode::Triggered`: they trigger in any mode.
pub(crate) fn to_mode(device: WireVersion, m: model::Mode) -> Result<api::Mode> {
    match m {
        model::Mode::Idle => Ok(api::Mode::Idle),
        model::Mode::Continuous => Ok(api::Mode::Continuous),
        model::Mode::Triggered => Err(Error::Unsupported {
            what: "Mode::Triggered",
            device,
            needs: "line 1 (lines 2 and 3 trigger in any mode)".into(),
        }),
    }
}

/// Every line.
pub(crate) fn command(c: model::Command) -> Result<api::Command> {
    Ok(match c {
        model::Command::Trigger => api::Command::Trigger,
        model::Command::Mark { seq } => api::Command::Mark { seq },
    })
}

/// Every line.
impl From<DeviceError<api::RateError>> for Error {
    fn from(e: DeviceError<api::RateError>) -> Self {
        match e.0 {
            api::RateError::Busy => Error::Busy,
            api::RateError::Unknown(code) => Error::UnknownVariant {
                what: "RateError",
                code,
            },
        }
    }
}

/// Lines 2 and 3.
impl From<DeviceError<api::SelfTestError>> for Error {
    fn from(e: DeviceError<api::SelfTestError>) -> Self {
        Error::SelfTest(match e.0 {
            api::SelfTestError::SensorOpen => model::SelfTestError::SensorOpen,
            api::SelfTestError::SensorShorted => model::SelfTestError::SensorShorted,
            api::SelfTestError::Unknown(code) => {
                return Error::UnknownVariant {
                    what: "SelfTestError",
                    code,
                };
            }
        })
    }
}
