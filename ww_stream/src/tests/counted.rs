use crate::tx::Counted;
use crate::{HeadExt, Opened, ResumeError, StreamCursor};
use wire_weaver::shrink_wrap::{UVlq32, UVlq64};

fn cursor(session: u32, pos: u64) -> StreamCursor {
    StreamCursor {
        session: UVlq32(session),
        pos: UVlq64(pos),
    }
}

#[test]
fn positions_and_lost() {
    let mut c = Counted::new();
    assert_eq!(c.head(3, None), None, "no session, no host");
    assert_eq!(c.stats().lost_no_host, 3);
    assert_eq!(
        c.open(5),
        Opened {
            session: 5,
            oldest: 0,
            live_from: 0,
            resume_from: None
        }
    );
    let h = c.head(37, None).unwrap();
    assert_eq!((h.pos, h.units, h.lost), (0, 37, None));
    c.lost(100);
    let h = c.head(1, None).unwrap();
    assert_eq!((h.pos, h.units, h.lost), (137, 1, Some(100)));
    assert_eq!(h.after_previous(), 37);
    assert_eq!(c.next_pos(), 138);
    assert_eq!(c.stats().sent, 38);
    assert_eq!(c.stats().lost_producer, 100);
}

#[test]
fn unsent_becomes_lost_of_the_next() {
    let mut c = Counted::new();
    c.open(1);
    c.head(10, None).unwrap();
    c.lost(5);
    let h = c.head(4, None).unwrap();
    assert_eq!((h.pos, h.lost), (15, Some(5)));
    c.unsent(&h);
    let next = c.head(2, None).unwrap();
    // the host expected 10 (end of the first): 5 + 4 lost, all the device's
    assert_eq!(
        (next.pos, next.lost, next.after_previous()),
        (19, Some(9), 10)
    );
    assert_eq!(c.stats().sent, 12);
    assert_eq!(c.stats().lost_no_host, 4);
}

#[test]
fn keeps_counting_without_host_and_resumes() {
    let mut c = Counted::new();
    assert_eq!(c.resume(cursor(0, 0)), Err(ResumeError::NoSession));
    c.open(2);
    c.head(10, None).unwrap();
    c.detach();
    assert_eq!(c.head(7, None), None);
    assert_eq!(c.head(3, None), None);
    let opened = c.resume(cursor(2, 10)).unwrap();
    assert_eq!(
        (opened.session, opened.live_from, opened.resume_from),
        (2, 20, None)
    );
    let h = c.head(1, None).unwrap();
    assert_eq!((h.pos, h.lost), (20, Some(10)));
    assert_eq!(c.stats().lost_no_host, 10);
}

#[test]
fn device_started_session_and_other_cursor() {
    let mut c = Counted::new();
    c.start_session(9);
    assert!(!c.is_attached());
    assert_eq!(c.head(4, None), None);
    // a cursor of another session gets the current one
    let opened = c.resume(cursor(3, 1000)).unwrap();
    assert_eq!((opened.session, opened.live_from), (9, 4));
    let h = c.head(1, None).unwrap();
    assert_eq!((h.pos, h.lost), (4, Some(4)));
}

#[test]
fn ext_goes_into_the_next_head_once() {
    let mut c = Counted::new();
    c.open(1);
    let ext = HeadExt {
        codec_restart: false,
        clock_epoch: Some(UVlq32(2)),
    };
    c.set_ext(ext);
    let h = c.head(1, None).unwrap();
    assert_eq!(h.ext, Some(ext));
    c.unsent(&h);
    assert_eq!(c.head(1, None).unwrap().ext, Some(ext));
    assert_eq!(c.head(1, None).unwrap().ext, None);
}

#[test]
fn positions_past_u32_and_2_pow_50() {
    let mut c = Counted::new();
    c.open(1);
    c.head(1, None).unwrap();
    c.lost((1 << 32) + 5);
    let h = c.head(3, None).unwrap();
    assert_eq!((h.pos, h.lost), ((1 << 32) + 6, Some((1 << 32) + 5)));
    c.lost(1 << 50);
    let h = c.head(1, None).unwrap();
    assert_eq!(h.pos, (1 << 50) + (1 << 32) + 9);
}

#[cfg(feature = "cnt")]
#[test]
fn cnt_instance_counters() {
    use crate::counters::StreamCnt;
    static CNT: cnt::Counters<StreamCnt> = cnt::counters!(StreamCnt, test_stream);
    let mut c = Counted::new().counters(&CNT);
    c.open(1);
    c.head(5, None).unwrap();
    c.lost(3);
    c.detach();
    c.head(2, None);
    assert_eq!(CNT.get(StreamCnt::Sent), 5);
    assert_eq!(CNT.get(StreamCnt::LostProducer), 3);
    assert_eq!(CNT.get(StreamCnt::LostNoHost), 2);
    assert_eq!(c.stats().lost(), 5);
}
