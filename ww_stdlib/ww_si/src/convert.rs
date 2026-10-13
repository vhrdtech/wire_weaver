use super::*;

/// The division sign "/".
pub struct Per;

/// Convert quantities to f32 before calculating final quantity.
/// E.g., (distance, Per, time, ToF32).into()
#[cfg(feature = "f32")]
pub struct ToF32;

#[derive(Debug)]
pub enum Error {
    DifferentNumberTypes,
    UnsupportedType,
}

/// Only number types that are enabled as features are handled.
fn div(distance: NumericValue, time: NumericValue) -> Result<NumericValue, Error> {
    match (distance, time) {
        #[cfg(feature = "u32")]
        (NumericValue::U32(distance), NumericValue::U32(time)) => {
            Ok(NumericValue::U32(distance / time))
        }
        #[cfg(feature = "u64")]
        (NumericValue::U64(distance), NumericValue::U64(time)) => {
            Ok(NumericValue::U64(distance / time))
        }
        #[cfg(feature = "f32")]
        (NumericValue::F32(distance), NumericValue::F32(time)) => {
            Ok(NumericValue::F32(distance / time))
        }
        #[cfg(feature = "f64")]
        (NumericValue::F64(distance), NumericValue::F64(time)) => {
            Ok(NumericValue::F64(distance / time))
        }
        _ => Err(Error::UnsupportedType),
    }
}

impl TryFrom<(Meter, Per, Second)> for Speed {
    type Error = Error;

    fn try_from(quantity: (Meter, Per, Second)) -> Result<Self, Self::Error> {
        if quantity.0.value.ty() != quantity.2.value.ty() {
            return Err(Error::DifferentNumberTypes);
        }
        let value = div(quantity.0.value, quantity.2.value)?;
        let distance_prefix: i8 = quantity.0.prefix.into();
        let time_prefix: i8 = quantity.2.prefix.into();
        let prefix = distance_prefix - time_prefix;
        let prefix = prefix.try_into().expect("");
        Ok(Speed { prefix, value })
    }
}

#[cfg(feature = "f32")]
impl TryFrom<(Meter, Per, Second, ToF32)> for Speed {
    type Error = Error;

    fn try_from(quantity: (Meter, Per, Second, ToF32)) -> Result<Self, Self::Error> {
        let distance = Meter {
            prefix: quantity.0.prefix,
            value: NumericValue::F32(quantity.0.value.as_f32()),
        };
        let time = Second {
            prefix: quantity.2.prefix,
            value: NumericValue::F32(quantity.2.value.as_f32()),
        };
        (distance, Per, time).try_into()
    }
}

#[cfg(all(test, feature = "u32"))]
mod tests {
    use super::*;
    use crate as ww_si;

    #[test]
    fn convert_sanity() {
        let distance = quantity!(10 m u32);
        let time = quantity!(2 s u32);
        let speed: Speed = (distance, Per, time).try_into().unwrap();
        assert_eq!(speed.value, NumericValue::U32(5));
    }
}
