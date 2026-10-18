//! `ww explain`'s two halves as a consumer (TOOL-9): the case's value written as a Rust expression
//! ([ww_self::expr::to_expr]) and read back ([ww_self::expr::parse_value]) before it is encoded, and the golden bytes
//! decoded by [ww_self::explain], whose map of the bits must hold up: no bit in two parts, nothing left over but
//! padding (less than a byte at a time).

use crate::corpus::{Case, Direction};
use crate::dynamic::{bundle, property_ty};
use ww_self::explain::{Explained, explain};
use ww_self::expr::{parse_value, to_expr};
use ww_self::{TypeOwned, ValueOwned};

pub struct Explain;

impl Explain {
    fn value(case: &Case, ty: &TypeOwned) -> Result<ValueOwned, String> {
        crate::json::to_value(&case.value, ty, bundle()).map_err(|e| format!("corpus value: {e}"))
    }
}

impl crate::Consumer for Explain {
    const NAME: &'static str = "explain";

    fn encode(&self, case: &Case) -> Option<Result<Vec<u8>, String>> {
        let ty = property_ty(&case.ty)?;
        Some((|| {
            let value = Self::value(case, ty)?;
            let text = to_expr(&value, ty, bundle()).map_err(|e| format!("as Rust: {e:#}"))?;
            let read = parse_value(&text, ty, bundle())
                .map_err(|e| format!("`{text}` read back: {e:#}"))?;
            read.ser_shrink_wrap_dyn(ty, bundle())
                .map_err(|e| format!("`{text}`: {e:#}"))
        })())
    }

    fn decode(&self, case: &Case) -> Option<crate::DecodeResult> {
        let ty = property_ty(&case.ty)?;
        let golden = case.golden();
        let explained = explain(&golden, ty, bundle());
        if explained.result.is_ok()
            && let Err(e) = bit_map(&explained)
        {
            return Some(Ok(Err(e)));
        }
        let got = match explained.result {
            Ok(mut values) if values.len() == 1 => values.remove(0),
            Ok(values) => return Some(Ok(Err(format!("{} values", values.len())))),
            Err(e) => return Some(Err(format!("{e:#}"))),
        };
        if case.value.is_null() && case.direction == Direction::Reject {
            return Some(Ok(Err(format!("decoded {got:?}"))));
        }
        Some(Ok(match Self::value(case, ty) {
            Ok(want) if want == got => Ok(()),
            Ok(want) => Err(format!("decoded {got:?}, expected {want:?}")),
            Err(e) => Err(e),
        }))
    }
}

/// Every bit in exactly one part with no parts inside it (padding and skipped bytes are parts too).
fn bit_map(explained: &Explained) -> Result<(), String> {
    let parts = &explained.parts;
    let mut owner: Vec<Option<usize>> = vec![None; explained.len * 8];
    for (i, part) in parts.iter().enumerate() {
        let leaf = parts.get(i + 1).is_none_or(|next| next.depth <= part.depth);
        if !leaf {
            continue;
        }
        for bit in part
            .front
            .clone()
            .chain(part.back.start * 4..part.back.end * 4)
        {
            let slot = owner
                .get_mut(bit)
                .ok_or_else(|| format!("{part:?} reaches past the end, bit {bit}"))?;
            if let Some(other) = slot {
                return Err(format!("bit {bit} in {:?} and {part:?}", parts[*other]));
            }
            *slot = Some(i);
        }
    }
    match owner.iter().position(Option::is_none) {
        Some(bit) => Err(format!("bit {bit} is in no part")),
        None => Ok(()),
    }
}
