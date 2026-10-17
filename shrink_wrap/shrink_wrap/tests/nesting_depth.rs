//! Values nested deeper than the reader allows ([BufReader::with_max_depth]) are `Error::NestingTooDeep`: a
//! recursive type decoded from untrusted data must not overflow the stack. One test per way a type can contain
//! itself: through an `Unsized` value (a split reader), a `final_structure` one (read in place), the slots of a
//! `sparse` struct, and a `RefBox` read later.

use shrink_wrap::prelude::*;

/// Deep enough to overflow a 2 MiB stack without a limit.
const DEEP: usize = 10_000;

/// Run `f` on a thread with a stack of `mib` MiB: building, serializing and dropping a deep value recurses too.
fn on_stack<T: Send>(mib: usize, f: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|s| {
        std::thread::Builder::new()
            .stack_size(mib << 20)
            .spawn_scoped(s, f)
            .unwrap()
            .join()
            .unwrap()
    })
}

#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq, Clone))]
struct Linked<'i> {
    a: u8,
    next: Option<RefBox<'i, Linked<'i>>>,
}

/// `nodes` nodes, each one's `next` the one after it.
fn linked_bytes(nodes: usize) -> Vec<u8> {
    on_stack(256, || {
        let mut list = LinkedOwned { a: 0, next: None };
        for a in 1..nodes {
            list = LinkedOwned {
                a: a as u8,
                next: Some(Box::new(list)),
            };
        }
        let bytes = list.to_ww_bytes_owned().unwrap();
        drop(list);
        bytes
    })
}

fn linked_owned(bytes: &[u8], max_depth: u16) -> Result<LinkedOwned, ShrinkWrapError> {
    LinkedOwned::des_shrink_wrap_owned(&mut BufReader::new(bytes).with_max_depth(max_depth))
}

#[test]
fn unsized_levels_up_to_the_limit() {
    // the first node is the value read, each `next` one level deeper
    assert!(linked_owned(&linked_bytes(5), 4).is_ok());
    assert_eq!(
        linked_owned(&linked_bytes(6), 4),
        Err(ShrinkWrapError::NestingTooDeep)
    );
    assert!(linked_owned(&linked_bytes(1), 0).is_ok());
    assert_eq!(
        linked_owned(&linked_bytes(2), 0),
        Err(ShrinkWrapError::NestingTooDeep)
    );
    let default = BufReader::DEFAULT_MAX_DEPTH as usize;
    assert!(LinkedOwned::from_ww_bytes_owned(&linked_bytes(default + 1)).is_ok());
    assert_eq!(
        LinkedOwned::from_ww_bytes_owned(&linked_bytes(default + 2)),
        Err(ShrinkWrapError::NestingTooDeep)
    );
}

#[test]
fn deep_owned_box_is_an_error() {
    let bytes = linked_bytes(DEEP);
    let r = on_stack(2, || LinkedOwned::from_ww_bytes_owned(&bytes).map(|_| ()));
    assert_eq!(r, Err(ShrinkWrapError::NestingTooDeep));
}

#[test]
fn ref_box_reads_at_its_depth() {
    // a borrowed RefBox reads its value only when asked: walking the list reads one level at a time, from the
    // reader the RefBox was read with, so the limit holds across the walk too
    let bytes = linked_bytes(DEEP);
    let mut node = Linked::from_ww_bytes(&bytes).unwrap();
    let mut nodes = 1;
    let err = loop {
        match node.next.as_ref().map(|next| next.read()) {
            Some(Ok(next)) => node = next,
            Some(Err(e)) => break e,
            None => panic!("the list ended without an error"),
        }
        nodes += 1;
    };
    assert_eq!(err, ShrinkWrapError::NestingTooDeep);
    assert_eq!(nodes, BufReader::DEFAULT_MAX_DEPTH as usize);
}

/// Contains itself through a `final_structure` value, read in place: no split reader, no length.
#[derive_shrink_wrap(
    owned(feature = "std"),
    final_structure,
    ww_repr = nib,
    derive(Debug, PartialEq, Clone)
)]
enum Tree<'i> {
    Leaf,
    Node(RefVec<'i, Tree<'i>>),
}

