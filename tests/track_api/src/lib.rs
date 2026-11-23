//! Test API of `#[track(..)]` (API-71), used by `tests/track`: how a recorder lays each stream out on a tape. The
//! trait's `#[track]` is the default of its streams, a stream's own wins; a sub-trait has its own default.

use ww_api::prelude::*;
use ww_si::prefix::{Milli, One};
use ww_si::units::{Item, Volt};
use ww_si::{Fixed, Range};

/// A device that detects things in an input and samples its levels.
#[ww_api_root]
#[track(points, dims(input, producer))]
pub trait Track {
    /// Something seen, at an instant: the trait's default, points with the input and producer dimensions.
    stream!(events: Event);

    /// A detection with a start and an end, and a severity in its flags word.
    #[track(spans, dims(input), flags)]
    stream!(detections: Detection);

    /// Left and right levels, two channels of a series.
    #[track(series, dims(input))]
    stream!(levels: Levels);

    /// Four probe voltages at once, four channels of a series.
    #[track(series)]
    stream!(probes: [Fixed<Volt, Milli, i16>; 4]);

    /// A stream function, opened by calling it: also a stream a recorder records.
    #[effect(read)]
    #[stream(Event)]
    fn watch(threshold: Fixed<Item, One, u8>);

    /// The detector's own streams.
    ww_impl!(detector: Detector);
}

/// A detector with a default of its own: spans.
#[ww_trait]
#[track(spans)]
pub trait Detector {
    /// Hits, spans by the trait's default.
    stream!(hits: Event);
}

/// What was seen.
#[derive_shrink_wrap(borrowed, derive(Debug, Clone, PartialEq))]
pub struct Event {
    /// How many times in a row.
    pub count: Fixed<Item, One, u16>,
}

/// A detection: the tape holds its start and end.
#[derive_shrink_wrap(borrowed, derive(Debug, Clone, PartialEq))]
pub struct Detection {
    /// How sure the detector is, in percent.
    pub confidence: Fixed<ww_si::units::Ratio, ww_si::prefix::Centi, u8, Range<0, 100>>,
}

/// Levels of both sides.
#[derive_shrink_wrap(borrowed, derive(Debug, Clone, PartialEq))]
pub struct Levels {
    /// Left level.
    pub left: Fixed<Volt, Milli, i16>,
    /// Right level.
    pub right: Fixed<Volt, Milli, i16>,
}
