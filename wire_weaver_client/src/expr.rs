//! Values and calls written as Rust expressions (TOOL-9), [ww_self::expr] with `ww_si` quantities: `q!(3300 mV)`,
//! `q!(3.3 V)`, `q!(1.2e-4 N*m/(rad/s))`, `q!("20 °C")` (a string for symbols Rust can't read), or a plain number in
//! the unit itself, wherever the type is a `Quantity<U>` or an `AnyQuantity`.

use crate::quantity::{DynQuantity, is_any_quantity, parse_quantity, unit_of};
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
    let unit = unit_of(ty, bundle);
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
        parse_quantity(&text, unit.as_ref())
            .and_then(|q: DynQuantity| q.to_value(ty, bundle))
            .map_err(|e| anyhow!(e)),
    )
}

fn quote_text(expr: &Expr) -> String {
    use ww_self::expr::quote::ToTokens;
    expr.to_token_stream().to_string()
}
