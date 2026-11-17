use super::*;
use crate::tests::id;

fn ident(seq: u64) -> PieceIdent {
    PieceIdent {
        id: PieceId([seq as u8 + 1; 16]),
        owner: OwnerId([0xaa; 32]),
        seq,
    }
}

#[test]
fn piece_head_round_trip() {
    let head = PieceHead {
        ident: ident(3),
        prev: Some((ContentHash([7; 32]), PieceId([3; 16]))),
        prev_session: crate::SessionTag::writable(0x1234_abcd),
    };
    let tracks = [
        TrackStart {
            track_id: 11,
            track_number: 1,
            next_item: 8192,
            from_tick: -5,
        },
        TrackStart {
            track_id: 12,
            track_number: 2,
            next_item: 0,
            from_tick: i64::MIN,
        },
    ];
    let mut buf = vec![0u8; piece_head_len(2)];
    let n = write_piece_head(&mut buf, id(40), &head, &tracks).unwrap();
    assert_eq!(n, 32 + 112 + 64 + 4);
    let block = Block::read(&buf).unwrap();
    assert_eq!((block.kind, block.seq), (BlockKind::PieceHead, 40));
    let h = PieceHeadBlock::read(&block).unwrap();
    assert_eq!(h.head(), head);
    assert_eq!(h.tracks().collect::<Vec<_>>(), tracks);

    let first = PieceHead {
        ident: ident(0),
        prev: None,
        prev_session: crate::SessionTag::NONE,
    };
    let n = write_piece_head(&mut buf, id(0), &first, &[]).unwrap();
    let block = Block::read(&buf[..n]).unwrap();
    assert_eq!(PieceHeadBlock::read(&block).unwrap().head(), first);
}

#[test]
fn finish_and_tail_round_trip() {
    let finish = Finish {
        state: FinishState::Continued,
        hash: ContentHash([9; 32]),
        checkpoint: Some(4 << 20),
        final_index: Some(5 << 20),
        describe: None,
        ident: Some(ident(1)),
        next: Some(PieceId([0x55; 16])),
    };
    let tracks = [TrackRange {
        track_id: 11,
        track_number: 1,
        chunks: 2,
        first_item: 8192,
        items: 8000,
        first_tick: 100,
        last_tick: 900,
        latest_end: 950,
    }];
    let mut buf = vec![0u8; finish_len(1) + 4095];
    for padding in [0, 1, 1000, 4095] {
        let n = write_finish(&mut buf, id(77), &finish, &tracks, padding).unwrap();
        assert_eq!(n, finish_len(1) + padding);
        let block = Block::read(&buf[..n]).unwrap();
        let f = FinishBlock::read(&block).unwrap();
        assert_eq!(f.finish(), finish);
        assert_eq!(f.tracks().collect::<Vec<_>>(), tracks);
    }
    assert_eq!(tracks[0].end_item(), 16192);
    assert!(write_finish(&mut buf, id(77), &finish, &tracks, 4096).is_err());

    let plain = Finish {
        ident: None,
        next: None,
        state: FinishState::Complete,
        ..finish
    };
    let n = write_finish(&mut buf, id(77), &plain, &[], 0).unwrap();
    let block = Block::read(&buf[..n]).unwrap();
    assert_eq!(FinishBlock::read(&block).unwrap().finish(), plain);

    let tail = Tail {
        piece_len: 6 << 20,
        finish_offset: (6 << 20) - 400,
        finish_len: 200,
        state: FinishState::Continued,
        hash: finish.hash,
        ident: finish.ident,
        next: finish.next,
    };
    let mut tb = [0u8; TAIL_LEN];
    assert_eq!(tail.write(&mut tb, id(78)), TAIL_LEN);
    assert_eq!(Tail::read(&tb).unwrap(), tail);
    for at in 0..TAIL_LEN {
        let mut bad = tb;
        bad[at] ^= 1;
        assert!(Tail::read(&bad).is_err(), "byte {at}");
    }
    // a finish that doesn't fit before the tail
    let wrong = Tail {
        finish_offset: 6 << 20,
        ..tail
    };
    wrong.write(&mut tb, id(78));
    assert!(Tail::read(&tb).is_err());
    // offsets whose sum overflows u64
    let wraps = Tail {
        piece_len: TAIL_LEN as u64,
        finish_offset: u64::MAX - 343,
        finish_len: 180,
        ..tail
    };
    wraps.write(&mut tb, id(78));
    assert_eq!(Tail::read(&tb), Err(IndexError::BadHead));
}

#[test]
fn piece_ids_follow_the_chain() {
    let owner = OwnerId([1; 32]);
    let a = PieceId::first(&owner);
    assert_eq!(a, PieceId::first(&owner));
    assert_ne!(a, PieceId::first(&OwnerId([2; 32])));
    let b = PieceId::after(a, &ContentHash([3; 32]));
    assert_ne!(b, a);
    assert_ne!(b, PieceId::after(a, &ContentHash([4; 32])));
    assert!(!a.is_none() && !b.is_none());
}

