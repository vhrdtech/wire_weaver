use super::*;
use crate::{Framer, Medium, Side};
use wire_weaver::shrink_wrap::UNib32;

fn session(peer: &str) -> Session<'_> {
    Session {
        side: Side::Host,
        medium: Medium::Usb,
        framer: Framer::None,
        frame_size: UNib32(64),
        peer,
        device_info: RefVec::Slice { slice: &[] },
    }
}

/// Every stream's track is found again from its definition, with the link's identity.
#[test]
fn a_link_s_tracks_are_found_by_their_definitions() {
    let mut defs = Definitions::new();
    let clock = define_clocks(&mut defs, 1_000);
    let identity = LinkIdentity::of(&session("SN-1"), None);
    let tracks = define_link_tracks(&mut defs, clock, &identity);
    for (stream, track) in tracks.into_iter().enumerate() {
        let (found, s) = link_stream_in(&defs, track).expect("a tap track");
        assert_eq!(found, identity);
        assert_eq!(s as usize, stream);
    }
    assert_eq!(identity.get(dims::PEER), Some("SN-1"));
    assert_eq!(identity.to_string(), "medium=usb peer=SN-1");
}

/// The same device gives the same track ids whatever else a capture holds; another peer gives others.
#[test]
fn a_device_has_the_same_track_ids_in_every_capture() {
    let a = LinkIdentity::of(&session("SN-1"), None);
    let b = LinkIdentity::of(&session("SN-1"), None);
    let other = LinkIdentity::of(&session("SN-2"), None);
    for stream in 0..u32::try_from(STREAMS).unwrap() {
        assert_eq!(a.track_id(stream), b.track_id(stream));
        assert_ne!(a.track_id(stream), other.track_id(stream));
    }
    assert_ne!(
        LinkIdentity::of(&session(""), Some(3)),
        LinkIdentity::numbered(3)
    );
}

/// A records block with a link's Closed event drops that link's tracks from a relay's definitions, not another's.
#[test]
fn a_closed_link_s_tracks_are_dropped() {
    let mut defs = Definitions::new();
    let clock = define_clocks(&mut defs, 0);
    let one = define_link_tracks(&mut defs, clock, &LinkIdentity::numbered(0));
    let two = define_link_tracks(&mut defs, clock, &LinkIdentity::numbered(1));
    let mut closed = [0u8; 8];
    let closed = LinkEvent::Closed.to_ww_bytes(&mut closed).unwrap();
    let mut buf = vec![0; 4096];
    let id = ww_tape::BlockId::new(ww_tape::SessionTag::writable(1), 0);
    let mut w =
        ww_tape::BlockWriter::new(&mut buf, ww_tape::BlockKind::Records, id, &[clock]).unwrap();
    w.push(
        one[ids::EVENTS as usize],
        ww_tape::ClockTime::new(clock, 5),
        &[closed],
    )
    .unwrap();
    let len = w.finish();
    let block = Block::read(&buf[..len]).unwrap();
    assert_eq!(drop_closed_links(&mut defs, &block), 1);
    assert!(link_stream_in(&defs, one[0]).is_none());
    assert!(link_stream_in(&defs, two[0]).is_some());
}
