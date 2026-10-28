//! Requantize cases (STD-63): a stored number converted to another scale and storage with a named rounding and
//! overflow policy, the same result from every consumer. The derive runs the typed conversion
//! (`Fixed::requantize::<P2, T2>`) and checks the case's factor is its scales' quotient; the dynamic client has only
//! what the model gives it (the factor as a `Magnitude` on the wire, the target storage's name) and runs
//! `exact_units::Requantizer`.

use crate::corpus::RequantizeCase;
use crate::dynamic::{bundle, property_ty};
use serde_json::Value as J;
use wire_weaver::shrink_wrap::{DeserializeShrinkWrapOwned, SerializeShrinkWrapOwned};
use ww_si::Fixed;
use ww_si::exact_units::prefix::{Deci, Micro, Milli, One, Pow2, Rational};
use ww_si::exact_units::units::{Radian, Turn, Volt};
use ww_si::exact_units::{
    FixedScale, Magnitude, Overflow, Repr, Requantizer, Rounding, Storage, StorageKind, Unit,
};

ww_si::exact_units::scale! {
    /// Steps of 1/1000 °, over the unit radian: π/180000, not a fraction of integers.
    struct MilliDegree = "m°", Exact(1 / 180000, pi 1);
}

/// A typed conversion: its factor and the value converted by the derive's `Fixed`.
pub struct Conversion {
    pub id: &'static str,
    pub factor: Magnitude,
    pub to: &'static str,
    pub run: fn(Repr, Rounding, Overflow) -> Result<Option<Repr>, String>,
}

fn typed<U: Unit, P: FixedScale, T: Storage, P2: FixedScale, T2: Storage>(
    x: Repr,
    r: Rounding,
    o: Overflow,
) -> Result<Option<Repr>, String> {
    let v = match x {
        Repr::Int(i) => T::from_int(i),
        Repr::Float(f) => T::from_float(f),
    }
    .ok_or_else(|| format!("{x:?} isn't a {}", T::NAME))?;
    Ok(Fixed::<U, P, T>::new(v)
        .requantize::<P2, T2>(r, o)
        .map(|f| f.get().to_repr()))
}

macro_rules! conversions {
    ($($id:literal: $u:ty, $p:ty, $t:ty => $p2:ty, $t2:ty;)*) => {
        &[$(Conversion {
            id: $id,
            factor: <$p as FixedScale>::SCALE.div(<$p2 as FixedScale>::SCALE),
            to: <$t2 as Storage>::NAME,
            run: typed::<$u, $p, $t, $p2, $t2>,
        },)*]
    };
}

/// Every conversion the corpus names, by id.
pub const CONVERSIONS: &[Conversion] = conversions! {
    "mV i16 -> dV i8": Volt, Milli, i16 => Deci, i8;
    "uV i32 -> mV u8": Volt, Micro, i32 => Milli, u8;
    "Q15 -> Q7": Volt, Pow2<-15>, i16 => Pow2<-8>, i8;
    "7/3 i16 -> i16": Volt, Rational<7, 3>, i16 => One, i16;
    "mV u16 -> uV u16": Volt, Milli, u16 => Micro, u16;
    "BAM16 -> BAM8": Turn, Pow2<-16>, u16 => Pow2<-8>, u8;
    "f32 -> i16": Volt, One, f32 => One, i16;
    "mV i32 -> V f64": Volt, Milli, i32 => One, f64;
    "m° i32 -> urad i32": Radian, MilliDegree, i32 => Micro, i32;
    "m° i64 -> urad i64": Radian, MilliDegree, i64 => Micro, i64;
};

pub fn conversion(id: &str) -> Option<&'static Conversion> {
    CONVERSIONS.iter().find(|c| c.id == id)
}

