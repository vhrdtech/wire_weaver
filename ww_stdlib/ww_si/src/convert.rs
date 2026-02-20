use super::*;

/// The division sign "/".
pub struct Per;

/// Convert quantities to f32 before calculating final quantity.
/// E.g., (distance, Per, time, ToF32).into()
pub struct ToF32;

#[derive(Debug)]
pub enum Error {
    DifferentNumberTypes,
    UnsupportedType,
}

impl TryFrom<(Meter, Per, Second)> for Speed {
    type Error = Error;

    fn try_from(quantity: (Meter, Per, Second)) -> Result<Self, Self::Error> {
        if quantity.0.value.ty() != quantity.2.value.ty() {
            return Err(Error::DifferentNumberTypes);
        }
        let value = match (quantity.0.value, quantity.2.value) {
            (NumericValue::U32(distance), NumericValue::U32(time)) => {
                NumericValue::U32(distance / time)
            }
            (NumericValue::U64(distance), NumericValue::U64(time)) => {
                NumericValue::U64(distance / time)
            }
            (NumericValue::F32(distance), NumericValue::F32(time)) => {
                NumericValue::F32(distance / time)
            }
            (NumericValue::F64(distance), NumericValue::F64(time)) => {
                NumericValue::F64(distance / time)
            }
            _ => return Err(Error::UnsupportedType),
        };
        let distance_prefix: i8 = quantity.0.prefix.into();
        let time_prefix: i8 = quantity.2.prefix.into();
        let prefix = distance_prefix - time_prefix;
        let prefix = prefix.try_into().expect("");
        Ok(Speed {
            prefix,
            value,
        })
    }
}

impl TryFrom<(Meter, Per, Second, ToF32)> for Speed {
    type Error = Error;

    fn try_from(quantity: (Meter, Per, Second, ToF32)) -> Result<Self, Self::Error> {
        let distance = Meter { prefix: quantity.0.prefix, value: NumericValue::F32(quantity.0.value.as_f32()) };
        let time = Second { prefix: quantity.2.prefix, value: NumericValue::F32(quantity.2.value.as_f32()) };
        (distance, Per, time).try_into()
    }
}

#[cfg(test)]
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
