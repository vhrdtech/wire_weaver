//! Codec cases (LVL-6): a case of a codec's `Coded` type carries the linear number its code means (`decoded`) and,
//! where it isn't the case's own, the code that number encodes back to (`encodes_to`). The derive runs the typed
//! `Codec`; the dynamic client has only the codec's definition in the model and runs `CodecDef::decode` / `encode`
//! through `ww_self::units`. A change to a codec's algorithm that moves a code's meaning fails here, as a moved wire
//! byte fails the cases' golden bytes.

use crate::corpus::Case;
use crate::dynamic::{bundle, property_ty};
use ww_si::Fixed;
use ww_si::codec::{Codec, MuLaw};
use ww_si::exact_units::Rounding;

/// The code of a codec case: its golden bytes, little-endian.
fn code(case: &Case) -> Result<i128, String> {
    let bytes = case.golden();
    match bytes.as_slice() {
        [b] => Ok(i128::from(*b)),
        _ => Err(format!(
            "a code of {} bytes: only u8 codes so far",
            bytes.len()
        )),
    }
}

/// What a case expects: its code, the linear number, the code that number encodes to.
fn expected(case: &Case) -> Result<Option<(i128, i128, i128)>, String> {
    let Some(decoded) = case.decoded else {
        return Ok(None);
    };
    let code = code(case)?;
    Ok(Some((
        code,
        i128::from(decoded),
        case.encodes_to.map_or(code, i128::from),
    )))
}

fn check(got: (i128, i128), want: (i128, i128)) -> Result<(), String> {
    match got == want {
        true => Ok(()),
        false => Err(format!(
            "decodes to {} and encodes back to {}, expected {} and {}",
            got.0, got.1, want.0, want.1
        )),
    }
}

/// The typed codec of the case's property.
pub fn derive(case: &Case) -> Result<(), String> {
    let Some((code, decoded, encodes_to)) = expected(case)? else {
        return Ok(());
    };
    match case.ty.as_str() {
        "coded_mu_law" => {
            let raw = u8::try_from(code).map_err(|_| format!("{code} is no u8"))?;
            let linear = MuLaw::decode(raw).get();
            let value = i16::try_from(decoded).map_err(|_| format!("{decoded} is no i16"))?;
            let back = MuLaw::encode(Fixed::new(value), Rounding::NearestEven)
                .ok_or_else(|| format!("{decoded} doesn't encode"))?;
            check(
                (i128::from(linear), i128::from(back)),
                (decoded, encodes_to),
            )
        }
        other => Err(format!("no typed codec for property '{other}'")),
    }
}

/// The codec's definition in the API model, run without the trait.
pub fn dynamic(case: &Case) -> Result<(), String> {
    let Some((code, decoded, encodes_to)) = expected(case)? else {
        return Ok(());
    };
    let ty = property_ty(&case.ty).ok_or_else(|| format!("no property '{}'", case.ty))?;
    let Some(ww_self::units::Wrapper::Coded { codec, .. }) =
        ww_self::units::wrapper_of(ty, bundle())
    else {
        return Err(format!("'{}' is no Coded in the model", case.ty));
    };
    let def = ww_self::units::codec_of(codec, bundle()).map_err(|e| format!("{e:#}"))?;
    let linear = ww_self::units::codec_decode(&def, code).map_err(|e| format!("{e:#}"))?;
    let back = ww_self::units::codec_encode(&def, decoded, Rounding::NearestEven)
        .map_err(|e| format!("{e:#}"))?;
    check((linear, back), (decoded, encodes_to))
}
