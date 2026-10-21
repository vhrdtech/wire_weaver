//! Typed attributes beyond the wire contract (SRV-34): `#[claim(..)]`, `#[shares(..)]` and `#[label(..)]`, read
//! into [MetaOwned] of an item or a trait definition. See `docs/api/metadata.md`.

use super::crate_walker::CrateContext;
use anyhow::{Result, anyhow};
use proc_macro2::Ident;
use shrink_wrap::UNib32;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Attribute, LitInt, LitStr, Meta, Token};
use ww_self::{ClaimMode, MetaOwned, SharesOwned};

/// Most pins one range (`PA0-PA15`) expands to, against a typo making thousands.
const MAX_PIN_RANGE: u32 = 256;

/// Where the attributes are written, which decides what is allowed.
pub(crate) enum MetaSite<'a> {
    /// A trait definition: `#[claim]` only, for every `ww_impl!` of it.
    Trait,
    /// A `ww_impl!`, `array` for `ww_impl!(name[]: ..)`.
    Impl { array: bool },
    /// A method, property, stream or const named `name`, `array` for a resource array.
    Item { name: &'a str, array: bool },
}

/// The attributes of an item or trait, `None` without any of them.
pub(crate) fn get_meta(
    attrs: &[Attribute],
    site: MetaSite,
    current_crate: &CrateContext,
) -> Result<Option<MetaOwned>> {
    get_meta_inner(attrs, &site).map_err(|e| e.context(current_crate.err_context()))
}

fn get_meta_inner(attrs: &[Attribute], site: &MetaSite) -> Result<Option<MetaOwned>> {
    let mut meta = MetaOwned::default();
    let mut found = false;
    let mut label_seen = false;
    for attr in attrs {
        let kind = if attr.path().is_ident("claim") {
            "claim"
        } else if attr.path().is_ident("shares") {
            "shares"
        } else if attr.path().is_ident("label") {
            "label"
        } else {
            continue;
        };
        found = true;
        match (kind, site) {
            ("claim" | "shares", MetaSite::Item { name, .. }) => {
                return Err(anyhow!(
                    "#[{kind}] on `{name}`: only a ww_impl! (or a trait definition, for #[claim]) is a resource, put \
                     it on the ww_impl! this item belongs to"
                ));
            }
            ("shares", MetaSite::Trait) => {
                return Err(anyhow!(
                    "#[shares] on a trait definition: pins and peripherals belong to one board's resource, put it on \
                     the ww_impl!"
                ));
            }
            ("label", MetaSite::Trait) => {
                return Err(anyhow!(
                    "#[label] on a trait definition: a label names one resource of a board, put it on the ww_impl!"
                ));
            }
            _ => {}
        }
        match kind {
            "claim" => {
                if meta.claim.is_some() {
                    return Err(anyhow!("more than one #[claim]"));
                }
                meta.claim = Some(claim_mode(attr)?);
            }
            "shares" => {
                let array = matches!(site, MetaSite::Impl { array: true });
                meta.shares.push(shares(attr, array)?);
            }
            _ => {
                if label_seen {
                    return Err(anyhow!(
                        "more than one #[label]: one attribute, with one label per element of a resource array"
                    ));
                }
                label_seen = true;
                let array = match site {
                    MetaSite::Impl { array } | MetaSite::Item { array, .. } => *array,
                    MetaSite::Trait => false,
                };
                meta.labels = labels(attr, array)?;
            }
        }
    }
    Ok(found.then_some(meta))
}

/// `#[claim]` (exclusive) or `#[claim(exclusive | shared | observe)]`.
fn claim_mode(attr: &Attribute) -> Result<ClaimMode> {
    const EXPECTED: &str =
        "expected #[claim], #[claim(exclusive)], #[claim(shared)] or #[claim(observe)]";
    match &attr.meta {
        Meta::Path(_) => Ok(ClaimMode::Exclusive),
        Meta::List(list) => {
            let mode: Ident = list.parse_args().map_err(|e| anyhow!("{EXPECTED}: {e}"))?;
            match mode.to_string().as_str() {
                "exclusive" => Ok(ClaimMode::Exclusive),
                "shared" => Ok(ClaimMode::Shared),
                "observe" => Ok(ClaimMode::Observe),
                other => Err(anyhow!("unknown claim mode `{other}`, {EXPECTED}")),
            }
        }
        Meta::NameValue(_) => Err(anyhow!(EXPECTED)),
    }
}

/// One `key = value` of `#[shares(..)]`.
struct SharesArg {
    key: Ident,
    value: SharesValue,
}

enum SharesValue {
    Str(LitStr),
    Int(LitInt),
}

impl Parse for SharesArg {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let key = input.parse()?;
        let _: Token![=] = input.parse()?;
        let value = if input.peek(LitInt) {
            SharesValue::Int(input.parse()?)
        } else {
            SharesValue::Str(input.parse()?)
        };
        Ok(SharesArg { key, value })
    }
}