#[test]
fn chains_of_one_result_start_apart() {
    let owner = OwnerId([1; 32]);
    assert_eq!(PieceId::first(&owner), PieceId::first_of(&owner, 0));
    let firsts: Vec<PieceId> = (0..4).map(|c| PieceId::first_of(&owner, c)).collect();
    for (i, a) in firsts.iter().enumerate() {
        assert_eq!(
            *a,
            PieceId::first_of(&owner, i as u64),
            "the same chain, the same id"
        );
        assert!(firsts[i + 1..].iter().all(|b| b != a), "chain {i}");
    }
    assert_ne!(
        PieceId::first_of(&owner, 1),
        PieceId::first_of(&OwnerId([2; 32]), 1)
    );
    // pinned: a chain's first id is part of the format
    assert_eq!(PieceId::first_of(&owner, 1).0[..4], [241, 163, 28, 235]);
}

fn summary(seq: u64, end: Option<FinishState>) -> PieceSummary {
    let id = |s: u64| PieceId([s as u8 + 1; 16]);
    let hash = |s: u64| ContentHash([s as u8 + 100; 32]);
    PieceSummary {
        ident: PieceIdent {
            id: id(seq),
            owner: OwnerId([0xaa; 32]),
            seq,
        },
        prev: (seq > 0).then(|| (hash(seq - 1), id(seq - 1))),
        end: end.map(|st| {
            (
                st,
                hash(seq),
                (st == FinishState::Continued).then(|| id(seq + 1)),
            )
        }),
    }
}

#[test]
fn chains_are_checked_both_ways() {
    use FinishState::*;
    let whole = [
        summary(2, Some(Complete)),
        summary(0, Some(Continued)),
        summary(1, Some(Continued)),
    ];
    let r = check_chain(&whole);
    assert_eq!(r.order, [1, 2, 0]);
    assert!(r.is_whole());

    // the first piece deleted: head cut, not damage
    let r = check_chain(
        &whole[..1]
            .iter()
            .chain(&whole[2..])
            .copied()
            .collect::<Vec<_>>(),
    );
    assert!(r.head_cut);
    assert_eq!(r.links, [ChainLink::Linked]);
    assert_eq!(r.end, ChainEnd::Complete);

    // a gap, an open end
    let r = check_chain(&[summary(0, Some(Continued)), summary(2, None)]);
    assert_eq!(r.links, [ChainLink::Gap { missing: 1 }]);
    assert_eq!(r.end, ChainEnd::Open);

    // the last piece at hand says more follow
    let r = check_chain(&[summary(0, Some(Continued))]);
    assert_eq!(r.end, ChainEnd::TailCut);

    // a piece rewritten: its hash no longer matches the next head
    let mut changed = summary(0, Some(Continued));
    changed.end = Some((Continued, ContentHash([0; 32]), changed.end.unwrap().2));
    let r = check_chain(&[changed, summary(1, Some(Complete))]);
    assert_eq!(r.links, [ChainLink::Broken]);
    assert!(!r.is_whole());

    // a crash left the first piece unfinished
    let r = check_chain(&[summary(0, None), summary(1, Some(Complete))]);
    assert_eq!(r.links, [ChainLink::Unfinished]);

    // the same seq twice, another owner
    let mut other = summary(1, Some(Complete));
    other.ident.owner = OwnerId([1; 32]);
    let r = check_chain(&[
        summary(0, Some(Continued)),
        summary(0, Some(Continued)),
        other,
    ]);
    assert_eq!(r.order, [0]);
    assert_eq!(r.strays, [1, 2]);
}

#[test]
fn finish_padding_ends_the_piece_on_4_kib() {
    for tracks in [0, 1, 3, 70] {
        for offset in [0, 1, 100, 4095, 4096, 12345, (1 << 40) + 7, u64::MAX - 5] {
            let pad = finish_padding(offset, tracks);
            assert!(pad < 4096);
            let end = offset.wrapping_add((finish_len(tracks) + pad + TAIL_LEN) as u64);
            assert_eq!(end % 4096, 0, "{tracks} tracks at {offset}");
        }
    }
}

#[test]
fn finish_padding_is_checked_against_the_length() {
    let finish = Finish {
        state: FinishState::Complete,
        hash: ContentHash([3; 32]),
        checkpoint: None,
        final_index: None,
        describe: None,
        ident: None,
        next: None,
    };
    let mut buf = vec![0u8; finish_len(0) + 64];
    let n = write_finish(&mut buf, id(5), &finish, &[], 64).unwrap();
    // a padding field that disagrees with the block's length, CRC made good again
    for padding in [0u32, 63, 65, 4095, 4096, u32::MAX] {
        let mut b = buf[..n].to_vec();
        b[172..176].copy_from_slice(&padding.to_le_bytes());
        crate::index::seal(&mut b);
        let block = Block::read(&b).unwrap();
        assert_eq!(
            FinishBlock::read(&block).err(),
            Some(IndexError::BadHead),
            "padding {padding}"
        );
    }
}
