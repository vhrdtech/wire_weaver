//! How a recorder lays a stream out on a tape (API-71), `#[track(..)]` in the API: [Track].

use shrink_wrap::prelude::*;

/// How a recorder lays the items of a stream out on a tape (API-71): `#[track(points | spans | series, dims(..),
/// flags)]` on a `stream!` or a stream function, or on its trait for every stream of it without one. A stream without
/// one is recorded as points without dimensions. See `docs/api/metadata.md`.
///
/// The model names no tape type: a recorder (`ww_tape_bundle`) maps [TrackLayout] to its own track kinds.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_owned(feature = "serde", derive(serde::Deserialize, serde::Serialize)),
    derive(Clone, Debug, PartialEq, Eq)
)]
pub struct Track<'i> {
    /// What one item of the stream is on the tape.
    pub layout: TrackLayout,
    /// `dims(input, producer, ..)`: names of the dimensions a track of this stream may carry, in the order written.
    /// A writer gives their values; a name not listed here is refused. Empty: no dimensions.
    pub dims: RefVec<'i, &'i str>,
    /// `flags`: every item carries a `u32` flags word beside it (severity in its low bits, which summaries OR
    /// together). Points and spans only.
    pub flags: bool,
}

/// What one item of a stream is on a tape (API-71), the first word of `#[track(..)]`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "serde", derive(serde::Deserialize, serde::Serialize)),
    derive(Clone, Copy, Debug, PartialEq, Eq, Hash),
    ww_repr = nib,
    no_future_version
)]
pub enum TrackLayout {
    /// `points`: an event at an instant, the item its payload. What a stream without `#[track]` is.
    Points,
    /// `spans`: an event with a start and an end, which the tape holds; the item holds the rest.
    Spans,
    /// `series`: samples at an exact period, no time per item. The item is a number, a fixed-size array of numbers
    /// or a struct of numbers (each number a channel); the writer gives the period.
    Series,
}

impl TrackLayout {
    /// Every layout with its name as `#[track(..)]` writes it.
    pub const NAMED: [(TrackLayout, &'static str); 3] = [
        (TrackLayout::Points, "points"),
        (TrackLayout::Spans, "spans"),
        (TrackLayout::Series, "series"),
    ];

    /// The name as `#[track(..)]` writes it.
    pub const fn name(self) -> &'static str {
        match self {
            TrackLayout::Points => "points",
            TrackLayout::Spans => "spans",
            TrackLayout::Series => "series",
        }
    }

    /// The layout named `name` as `#[track(..)]` writes it, `None` for an unknown name.
    pub fn from_name(name: &str) -> Option<TrackLayout> {
        TrackLayout::NAMED
            .iter()
            .find(|(_, n)| *n == name)
            .map(|(l, _)| *l)
    }
}

impl core::fmt::Display for TrackLayout {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.name())
    }
}

#[cfg(feature = "std")]
mod series {
    use crate::{ApiBundleOwned, FieldsOwned, Length, TrackOwned, TypeLocationOwned, TypeOwned};
    use anyhow::{Result, anyhow, bail};

    /// Nesting of arrays and structs a series sample may have: a bound on a walk over an untrusted bundle.
    pub const SERIES_DEPTH_MAX: usize = 16;