pub fn rounding(name: &str) -> Result<Rounding, String> {
    Ok(match name {
        "truncate" => Rounding::Truncate,
        "floor" => Rounding::Floor,
        "ceil" => Rounding::Ceil,
        "nearest_even" => Rounding::NearestEven,
        "nearest_away" => Rounding::NearestAway,
        _ => return Err(format!("no rounding '{name}'")),
    })
}

pub fn overflow(name: &str) -> Result<Overflow, String> {
    Ok(match name {
        "checked" => Overflow::Checked,
        "saturate" => Overflow::Saturate,
        "wrap" => Overflow::Wrap,
        _ => return Err(format!("no overflow policy '{name}'")),
    })
}

/// A storage by the name the model gives it.
pub fn storage(name: &str) -> Result<StorageKind, String> {
    let int = |bits, signed| StorageKind::Int { bits, signed };
    Ok(match name {
        "u8" => int(8, false),
        "u16" => int(16, false),
        "u32" => int(32, false),
        "u64" => int(64, false),
        "i8" => int(8, true),
        "i16" => int(16, true),
        "i32" => int(32, true),
        "i64" => int(64, true),
        "f32" => StorageKind::F32,
        "f64" => StorageKind::F64,
        _ => return Err(format!("no storage '{name}'")),
    })
}

/// A corpus number: an integer (a string beyond 64 bits) or a float.
pub fn repr(j: &J) -> Result<Repr, String> {
    if j.is_f64() {
        return Ok(Repr::Float(j.as_f64().unwrap()));
    }
    crate::json::int(j)
        .map(Repr::Int)
        .ok_or_else(|| format!("{j} is not a number"))
}

fn expected(case: &RequantizeCase) -> Result<Option<Repr>, String> {
    if case.result.is_null() {
        Ok(None)
    } else {
        repr(&case.result).map(Some)
    }
}

pub(crate) fn compare(got: Option<Repr>, case: &RequantizeCase) -> Result<(), String> {
    let want = expected(case)?;
    if got == want {
        Ok(())
    } else {
        Err(format!("gave {got:?}, expected {want:?}"))
    }
}

/// The case's factor as the dynamic client has it: the corpus form through the model to bytes, and those bytes read
/// as a `Magnitude`.
pub fn factor(case: &RequantizeCase) -> Result<Magnitude, String> {
    let ty = property_ty("magnitude").ok_or("no magnitude property in conformance_api")?;
    let value =
        crate::json::to_value(&case.factor, ty, bundle()).map_err(|e| format!("factor: {e}"))?;
    let bytes = value
        .ser_shrink_wrap_dyn(ty, bundle())
        .map_err(|e| format!("factor: {e:#}"))?;
    Magnitude::from_ww_bytes_owned(&bytes).map_err(|e| format!("factor: {e:?}"))
}

/// The typed conversion of the derive.
pub fn derive(case: &RequantizeCase) -> Result<(), String> {
    let c = conversion(&case.conversion)
        .ok_or_else(|| format!("no conversion '{}' in requantize.rs", case.conversion))?;
    if c.to != case.to {
        return Err(format!(
            "the conversion is to {}, the case says {}",
            c.to, case.to
        ));
    }
    let got = factor(case)?;
    if got != c.factor {
        let bytes = c.factor.to_ww_bytes_owned().map_err(|e| format!("{e:?}"))?;
        return Err(format!(
            "the factor is {got}, the conversion's is {} ({})",
            c.factor,
            hex::encode(bytes)
        ));
    }
    let x = repr(&case.value)?;
    compare(
        (c.run)(x, rounding(&case.rounding)?, overflow(&case.overflow)?)?,
        case,
    )
}

/// The run-time conversion of a dynamic client.
pub fn dynamic(case: &RequantizeCase) -> Result<(), String> {
    let q = Requantizer::new(
        &factor(case)?,
        storage(&case.to)?,
        rounding(&case.rounding)?,
        overflow(&case.overflow)?,
    );
    compare(q.apply(repr(&case.value)?), case)
}
