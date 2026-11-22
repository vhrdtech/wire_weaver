//! How a capture names the tracks of a link tap in `ww_tape`: one track per stream of [LinkTap](crate::LinkTap),
//! keyed by the stream's name path ([track_key]) and told apart by the link's identity ([LinkIdentity], the
//! track's dimensions), so the same device has the same track ids in every capture. A producer defines the tap
//! clock ([define_clocks]) and each link's tracks ([define_link_tracks]); a reader finds a link and stream by a
//! track's definition ([link_stream_of]), never by its number.

use wire_weaver::prelude::*;
use ww_tape::{
    Block, ClockMap, ClockNum, ClockOrigin, ClockSpec, Definitions, Describe, Dimension, Magnitude,
    MapSegment, Segment, Slot, TrackDef, TrackId, TrackNum, TrackRepeat, TrackSpec,
};

use crate::{LinkEvent, Session, api, dims, ids};

/// Streams of a link, each a track: the ids of [ids] from 0 on.
pub const STREAMS: usize = 7;

/// Stable key of a tap stream, the [ww_tape::track_key] of its name path `<stream>` in [LinkTap](crate::LinkTap):
/// the same for every link, whose tracks the [LinkIdentity] tells apart.
///
/// # Panics
///
/// When `stream` is not one of [ids]' streams.
pub fn track_key(stream: u32) -> ww_tape::TrackKey {
    let name = ids::stream_name(stream).expect("stream is one of ids' streams");
    ww_tape::track_key(ww_tape::api_root_id(api().crate_id), &[Segment::Name(name)])
}

/// The identity of a tapped link: its tracks' dimensions ([dims]), in the ascending order of name a definition
/// keeps them in.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LinkIdentity {
    /// Name and value of each dimension
    pub dimensions: Vec<(String, String)>,
}

impl LinkIdentity {
    /// The identity of a link whose first session is `session`: its medium and peer, and the producer's number of
    /// the link `link` when given (no peer, or another open link of the same medium and peer).
    pub fn of(session: &Session<'_>, link: Option<u32>) -> Self {
        let mut dimensions = Vec::with_capacity(3);
        if let Some(link) = link {
            dimensions.push((dims::LINK.to_owned(), link.to_string()));
        }
        dimensions.push((dims::MEDIUM.to_owned(), session.medium.name().to_owned()));
        if !session.peer.is_empty() {
            dimensions.push((dims::PEER.to_owned(), session.peer.to_owned()));
        }
        LinkIdentity { dimensions }
    }

    /// The identity of a link recorded before any session: only the producer's number of it.
    pub fn numbered(link: u32) -> Self {
        LinkIdentity {
            dimensions: vec![(dims::LINK.to_owned(), link.to_string())],
        }
    }

    /// The identity a track's definition carries.
    pub fn of_track(def: &TrackDef<'_>) -> Self {
        LinkIdentity {
            dimensions: def
                .dimensions
                .iter()
                .filter_map(Result::ok)
                .map(|d| (d.name.to_owned(), d.value.to_owned()))
                .collect(),
        }
    }

    /// The value of dimension `name` ([dims]), None when the link has none: how a reader picks a device
    /// ([dims::PEER]).
    pub fn get(&self, name: &str) -> Option<&str> {
        let (_, value) = self.dimensions.iter().find(|(n, _)| n == name)?;
        Some(value)
    }

    /// As ww_tape's dimensions.
    pub fn dimensions(&self) -> impl Iterator<Item = Dimension<'_>> + Clone {
        self.dimensions
            .iter()
            .map(|(name, value)| Dimension { name, value })
    }

    /// The [ww_tape::track_id] of `stream`'s track of the link.
    pub fn track_id(&self, stream: u32) -> TrackId {
        ww_tape::track_id(track_key(stream), self.dimensions())
    }
}

impl core::fmt::Display for LinkIdentity {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for (i, (name, value)) in self.dimensions.iter().enumerate() {
            let sep = if i == 0 { "" } else { " " };
            write!(f, "{sep}{name}={value}")?;
        }
        Ok(())
    }
}

/// The link and stream id of the tap track `def` defines: its path `<stream>` in [LinkTap](crate::LinkTap), with
/// its key from [track_key], and its link's identity from its dimensions. None for any other track. How a reader
/// finds a link's stream: the number is the definitions', not the link's.
pub fn link_stream_of(def: &TrackDef<'_>) -> Option<(LinkIdentity, u32)> {
    let ww_tape::Items::WireWeaver { path, .. } = &def.items else {
        return None;
    };
    let mut path = path.iter();
    let (Some(Ok(UNib32(stream))), None) = (path.next(), path.next()) else {
        return None;
    };
    ids::stream_name(stream)?;
    (def.key == track_key(stream)).then(|| (LinkIdentity::of_track(def), stream))
}

