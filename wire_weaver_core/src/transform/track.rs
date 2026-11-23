//! `#[track(..)]` (API-71): how a recorder lays a stream out on a tape. Read by [get_meta](super::meta::get_meta)
//! into `Meta::track`, then settled per trait by [settle]: a stream without its own takes the trait's, an item that is
//! no stream is refused, and a series stream's item type must be numbers. See `docs/api/metadata.md`.

use anyhow::{Result, anyhow};
use proc_macro2::Ident;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Attribute, Meta, Token, parenthesized};
use ww_self::{ApiBundleOwned, ApiItemKindOwned, ApiItemOwned, MetaOwned, TrackLayout, TrackOwned};

/// Most dimension names one `dims(..)` lists, against a list nobody can fill in.
pub(crate) const TRACK_DIMS_MAX: usize = 16;

/// One argument of `#[track(..)]`: a word (`series`, `flags`) or `dims(..)`.
enum TrackArg {
    Word(Ident),
    Dims(Ident, Punctuated<Ident, Token![,]>),
}

impl Parse for TrackArg {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let word: Ident = input.parse()?;
        if input.peek(syn::token::Paren) {
            let content;
            parenthesized!(content in input);
            return Ok(TrackArg::Dims(
                word,
                content.parse_terminated(Ident::parse, Token![,])?,
            ));
        }
        Ok(TrackArg::Word(word))
    }
}

/// `#[track(spans, dims(input, producer), flags)]`: the layout first, then `dims(..)` and `flags`, each at most once.
pub(crate) fn parse(attr: &Attribute) -> Result<TrackOwned> {
    let layouts = TrackLayout::NAMED.map(|(_, n)| n).join(" | ");
    let expected = format!("expected #[track({layouts}, dims(..), flags)], the layout first");
    let Meta::List(list) = &attr.meta else {
        return Err(anyhow!("{expected}"));
    };
    let args = list
        .parse_args_with(Punctuated::<TrackArg, Token![,]>::parse_terminated)
        .map_err(|e| anyhow!("{expected}: {e}"))?;
    let mut args = args.into_iter();
    let layout = match args.next() {
        Some(TrackArg::Word(word)) => TrackLayout::from_name(&word.to_string())
            .ok_or_else(|| anyhow!("unknown track layout `{word}`, {expected}"))?,
        _ => return Err(anyhow!("{expected}")),
    };
    let mut track = TrackOwned {
        layout,
        dims: vec![],
        flags: false,
    };
    let mut dims_seen = false;
    for arg in args {
        match arg {
            TrackArg::Word(word) if word == "flags" => {
                if track.flags {
                    return Err(anyhow!("`flags` named twice in #[track]"));
                }
                track.flags = true;
            }
            TrackArg::Dims(word, names) if word == "dims" => {
                if dims_seen {
                    return Err(anyhow!(
                        "more than one `dims(..)` in #[track]: list every name in one"
                    ));
                }
                dims_seen = true;
                track.dims = dims(&names)?;
            }
            TrackArg::Word(word) | TrackArg::Dims(word, _) => {
                return Err(anyhow!("unknown `{word}` in #[track], {expected}"));
            }
        }
    }
    if track.flags && layout == TrackLayout::Series {
        return Err(anyhow!(
            "#[track(series, flags)]: a series has no flags word per sample, only points and spans have one"
        ));
    }
    Ok(track)
}

/// The names of `dims(..)`: at least one, each once, at most [TRACK_DIMS_MAX].
fn dims(names: &Punctuated<Ident, Token![,]>) -> Result<Vec<String>> {
    if names.is_empty() {
        return Err(anyhow!(
            "`dims()` names no dimension: list the names a track of the stream may carry, or leave it out"
        ));
    }
    if names.len() > TRACK_DIMS_MAX {
        return Err(anyhow!(
            "`dims(..)` names {} dimensions, at most {TRACK_DIMS_MAX}",
            names.len()
        ));
    }
    let mut dims: Vec<String> = Vec::with_capacity(names.len());
    for name in names {
        let name = name.to_string();
        let name = name.strip_prefix("r#").unwrap_or(&name).to_owned();
        if dims.contains(&name) {
            return Err(anyhow!("dimension `{name}` named twice in `dims(..)`"));
        }
        dims.push(name);
    }
    Ok(dims)
}

