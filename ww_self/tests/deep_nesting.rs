//! Introspection data comes from the device: a malformed or malicious one must not take the host down. A type or
//! value nested deeper than the reader allows is `Error::NestingTooDeep`, not a stack overflow (P2401).

use shrink_wrap::prelude::*;
use ww_self::{TypeOwned, Value, ValueOwned};

/// Far deeper than the default limit, and deep enough to overflow a 2 MiB stack without it.
const DEPTH: usize = 10_000;

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

fn deep_type_bytes(depth: usize) -> Vec<u8> {
    on_stack(256, || {
        let mut ty = TypeOwned::Bool;
        for _ in 0..depth {
            ty = TypeOwned::Vec(Box::new(ty));
        }
        let bytes = ty.to_ww_bytes_owned().unwrap();
        // dropping 10k nested boxes recurses as well
        drop(ty);
        bytes
    })
}

fn deep_value_bytes(depth: usize) -> Vec<u8> {
    on_stack(256, || {
        let mut value = ValueOwned::Bool(true);
        for _ in 0..depth {
            value = ValueOwned::Vec(vec![value]);
        }
        let bytes = value.to_ww_bytes_owned().unwrap();
        drop(value);
        bytes
    })
}

#[test]
fn deep_type_owned_is_an_error() {
    let bytes = deep_type_bytes(DEPTH);
    // the default spawned thread's stack size
    let r = on_stack(2, || TypeOwned::from_ww_bytes_owned(&bytes).map(|_| ()));
    assert_eq!(r, Err(ShrinkWrapError::NestingTooDeep));
}

#[test]
fn deep_value_owned_is_an_error() {
    let bytes = deep_value_bytes(DEPTH);
    let r = on_stack(2, || ValueOwned::from_ww_bytes_owned(&bytes).map(|_| ()));
    assert_eq!(r, Err(ShrinkWrapError::NestingTooDeep));
}

#[test]
fn deep_value_borrowed_is_an_error() {
    // RefVec reads its elements to find its end, so a borrowed Value recurses as deep as the owned one
    let bytes = deep_value_bytes(DEPTH);
    let r = on_stack(2, || Value::from_ww_bytes(&bytes).map(|_| ()));
    assert_eq!(r, Err(ShrinkWrapError::NestingTooDeep));
}

#[test]
fn nesting_within_the_limit_decodes() {
    // a type nested 100 levels deep is far more than a real API has, and decodes on a 2 MiB stack
    let bytes = deep_type_bytes(100);
    let r = on_stack(2, || {
        let ty = TypeOwned::from_ww_bytes_owned(&bytes)?;
        let mut depth = 0;
        let mut node = &ty;
        while let TypeOwned::Vec(inner) = node {
            depth += 1;
            node = inner;
        }
        assert_eq!(node, &TypeOwned::Bool);
        Ok::<_, ShrinkWrapError>(depth)
    });
    assert_eq!(r, Ok(100));
}