/// `#[shares(pins = "PA9, PA10", peripherals = "USART1", index = 0)]`.
fn shares(attr: &Attribute, array: bool) -> Result<SharesOwned> {
    const EXPECTED: &str = "expected #[shares(pins = \"PA9, PA10\", peripherals = \"USART1\")], index = N on a resource array";
    let Meta::List(list) = &attr.meta else {
        return Err(anyhow!(EXPECTED));
    };
    let args = list
        .parse_args_with(Punctuated::<SharesArg, Token![,]>::parse_terminated)
        .map_err(|e| anyhow!("{EXPECTED}: {e}"))?;
    let mut shares = SharesOwned {
        index: None,
        pins: vec![],
        peripherals: vec![],
    };
    for arg in args {
        match (arg.key.to_string().as_str(), arg.value) {
            ("pins", SharesValue::Str(s)) if shares.pins.is_empty() => {
                shares.pins = expand_pins(&s.value())?;
            }
            ("peripherals", SharesValue::Str(s)) if shares.peripherals.is_empty() => {
                shares.peripherals = list_of(&s.value());
            }
            ("index", SharesValue::Int(i)) if shares.index.is_none() => {
                if !array {
                    return Err(anyhow!(
                        "#[shares(index = ..)] on a ww_impl! that isn't a resource array"
                    ));
                }
                shares.index = Some(UNib32(i.base10_parse()?));
            }
            (key, _) => {
                return Err(anyhow!("unexpected or repeated `{key}`, {EXPECTED}"));
            }
        }
    }
    if shares.pins.is_empty() && shares.peripherals.is_empty() {
        return Err(anyhow!(
            "#[shares] names no pins and no peripherals, {EXPECTED}"
        ));
    }
    Ok(shares)
}

/// Comma separated names, trimmed, empty ones dropped.
fn list_of(s: &str) -> Vec<String> {
    s.split(',')
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(String::from)
        .collect()
}

/// Comma separated pin names, a range `PA0-PA15` (same prefix, a number at the end of both) as its pins.
fn expand_pins(s: &str) -> Result<Vec<String>> {
    let mut pins = vec![];
    for name in list_of(s) {
        let range = name.split_once('-').and_then(|(first, last)| {
            let (prefix, first) = split_number(first.trim())?;
            let (last_prefix, last) = split_number(last.trim())?;
            (prefix == last_prefix).then_some((prefix.to_string(), first, last))
        });
        match range {
            Some((prefix, first, last)) => {
                if first > last || last - first >= MAX_PIN_RANGE {
                    return Err(anyhow!(
                        "pin range `{name}` must go up, at most {MAX_PIN_RANGE} pins"
                    ));
                }
                pins.extend((first..=last).map(|n| format!("{prefix}{n}")));
            }
            None => pins.push(name),
        }
    }
    Ok(pins)
}

/// `PA15` as `("PA", 15)`, `None` without a number at the end.
fn split_number(s: &str) -> Option<(&str, u32)> {
    let digits = s.len() - s.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    if digits == 0 || digits == s.len() {
        return None;
    }
    let (prefix, number) = s.split_at(s.len() - digits);
    Some((prefix, number.parse().ok()?))
}

/// `#[label("CAN1, DB-9 on the front")]`, or one label per element of a resource array.
fn labels(attr: &Attribute, array: bool) -> Result<Vec<String>> {
    const EXPECTED: &str =
        "expected #[label(\"what a person calls it\")], one per element on a resource array";
    let Meta::List(list) = &attr.meta else {
        return Err(anyhow!(EXPECTED));
    };
    let labels = list
        .parse_args_with(Punctuated::<LitStr, Token![,]>::parse_terminated)
        .map_err(|e| anyhow!("{EXPECTED}: {e}"))?;
    let labels: Vec<String> = labels.iter().map(LitStr::value).collect();
    if labels.is_empty() || labels.iter().any(|l| l.trim().is_empty()) {
        return Err(anyhow!("an empty label, {EXPECTED}"));
    }
    if labels.len() > 1 && !array {
        return Err(anyhow!(
            "{} labels on an item that isn't a resource array, {EXPECTED}",
            labels.len()
        ));
    }
    Ok(labels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_ranges_expand() {
        assert_eq!(
            expand_pins("PA9, PA10").unwrap(),
            ["PA9", "PA10"].map(String::from)
        );
        assert_eq!(
            expand_pins("PB0-PB2, 5V").unwrap(),
            ["PB0", "PB1", "PB2", "5V"].map(String::from)
        );
        // not a range: different prefixes, or no number
        assert_eq!(
            expand_pins("PA1-PB2, VDD-IO").unwrap(),
            ["PA1-PB2", "VDD-IO"].map(String::from)
        );
        assert!(expand_pins("PA5-PA1").is_err());
        assert!(expand_pins("P0-P1000").is_err());
    }
}
