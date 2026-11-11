//! API of the tests of observed properties under resource arrays (API-63), shaped like a media box's control API:
//! a fixed set of browser slots, players that come and go, and one `Playback` trait reached through both arrays.
//! Next to them a flat level, observed as before.
use wire_weaver::prelude::*;

#[ww_api_root]
trait Arrays {
    /// Browser slots 0..3
    ww_impl!(browsers[]: Browser);
    ww_impl!(media: Media);
    property!(ro+observe volume: u8);
    /// Device side changes, as the device's own events would make them
    fn set_browser_state(slot: u32, state: u8);
    fn set_title(player: u32, title: &'i str);
    fn add_player(player: u32);
    fn remove_player(player: u32);
    /// Changes nothing: its answer comes after everything the device sent before
    fn barrier();
}

#[ww_trait]
trait Browser {
    property!(ro+observe state: u8);
    /// `value_on_changed`, written by hosts: the write marks the element changed (SRV-57)
    property!(rw+observe zoom: u8);
    ww_impl!(playback: Playback);
}

#[ww_trait]
trait Media {
    /// Players by id, the live ones
    ww_impl!(players[]: Player);
}

#[ww_trait]
trait Player {
    property!(ro source: u8);
    ww_impl!(playback: Playback);
}

#[ww_trait]
trait Playback {
    property!(ro+observe title: &'i str);
    fn play();
}
