#![no_main]

//! Recursive types from random bytes, at a random nesting limit (P2401): every way a type can contain itself (an
//! `Unsized` value through a split reader, a `final_structure` one read in place, the slots of a `sparse` struct,
//! a `RefBox` read later) is bounded by `BufReader::with_max_depth`. A decode never overflows the stack of a small
//! thread, and a limit only decides where it stops: a value that decodes keeps decoding the same with a higher
//! limit, an error other than `NestingTooDeep` stays the same error.

use libfuzzer_sys::fuzz_target;
use shrink_wrap::prelude::*;
use shrink_wrap::{BufReader, Error};

#[derive_shrink_wrap(owned, derive(Debug, PartialEq, Clone))]
struct Linked<'i> {
    a: u8,
    next: Option<RefBox<'i, Linked<'i>>>,
}

#[derive_shrink_wrap(owned, final_structure, ww_repr = nib, derive(Debug, PartialEq, Clone))]
enum Tree<'i> {
    Leaf,
    Node(RefVec<'i, Tree<'i>>),
}

#[derive_shrink_wrap(sparse, owned, derive(Debug, PartialEq, Clone))]
struct Sparse<'i> {
    a: Option<u8>,
    next: Option<RefBox<'i, Sparse<'i>>>,
}

/// All of them in one: lists, boxes, pairs of boxes, a nested `final_structure` tree.
#[derive_shrink_wrap(owned, ww_repr = nib, derive(Debug, PartialEq, Clone))]
enum Node<'i> {
    Leaf(u8),
    List(RefVec<'i, Node<'i>>),
    Boxed(RefBox<'i, Node<'i>>),
    Pair(RefBox<'i, Node<'i>>, RefBox<'i, Node<'i>>),
    Tree(Tree<'i>),
    Linked(Linked<'i>),
    Sparse(Sparse<'i>),
}

fn owned<T: DeserializeShrinkWrapOwned>(buf: &[u8], max_depth: u16) -> Result<T, Error> {
    T::des_shrink_wrap_owned(&mut BufReader::new(buf).with_max_depth(max_depth))
}

fn check_owned<T: DeserializeShrinkWrapOwned + PartialEq + core::fmt::Debug>(
    buf: &[u8],
    depth: u16,
) {
    let at = owned::<T>(buf, depth);
    let deeper = owned::<T>(buf, depth + 1);
    match &at {
        Ok(_) => assert_eq!(at, deeper),
        Err(Error::NestingTooDeep) => {}
        Err(_) => assert_eq!(at, deeper),
    }
    // read as a field of a parent, the reader is given its depth back whatever the outcome
    let mut rd = BufReader::new(buf).with_max_depth(depth);
    let _ = rd.read_owned::<T>();
    assert_eq!(rd.depth_left(), depth);
}

fn check_borrowed<'i, T: DeserializeShrinkWrap<'i> + SerializeShrinkWrap>(
    buf: &'i [u8],
    depth: u16,
) {
    let mut rd = BufReader::new(buf).with_max_depth(depth);
    if let Ok(value) = T::des_shrink_wrap(&mut rd) {
        // writing a borrowed value decodes what its RefBoxes and RefVecs point to, at the depth they were read at
        let mut out = [0u8; 8192];
        let _ = value.to_ww_bytes(&mut out);
    }
}

fn check(kind: u8, depth: u16, buf: &[u8]) {
    match kind % 5 {
        0 => {
            check_owned::<LinkedOwned>(buf, depth);
            check_borrowed::<Linked>(buf, depth);
        }
        1 => {
            check_owned::<TreeOwned>(buf, depth);
            check_borrowed::<Tree>(buf, depth);
        }
        2 => {
            check_owned::<SparseOwned>(buf, depth);
            check_borrowed::<Sparse>(buf, depth);
        }
        _ => {
            check_owned::<NodeOwned>(buf, depth);
            check_borrowed::<Node>(buf, depth);
        }
    }
}

fuzz_target!(|data: &[u8]| {
    // the first byte picks the type, the second the limit (up to the default and a bit past), the rest is the buffer
    let Some(([kind, depth], buf)) = data.split_first_chunk::<2>() else {
        return;
    };
    let depth = (*depth as u16) % (BufReader::DEFAULT_MAX_DEPTH + 8);
    // a small stack: the limit, not the stack size, has to stop a deep decode
    std::thread::scope(|s| {
        std::thread::Builder::new()
            .stack_size(1 << 20)
            .spawn_scoped(s, || check(*kind, depth, buf))
            .unwrap()
            .join()
            .unwrap()
    });
});
