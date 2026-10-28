//! Values and calls written as Rust expressions (TOOL-9), [ww_self::expr] with `ww_si` quantities: `q!(3300 mV)`,
//! `q!(3.3 V)`, `q!(1.2e-4 N*m/(rad/s))`, `q!("20 °C")` (a string for symbols Rust can't read), or a plain number in
//! the unit itself, wherever the type is a `Quantity<U>` or an `AnyQuantity`. Into an `exact_units::Fixed` (CLT-54)
//! `q!(5 ms)` is converted exactly into its scale and storage, a plain number is the stored number (`5000` into a
//! `Fixed<Second, Micro, u32>` is 5 ms); an `Index`, `IndexFrom1` or `Seq` is written as its plain number.

use crate::quantity::{
    DynQuantity, fixed_of, is_any_quantity, parse_quantity_in, stored_number, unit_of,
};
use anyhow::{Result, anyhow};
use ww_self::expr::syn::{self, Expr, Lit};
use ww_self::expr::{self, Hook};
pub use ww_self::expr::{Call, parse_call};
use ww_self::{ApiBundleOwned, TypeOwned, ValueOwned};

/// Parse `text`, a Rust expression, as a value of type `ty`, quantities included.
pub fn parse_value(text: &str, ty: &TypeOwned, bundle: &ApiBundleOwned) -> Result<ValueOwned> {
    expr::parse_value_with(text, ty, bundle, QUANTITIES)
}

/// A value of type `ty` from a parsed expression, quantities included.
pub fn value_from_expr(expr: &Expr, ty: &TypeOwned, bundle: &ApiBundleOwned) -> Result<ValueOwned> {
    expr::value_from_expr_with(expr, ty, bundle, QUANTITIES)
}

const QUANTITIES: Hook = &quantity;

/// A `ww_si` quantity from `q!(..)` or a plain number, `None` for anything else (a struct literal of the quantity
/// still works the usual way).
fn quantity(expr: &Expr, ty: &TypeOwned, bundle: &ApiBundleOwned) -> Option<Result<ValueOwned>> {
    // an exact_units wrapper: a plain number is what it stores
    if let Expr::Lit(_) | Expr::Unary(_) = expr
        && let Some(value) = stored_number(&quote_text(expr), ty, bundle)
    {
        return Some(value.map_err(|e| anyhow!(e)));
    }
    let fixed = fixed_of(ty, bundle);
    let unit = unit_of(ty, bundle).or_else(|| fixed.as_ref().map(|f| f.unit.clone()));
    if unit.is_none() && !is_any_quantity(ty, bundle) {
        return None;
    }
    let text = match expr {
        Expr::Macro(m) if m.mac.path.is_ident("q") => {
            match syn::parse2::<Lit>(m.mac.tokens.clone()) {
                Ok(Lit::Str(s)) => s.value(),
                _ => m.mac.tokens.to_string(),
            }
        }
        // a number alone is in the unit itself
        Expr::Lit(_) | Expr::Unary(_) if unit.is_some() => quote_text(expr),
        _ => return None,
    };
    Some(
        parse_quantity_in(&text, unit.as_ref(), fixed.as_ref().map(|f| &f.prefix))
            .and_then(|q: DynQuantity| q.to_value(ty, bundle))
            .map_err(|e| anyhow!(e)),
    )
}

fn quote_text(expr: &Expr) -> String {
    use ww_self::expr::quote::ToTokens;
    expr.to_token_stream().to_string()
}
