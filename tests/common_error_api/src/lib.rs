//! An API error enum with the common errors first (STD-16), and a method returning `CommonError` alone.

use ww_api::prelude::*;

#[ww_api_root]
trait CommonErrors {
    /// Reads the sensor; `fail` picks the error: 0 none, 1 `InUse { by: "USART2" }`, 2 `Fault`, 3 `Nack`.
    fn read(fail: u8) -> Result<u16, SensorError>;
    /// Always `Busy`.
    fn calibrate();
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone), ww_repr = unib32)]
pub enum SensorError {
    /// The sensor did not answer.
    Nack,
}

ww_api::api_error!(SensorError);