/// The item type of a stream, `None` for an item that is no stream a recorder records (a sink, a method, ...).
fn stream_item(kind: &ApiItemKindOwned) -> Option<&ww_self::TypeOwned> {
    match kind {
        ApiItemKindOwned::Stream { ty, is_up: true } | ApiItemKindOwned::StreamFn { ty, .. } => {
            Some(ty)
        }
        _ => None,
    }
}

/// Gives each stream of trait `trait_name` without a `#[track]` of its own the trait's `default`, refuses
/// `#[track]` on an item that is no stream, and checks that a series stream's item is numbers (types looked up in
/// the bundle being built, which `bundle` makes, only when a series needs it).
pub(crate) fn settle(
    trait_name: &str,
    default: Option<&TrackOwned>,
    items: &mut [ApiItemOwned],
    bundle: impl Fn() -> ApiBundleOwned,
) -> Result<()> {
    let mut built: Option<ApiBundleOwned> = None;
    for item in items {
        let own = item.meta.as_ref().and_then(|m| m.track.as_ref());
        let Some(ty) = stream_item(&item.kind) else {
            if own.is_some() {
                return Err(anyhow!(
                    "#[track] on `{trait_name}::{}`: only a stream (`stream!` or a stream function) is recorded as a \
                     track, see docs/api/metadata.md",
                    item.ident
                ));
            }
            continue;
        };
        let track = match (own, default) {
            (Some(own), _) => own.clone(),
            (None, Some(default)) => {
                item.meta.get_or_insert_with(MetaOwned::default).track = Some(default.clone());
                default.clone()
            }
            (None, None) => continue,
        };
        if track.layout == TrackLayout::Series {
            let bundle = built.get_or_insert_with(&bundle);
            bundle.series_shape(ty).map_err(|e| {
                anyhow!(
                    "#[track(series)] on `{trait_name}::{}`: {e:#}; a series sample is a number, a fixed-size array \
                     of numbers or a struct of numbers, use `points` or `spans` for anything else",
                    item.ident
                )
            })?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_str(s: &str) -> Result<TrackOwned> {
        let mut attr: Attribute = syn::parse_quote!(#[track(points)]);
        attr.meta = syn::parse_str(s).unwrap();
        parse(&attr)
    }

    #[test]
    fn layout_dims_and_flags_parse() {
        let t = parse_str("track(spans, dims(input, producer), flags)").unwrap();
        assert_eq!(t.layout, TrackLayout::Spans);
        assert_eq!(t.dims, ["input", "producer"]);
        assert!(t.flags);
        let t = parse_str("track(series)").unwrap();
        assert_eq!(t.layout, TrackLayout::Series);
        assert!(t.dims.is_empty());
        assert!(!t.flags);
        let t = parse_str("track(points, flags, dims(r#type))").unwrap();
        assert_eq!(t.dims, ["type"]);
    }

    #[test]
    fn malformed_is_refused() {
        for (src, expected) in [
            ("track", "expected #[track("),
            ("track()", "expected #[track("),
            ("track(wobble)", "unknown track layout `wobble`"),
            ("track(flags, points)", "unknown track layout `flags`"),
            ("track(dims(a), points)", "the layout first"),
            ("track(points, points)", "unknown `points`"),
            ("track(points, flags, flags)", "`flags` named twice"),
            (
                "track(points, dims(a), dims(b))",
                "more than one `dims(..)`",
            ),
            ("track(points, dims())", "names no dimension"),
            ("track(points, dims(a, a))", "`a` named twice"),
            ("track(points, colour(a))", "unknown `colour`"),
            ("track(series, flags)", "a series has no flags word"),
            ("track(points = 1)", "expected #[track("),
        ] {
            let e = format!("{:#}", parse_str(src).unwrap_err());
            assert!(e.contains(expected), "{src}: {e}");
        }
        let many: Vec<String> = (0..=TRACK_DIMS_MAX).map(|i| format!("d{i}")).collect();
        let e = parse_str(&format!("track(points, dims({}))", many.join(", ")))
            .unwrap_err()
            .to_string();
        assert!(e.contains("at most 16"), "{e}");
    }
}

#[cfg(test)]
mod load_tests {
    use crate::transform::api::tests::load_src_with;
    use ww_self::{ApiBundleOwned, TrackLayout, TrackOwned};

    fn load(name: &str, src: &str) -> anyhow::Result<ApiBundleOwned> {
        load_src_with(name, "publish = false\n", src)
    }

    fn track<'a>(bundle: &'a ApiBundleOwned, name: &str) -> Option<&'a TrackOwned> {
        let item = bundle.root.items.iter().find(|i| i.ident == name).unwrap();
        item.meta.as_ref().and_then(|m| m.track.as_ref())
    }

    /// A stream's own `#[track]` wins, the trait's is the default of its streams only, and the trait keeps it.
    #[test]
    fn trait_default_and_own() {
        let bundle = load(
            "track_settled",
            r#"
            #[derive_shrink_wrap]
            pub struct Level { pub left: i16, pub right: i16 }
            #[ww_api_root]
            #[effect(edit)]
            #[track(points, dims(input, producer))]
            pub trait Api {
                stream!(events: u8);
                #[track(spans, flags)]
                stream!(detections: u8);
                #[track(series)]
                stream!(levels: Level);
                #[track(series)]
                stream!(window: [f32; 4]);
                #[stream(u16, counted)]
                fn opened(rate: u8);
                fn set();
                property!(rw gain: u8);
            }
            "#,
        )
        .unwrap();
        let events = track(&bundle, "events").unwrap();
        assert_eq!(events.layout, TrackLayout::Points);
        assert_eq!(events.dims, ["input", "producer"]);
        let detections = track(&bundle, "detections").unwrap();
        assert_eq!(detections.layout, TrackLayout::Spans);
        assert!(detections.flags);
        assert!(detections.dims.is_empty(), "own wins whole, not merged");
        assert_eq!(
            track(&bundle, "levels").unwrap().layout,
            TrackLayout::Series
        );
        assert_eq!(
            track(&bundle, "opened").unwrap().layout,
            TrackLayout::Points
        );
        assert_eq!(track(&bundle, "set"), None);
        assert_eq!(track(&bundle, "gain"), None);
        let level = bundle.root.meta.as_ref().unwrap().track.as_ref().unwrap();
        assert_eq!(level.dims, ["input", "producer"]);
    }

    #[test]
    fn misplaced_or_wrong_series_refused() {
        let impls = "#[ww_trait] #[track(points)] pub trait P { stream!(s: u8); }";
        for (src, expected) in [
            (
                "#[track(points)] fn send();",
                "#[track] on `Api::send`: only a stream",
            ),
            (
                "#[track(points)] property!(ro t: u8);",
                "#[track] on `Api::t`",
            ),
            ("#[track(points)] sink!(tx: u8);", "#[track] on `Api::tx`"),
            ("#[track(points)] const C: u8;", "#[track] on `Api::C`"),
            ("#[track(points)] ww_impl!(p: P);", "#[track] on a ww_impl!"),
            (
                "#[track(points)] #[track(spans)] stream!(s: u8);",
                "more than one #[track]",
            ),
            ("#[track(series)] stream!(s: bool);", "it holds a bool"),
            (
                "#[track(series)] stream!(s: Vec<u8>);",
                "a list of varying length",
            ),
            (
                "#[track(series)] stream!(s: Option<u8>);",
                "it holds an option",
            ),
            ("#[track(series)] stream!(s: Mode);", "it holds an enum"),
            ("#[track(series)] stream!(s: Mixed);", "it holds a bool"),
        ] {
            let src = format!(
                "#[derive_shrink_wrap(ww_repr = u4)] pub enum Mode {{ A, B }}\n\
                 #[derive_shrink_wrap] pub struct Mixed {{ pub a: u8, pub on: bool }}\n\
                 #[ww_api_root] #[effect(edit)] pub trait Api {{ {src} }}\n{impls}"
            );
            let e = format!("{:?}", load("track_bad", &src).unwrap_err());
            assert!(e.contains(expected), "{src}: {e}");
        }
        // a trait default only reaches streams, so a trait without any is fine
        load(
            "track_no_streams",
            "#[ww_api_root] #[effect(edit)] #[track(series)] pub trait Api { fn f(); property!(rw g: bool); }",
        )
        .unwrap();
    }
}