/// The link's identity and stream id of tap track `track` held in `defs`, see [link_stream_of].
pub fn link_stream_in(defs: &Definitions, track: TrackNum) -> Option<(LinkIdentity, u32)> {
    let item = defs.get(&Slot::Track { number: track })?;
    match Describe::from_ww_bytes(item) {
        Ok(Describe::Track(t)) => link_stream_of(&t),
        _ => None,
    }
}

/// Define in `defs` the tap clock (ns since the producer started) and the wall clock it maps to, read
/// `wall_ns_at_zero` at the start; the tap clock's number. A reader takes record times on this clock.
pub fn define_clocks(defs: &mut Definitions, wall_ns_at_zero: u64) -> ClockNum {
    let api_root = ww_tape::api_root_id(api().crate_id);
    let host = defs
        .define_clock(ClockSpec {
            // a session of the producer is a clock of its own
            key: ww_tape::clock_key(api_root, &[], wall_ns_at_zero),
            rate: Magnitude::pow10(9).into(),
            resolution: Magnitude::pow10(-9).into(),
            origin: ClockOrigin::SessionStart,
            label: "tap",
        })
        .expect("the tap clock definition is valid");
    let wall = defs
        .define_clock(ClockSpec {
            key: ww_tape::UNIX_CLOCK_KEY,
            rate: Magnitude::pow10(9).into(),
            resolution: Magnitude::pow10(-9).into(),
            origin: ClockOrigin::UnixEpoch,
            label: "wall",
        })
        .expect("the wall clock definition is valid");
    let segments = [MapSegment::offset(0, wall_ns_at_zero as i64)];
    defs.define(&Describe::Map(ClockMap {
        from: host,
        to: wall,
        segments: RefVec::Slice { slice: &segments },
    }))
    .expect("the map between two held clocks is valid");
    host
}

/// Define in `defs` the tracks of a link with the dimensions of `identity`, on the tap clock `clock` (from
/// [define_clocks]): one track per stream of [LinkTap](crate::LinkTap), its number at the stream's id ([ids]). The
/// session track is a state track.
///
/// # Panics
///
/// When `defs` doesn't hold `clock`.
pub fn define_link_tracks(
    defs: &mut Definitions,
    clock: ClockNum,
    identity: &LinkIdentity,
) -> [TrackNum; STREAMS] {
    let api = api();
    let dimensions: Vec<Dimension<'_>> = identity.dimensions().collect();
    core::array::from_fn(|stream| {
        let stream = u32::try_from(stream).expect("a stream id fits u32");
        let path = [UNib32(stream)];
        let label = format!(
            "{} {identity}",
            ids::stream_name(stream).expect("one of ids' streams")
        );
        defs.define_track(TrackSpec {
            key: track_key(stream),
            clock,
            dimensions: RefVec::Slice { slice: &dimensions },
            items: ww_tape::Items::WireWeaver {
                api,
                bundle: RefVec::Slice { slice: &[] },
                path: RefVec::Slice { slice: &path },
                keys: RefVec::Slice { slice: &[] },
                item: None,
            },
            label: &label,
            repeat: if stream == ids::SESSION {
                TrackRepeat::STATE
            } else {
                TrackRepeat::Stream
            },
            kind: ww_tape::TrackKind::Events(ww_tape::EventsDef::INSTANTS),
            // zeros: the owner of the piece; the link's tracks belong to the capture itself
            owner: [0; 32],
            relations: RefVec::Slice { slice: &[] },
        })
        .expect("a link's track definition is valid and its clock held")
    })
}

/// Drop from `defs` the tracks of each link a [LinkEvent::Closed] in the records block `block` closes, as a relay of
/// the stream does once it has passed the block on, so its definitions don't grow with every link ever seen. A
/// link's tracks are found by their definitions in `defs` ([link_stream_in]). How many links were closed.
pub fn drop_closed_links(defs: &mut Definitions, block: &Block<'_>) -> usize {
    let mut closed = 0;
    for r in block.records().flatten() {
        let Some(track) = r.track.data() else {
            continue;
        };
        let Some((link, ids::EVENTS)) = link_stream_in(defs, track) else {
            continue;
        };
        if matches!(LinkEvent::from_ww_bytes(r.item), Ok(LinkEvent::Closed)) {
            let tracks: Vec<TrackNum> = defs
                .tracks()
                .filter(|t| link_stream_in(defs, *t).is_some_and(|(l, _)| l == link))
                .collect();
            for t in tracks {
                defs.close(t);
            }
            closed += 1;
        }
    }
    closed
}

#[cfg(test)]
mod tests;
