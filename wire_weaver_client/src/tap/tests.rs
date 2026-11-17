use std::sync::{Arc, Mutex};

use wire_weaver::shrink_wrap::{RefVec, UNib32};

use super::*;

/// What a fake tap was told, as text.
#[derive(Clone, Default)]
struct Log(Arc<Mutex<Vec<String>>>);

impl Log {
    fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }
}

struct FakeTap {
    log: Log,
    frames: bool,
}

impl Tap for FakeTap {
    fn wants_frames(&self) -> bool {
        self.frames
    }
    fn session(&mut self, session: &Session<'_>) {
        let info = session.device_info.as_slice().to_vec();
        let line = format!("session {} {info:?}", session.peer);
        self.log.0.lock().unwrap().push(line);
    }
    fn frame(&mut self, dir: Dir, bytes: &[u8]) {
        self.log
            .0
            .lock()
            .unwrap()
            .push(format!("frame {dir:?} {bytes:?}"));
    }
    fn message(&mut self, dir: Dir, kind: u8, bytes: &[u8]) {
        let line = format!("message {dir:?} {kind} {bytes:?}");
        self.log.0.lock().unwrap().push(line);
    }
    fn link_event(&mut self, event: &LinkEvent<'_>) {
        self.log.0.lock().unwrap().push(format!("event {event:?}"));
    }
}

fn source(log: &Log, frames: bool) -> TapSource {
    let log = log.clone();
    TapSource::new(move || {
        Box::new(FakeTap {
            log: log.clone(),
            frames,
        })
    })
}

fn session(peer: &str) -> Session<'_> {
    Session {
        side: ww_tap::Side::Host,
        medium: ww_tap::Medium::InProcess,
        framer: ww_tap::Framer::None,
        frame_size: UNib32(0),
        peer,
        device_info: RefVec::Slice { slice: &[] },
    }
}

#[test]
fn without_a_source_nothing_is_wanted() {
    let tap = ClientTap::new(None);
    assert!(!tap.wants_frames());
    assert!(!tap.wants_messages());
}

#[test]
fn the_sources_tap_gets_the_link_and_the_session_again_once_ready() {
    let log = Log::default();
    let mut tap = ClientTap::new(Some(&source(&log, true)));
    let mut rx_half = tap.clone();
    assert!(tap.wants_frames() && tap.wants_messages());
    tap.session(&session("dev"));
    tap.frame(Dir::Tx, &[1, 2]);
    rx_half.message(Dir::Rx, 0, &[3]);
    tap.link_ready(&[9, 9]);
    assert_eq!(
        log.take(),
        [
            "session dev []",
            "frame Tx [1, 2]",
            "message Rx 0 [3]",
            "session dev [9, 9]",
            "event LinkReady",
        ]
    );
    let without_frames = ClientTap::new(Some(&source(&log, false)));
    assert!(!without_frames.wants_frames());
}

#[test]
fn tracers_get_data_messages_and_link_changes_until_gone() {
    let mut tap = ClientTap::new(None);
    let (tx, mut rx) = mpsc::unbounded_channel();
    tap.add_tracer(tx);
    assert!(tap.wants_messages() && !tap.wants_frames());
    tap.link_ready(&[]);
    tap.message(Dir::Tx, ww_link::Kind::Data0 as u8, &[1]);
    // an empty event and a link message are no data
    tap.message(Dir::Rx, ww_link::Kind::Data0 as u8, &[]);
    tap.message(Dir::Rx, ww_link::Kind::Data2 as u8 + 1, &[5]);
    tap.message(Dir::Rx, ww_link::Kind::Data1 as u8, &[2]);
    tap.link_event(&LinkEvent::Error { what: "e" });
    tap.link_event(&LinkEvent::Disconnected {
        reason: "r",
        keep_streams: true,
    });
    let mut got = vec![];
    while let Ok(ev) = rx.try_recv() {
        got.push(format!("{ev:?}"));
    }
    assert_eq!(
        got,
        [
            "Connected",
            "Request { bytes: [1] }",
            "Event { bytes: [2] }",
            "Error { reason: \"e\" }",
            "Disconnected { reason: \"r\", keep_streams: true }",
        ]
    );
    drop(rx);
    tap.message(Dir::Tx, ww_link::Kind::Data0 as u8, &[1]);
    assert!(!tap.wants_messages(), "the gone tracer is dropped");
}