    /// The channels of a series sample: one per number of the item, in field order.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum SeriesShape {
        /// Every number is known: this many.
        Channels(u32),
        /// The item reaches a definition the bundle left out (a dependency's, skipped): what it holds is known only
        /// once the bundle is resolved.
        Unresolved,
    }

    impl ApiBundleOwned {
        /// The shape of a series sample of type `ty` (API-71): a number, a fixed-size array of numbers or a struct of
        /// numbers, nested; a struct with one field (`Fixed<..>`) is its field. Fails for anything else (a bool, a
        /// string, a vector, an enum, an option) with what it is.
        pub fn series_shape(&self, ty: &TypeOwned) -> Result<SeriesShape> {
            let mut channels = 0u32;
            if !self.series_leaves(ty, 0, &mut channels)? {
                return Ok(SeriesShape::Unresolved);
            }
            if channels == 0 {
                bail!("it holds no number");
            }
            Ok(SeriesShape::Channels(channels))
        }

        /// Adds the numbers of `ty` to `channels`; false when a skipped definition is reached.
        fn series_leaves(&self, ty: &TypeOwned, depth: usize, channels: &mut u32) -> Result<bool> {
            if depth > SERIES_DEPTH_MAX {
                bail!("nested deeper than {SERIES_DEPTH_MAX} levels");
            }
            let not = |what: &str| {
                Err(anyhow!(
                    "it holds {what}, a series sample holds only numbers"
                ))
            };
            match ty {
                TypeOwned::NumericAny(_) => {
                    *channels = channels
                        .checked_add(1)
                        .ok_or_else(|| anyhow!("too many numbers"))?;
                    Ok(true)
                }
                TypeOwned::Array { len, ty } => {
                    let Length::Fixed(len) = len else {
                        bail!("an array whose length is a generic parameter");
                    };
                    let mut one = 0u32;
                    if !self.series_leaves(ty, depth + 1, &mut one)? {
                        return Ok(false);
                    }
                    let all = one
                        .checked_mul(len.0)
                        .ok_or_else(|| anyhow!("too many numbers"))?;
                    *channels = channels
                        .checked_add(all)
                        .ok_or_else(|| anyhow!("too many numbers"))?;
                    Ok(true)
                }
                TypeOwned::Struct(s) => {
                    let fields = match &s.fields {
                        FieldsOwned::Named(f) | FieldsOwned::Unnamed(f) => f,
                        FieldsOwned::Unit => return not("a unit struct"),
                    };
                    for field in fields {
                        if !self.series_leaves(&field.ty, depth + 1, channels)? {
                            return Ok(false);
                        }
                    }
                    Ok(true)
                }
                TypeOwned::OutOfLine { type_idx } => match self.types.get(type_idx.0 as usize) {
                    Some(TypeLocationOwned::InLine { ty, .. }) => {
                        self.series_leaves(ty, depth + 1, channels)
                    }
                    Some(TypeLocationOwned::SkippedFullVersion { .. }) => Ok(false),
                    None => bail!("no type with index {}", type_idx.0),
                },
                TypeOwned::Instance { type_idx, args } => {
                    if let Some(TypeLocationOwned::SkippedFullVersion { .. }) =
                        self.types.get(type_idx.0 as usize)
                    {
                        return Ok(false);
                    }
                    let ty = crate::generics::substitute(self, type_idx.0, args)?;
                    self.series_leaves(&ty, depth + 1, channels)
                }
                TypeOwned::Bool | TypeOwned::Flag => not("a bool"),
                TypeOwned::String => not("a string"),
                TypeOwned::Vec(_) | TypeOwned::MaxLen { .. } | TypeOwned::Series(_) => {
                    not("a list of varying length")
                }
                TypeOwned::Tuple(_) => not("a tuple"),
                TypeOwned::Enum(_) => not("an enum"),
                TypeOwned::Option { .. } => not("an option"),
                TypeOwned::Result { .. } => not("a result"),
                TypeOwned::Box(_) => not("a box"),
                TypeOwned::Range(_) | TypeOwned::RangeInclusive(_) => not("a range"),
                TypeOwned::Param(_) => not("a generic parameter"),
            }
        }
    }

    impl TrackOwned {
        /// Whether a writer may give a dimension named `name`: it is one of [dims](TrackOwned::dims).
        pub fn allows_dimension(&self, name: &str) -> bool {
            self.dims.iter().any(|d| d == name)
        }
    }
}
#[cfg(feature = "std")]
pub use series::{SERIES_DEPTH_MAX, SeriesShape};

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;

    #[test]
    fn layout_names_round_trip() {
        for (layout, name) in TrackLayout::NAMED {
            assert_eq!(layout.name(), name);
            assert_eq!(TrackLayout::from_name(name), Some(layout));
            assert_eq!(layout.to_string(), name);
        }
        assert_eq!(TrackLayout::from_name("state"), None);
    }

    #[test]
    fn dims_allowed_by_name() {
        let track = TrackOwned {
            layout: TrackLayout::Spans,
            dims: vec!["input".into(), "producer".into()],
            flags: true,
        };
        assert!(track.allows_dimension("producer"));
        assert!(!track.allows_dimension("platform"));
    }
}