fn tree_bytes(levels: usize) -> Vec<u8> {
    on_stack(256, || {
        let mut tree = TreeOwned::Leaf;
        for _ in 0..levels {
            tree = TreeOwned::Node(vec![tree]);
        }
        let bytes = tree.to_ww_bytes_owned().unwrap();
        drop(tree);
        bytes
    })
}

#[test]
fn final_structure_levels() {
    // a level is the list and its element, both final_structure
    let tree = |levels, max_depth| {
        TreeOwned::des_shrink_wrap_owned(
            &mut BufReader::new(&tree_bytes(levels)).with_max_depth(max_depth),
        )
        .map(|_| ())
    };
    assert_eq!(tree(3, 6), Ok(()));
    assert_eq!(tree(3, 5), Err(ShrinkWrapError::NestingTooDeep));
    // the depth is given back after each element: a wide tree is not a deep one
    let wide = on_stack(256, || {
        TreeOwned::Node(vec![TreeOwned::Node(vec![TreeOwned::Leaf]); 1000])
            .to_ww_bytes_owned()
            .unwrap()
    });
    let r = TreeOwned::des_shrink_wrap_owned(&mut BufReader::new(&wide).with_max_depth(4));
    assert!(r.is_ok());
}

#[test]
fn deep_final_structure_is_an_error() {
    let bytes = tree_bytes(DEEP);
    let owned = on_stack(2, || TreeOwned::from_ww_bytes_owned(&bytes).map(|_| ()));
    assert_eq!(owned, Err(ShrinkWrapError::NestingTooDeep));
    // RefVec reads its elements to find its end, so the borrowed one recurses as deep
    let borrowed = on_stack(2, || Tree::from_ww_bytes(&bytes).map(|_| ()));
    assert_eq!(borrowed, Err(ShrinkWrapError::NestingTooDeep));
}

#[test]
fn depth_is_given_back_after_an_error() {
    // a failed read in place leaves the reader at the depth it had, for what the caller reads next
    let bytes = tree_bytes(10);
    let mut rd = BufReader::new(&bytes).with_max_depth(5);
    assert_eq!(
        rd.read_owned::<TreeOwned>(),
        Err(ShrinkWrapError::NestingTooDeep)
    );
    assert_eq!(rd.depth_left(), 5);
}

/// A `sparse` struct reads its fields from the slots of an `OptionAnyVec`, a reader of its own.
#[derive_shrink_wrap(sparse, owned(feature = "std"), derive(Debug, PartialEq))]
struct Sparse<'i> {
    next: Option<RefBox<'i, Sparse<'i>>>,
}

#[test]
fn deep_sparse_is_an_error() {
    let bytes = on_stack(256, || {
        let mut s = SparseOwned { next: None };
        for _ in 0..DEEP {
            s = SparseOwned {
                next: Some(Box::new(s)),
            };
        }
        let bytes = s.to_ww_bytes_owned().unwrap();
        drop(s);
        bytes
    });
    let r = on_stack(2, || SparseOwned::from_ww_bytes_owned(&bytes).map(|_| ()));
    assert_eq!(r, Err(ShrinkWrapError::NestingTooDeep));
}

#[test]
fn sized_values_take_no_level() {
    let mut rd = BufReader::new(&[1, 2, 0x80]).with_max_depth(0);
    assert_eq!(rd.read::<u8>(), Ok(1));
    assert_eq!(rd.read::<u8>(), Ok(2));
    assert_eq!(rd.read::<bool>(), Ok(true));
    assert_eq!(rd.split(0), Err(ShrinkWrapError::NestingTooDeep));
    let mut rd = BufReader::new(&[0; 4]).with_max_depth(2);
    let split = rd.split(2).unwrap();
    assert_eq!(split.depth_left(), 1);
    assert_eq!(rd.depth_left(), 2);
}
