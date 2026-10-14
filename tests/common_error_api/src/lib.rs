//! An API error enum with the common errors first (STD-16), and a method returning `CommonError` alone.

use wire_weaver::prelude::*;

#[ww_trait]
trait CommonErrors {
    /// Reads the sensor; `fail` picks the error: 0 none, 1 `InUse { by: "USART2" }`, 2 `Fault`, 3 `Nack`.
    fn read(fail: u8) -> Result<u16, SensorError<'i>>;
    /// Always `Busy`.
    fn calibrate() -> Result<(), CommonError<'i>>;
}

#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone), ww_repr = unib32)]
pub enum SensorError<'i> {
    #[common]
    Common(CommonError<'i>),
    /// The sensor did not answer.
    Nack,
}
