//! Client API for multi requests (`MultiRead`, `MultiCall`, `MultiWrite`, CLT-15): several resources answered in
//! one request/response pair, each on its own (`Either<ErrorOwned, value>` per index, in index order).
//!
//! Two shapes are possible, both reduced to the same wire shape by `split_common` (private): a request path (the
//! common prefix), a `multi_idx` (the one path element that differs between indices) and an optional trailing
//! `in_each_array_id` (one more path element, the same for every index).
//! - **Different resources at one level**: [MultiRead], [MultiCall], [MultiWrite] are implemented for tuples of
//!   2 to 8 of the generated client's `PreparedRead`/`PreparedCall`/`PreparedWrite` builders, e.g.
//!   `(client.read_x(), client.read_y(), client.read_mode()).multi_read().await`.
//! - **The same resource across an array**: [read_array], [call_array] and [write_array] take a `MultiIndexOwned`
//!   selector (`Range`/`List`/`Mask32`, sent as given - not rebuilt as a `List`) and a closure that is just the
//!   existing generated per-index accessor, e.g. `read_array(MultiIndexOwned::Range(UNib32(0)..UNib32(4)), |i|
//!   client.pin(i).read_mode())`.
//!
//! The multi request goes at the highest [Priority](crate::Priority) one of its items asked for (`.urgent()` on any
//! of them makes the whole request urgent).
//!
//! `MultiArgs::Same`/`Different` is chosen automatically: if every index's serialized bytes are equal, `Same` is
//! used (smaller on the wire), otherwise `Different`.
use std::time::Duration;

use wire_weaver::shrink_wrap::{
    BufReader, DeserializeShrinkWrap, DeserializeShrinkWrapOwned, SerializeShrinkWrapOwned, UNib32,
    either_any_vec::EitherAnyVec,
};
use ww_client_server::{
    ErrorOwned, MultiArgsOwned, MultiIndexOwned, PathKindOwned, RequestKindOwned,
};

use crate::{Error, event_loop::commander::TransportCommander};

/// Resource path (and commander) of a [PreparedRead](crate::PreparedRead), extracted for a multi-read.
#[doc(hidden)]
pub trait PropertyPath {
    type Output: DeserializeShrinkWrapOwned;

    fn absolute_path(&self) -> Option<Vec<UNib32>>;
    fn commander(&self) -> TransportCommander;
}

/// Resource path, serialized arguments (and commander) of a [PreparedCall](crate::PreparedCall), extracted for a
/// multi-call.
#[doc(hidden)]
pub trait CallPath {
    /// What the method puts on the wire for one index: `T`, or `Result<T, D>` for a method returning one.
    type Item: DeserializeShrinkWrapOwned;
    /// What one index answers the user: `Result<T, ErrorOwned>`, or for a method returning `Result<T, D>` the same
    /// `Result<T, CallError<D>>` as its single call (CLT-48).
    type Output;

    fn absolute_path(&self) -> Option<Vec<UNib32>>;
    fn args_bytes(&self) -> Vec<u8>;
    fn commander(&self) -> TransportCommander;
    /// Turns an index's answer into [Self::Output], checking it as a single call would (`#[errors(..)]` list).
    fn output(&self, item: Result<Self::Item, ErrorOwned>) -> Self::Output;
}

/// Resource path, serialized value (and commander) of a [PreparedWrite](crate::PreparedWrite), extracted for a
/// multi-write.
#[doc(hidden)]
pub trait WritePath {
    fn absolute_path(&self) -> Option<Vec<UNib32>>;
    fn value_bytes(&self) -> Vec<u8>;
    fn commander(&self) -> TransportCommander;
}

/// `(prefix, varying values in `paths` order, trailing id)`, see [split_common].
type PathShape = (Vec<UNib32>, Vec<UNib32>, Option<UNib32>);

/// Finds the one path element where `paths` (same length, at least 2 entries) diverge: every one up to there must
/// agree (the common prefix, sent as the request's own path), and at most one more after it must also agree for
/// all of them (`in_each_array_id`). Identical paths vary at their last element, which repeats (`List([id, id])`).
/// Returns `(prefix, varying values in `paths` order, trailing id)`.
pub(crate) fn split_common(paths: &[Vec<UNib32>]) -> Result<PathShape, Error> {
    if paths.len() < 2 {
        return Err(Error::MultiReq("need at least 2 resources".into()));
    }
    let len = paths[0].len();
    if len == 0 {
        return Err(Error::MultiReq(
            "cannot multi-request the API root itself".into(),
        ));
    }
    for p in &paths[1..] {
        if p.len() != len {
            return Err(Error::MultiReq(
                "resources must be at the same level".into(),
            ));
        }
    }
    let mut diverges_at = None;
    'outer: for i in 0..len {
        for p in &paths[1..] {
            if p[i] != paths[0][i] {
                diverges_at = Some(i);
                break 'outer;
            }
        }
    }
    // the same resource more than once (a tuple calling one method twice, CLT-49): it repeats at its own last id
    let k = diverges_at.unwrap_or(len - 1);
    let prefix = paths[0][..k].to_vec();
    let varying: Vec<UNib32> = paths.iter().map(|p| p[k]).collect();
    let trailing = match len - k - 1 {
        0 => None,
        1 => {
            let id = paths[0][k + 1];
            for p in &paths[1..] {
                if p[k + 1] != id {
                    return Err(Error::MultiReq(
                        "resources diverge at more than one position".into(),
                    ));
                }
            }
            Some(id)
        }
        _ => {
            return Err(Error::MultiReq(
                "resources diverge too early: at most one more id may follow the differing one"
                    .into(),
            ));
        }
    };
    Ok((prefix, varying, trailing))
}

/// Picks `MultiArgs::Same` when every item's bytes are equal (smaller on the wire), `Different` otherwise.
pub(crate) fn merge_args(items: Vec<Vec<u8>>) -> Result<MultiArgsOwned, Error> {
    let same = items.windows(2).all(|w| w[0] == w[1]);
    if same {
        Ok(MultiArgsOwned::Same(
            items.into_iter().next().unwrap_or_default(),
        ))
    } else {
        Ok(MultiArgsOwned::Different(items.to_ww_bytes_owned()?))
    }
}

/// Sends an already built multi request and returns the raw response bytes (an `EitherAnyVec`).
pub(crate) async fn send_multi(
    commander: TransportCommander,
    prefix: Vec<UNib32>,
    kind: RequestKindOwned,
    timeout: Option<Duration>,
) -> Result<Vec<u8>, Error> {
    let done_rx = commander
        .send_multi_request(PathKindOwned::Absolute { path: prefix }, kind, timeout)
        .await?;
    done_rx.await.map_err(|_| Error::RxDispatcherNotRunning)?
}

pub(crate) fn send_multi_blocking(
    commander: TransportCommander,
    prefix: Vec<UNib32>,
    kind: RequestKindOwned,
    timeout: Option<Duration>,
) -> Result<Vec<u8>, Error> {
    let done_rx = commander.send_multi_request_blocking(
        PathKindOwned::Absolute { path: prefix },
        kind,
        timeout,
    )?;
    done_rx
        .blocking_recv()
        .map_err(|_| Error::RxDispatcherNotRunning)?
}

/// Decodes `count` items (in order) of an `EitherAnyVec` response into `Result<T, ErrorOwned>`.
pub(crate) fn decode_items<T: DeserializeShrinkWrapOwned>(
    bytes: &[u8],
    count: usize,
) -> Result<Vec<Result<T, ErrorOwned>>, Error> {
    let mut rd = BufReader::new(bytes);
    let vec = EitherAnyVec::des_shrink_wrap(&mut rd)?;
    let mut iter = vec.iter();
    (0..count)
        .map(|_| Ok(iter.next_owned::<ErrorOwned, T>()?.into()))
        .collect()
}

/// [CallPath::output] of every index.
fn call_outputs<C: CallPath>(call: &C, items: Vec<Result<C::Item, ErrorOwned>>) -> Vec<C::Output> {
    items.into_iter().map(|item| call.output(item)).collect()
}

/// `(commander, prefix, trailing id, per-index bytes, first index's builder)`, see [materialize].
type MaterializeResult<L> = (
    TransportCommander,
    Vec<UNib32>,
    Option<UNib32>,
    Vec<Vec<u8>>,
    L,
);

/// Materializes every index of `multi_idx` into (path, bytes, commander) triples via `leaf` (the existing
/// generated per-index accessor, e.g. `|i| client.pin(i).read_mode()`), locally only - no network traffic - then
/// finds the common path shape with [split_common]. `bytes` is `None` for a read. The first index's builder is
/// kept, it makes the answers ([CallPath::output]).
fn materialize<L>(
    multi_idx: &MultiIndexOwned,
    leaf: impl Fn(u32) -> L,
    path_of: impl Fn(&L) -> Option<Vec<UNib32>>,
    bytes_of: impl Fn(&L) -> Option<Vec<u8>>,
    commander_of: impl Fn(&L) -> TransportCommander,
) -> Result<MaterializeResult<L>, Error> {
    let mut paths = Vec::with_capacity(multi_idx.len());
    let mut bytes = Vec::with_capacity(multi_idx.len());
    let mut commander: Option<TransportCommander> = None;
    let mut first = None;
    for i in multi_idx.iter_u32() {
        let item = leaf(i);
        let path = path_of(&item).ok_or_else(|| Error::MultiReq("a path".into()))?;
        if let Some(b) = bytes_of(&item) {
            bytes.push(b);
        }
        let item_commander = commander_of(&item);
        match &mut commander {
            None => commander = Some(item_commander),
            Some(c) => c.priority = c.priority.max(item_commander.priority),
        }
        if first.is_none() {
            first = Some(item);
        }
        paths.push(path);
    }
    if paths.windows(2).all(|w| w[0] == w[1]) {
        // `multi_idx` is sent in place of the varying element, which must be the array index
        return Err(Error::MultiReq(
            "resources are identical, nothing to multi-request".into(),
        ));
    }
    let (prefix, _varying, trailing) = split_common(&paths)?;
    let commander = commander.expect("split_common already checked paths is non-empty");
    let first = first.expect("split_common already checked paths is non-empty");
    Ok((commander, prefix, trailing, bytes, first))
}

/// Read the same property/resource across an array (CLT-15): `multi_idx` is sent as given (`Range`/`List`/
/// `Mask32`), `get` is the existing generated per-index accessor, e.g. `|i| client.pin(i).read_mode()`, called once
/// per index locally (no network traffic) to find the common path shape. At least 2 indices.
pub async fn read_array<T, F>(
    multi_idx: MultiIndexOwned,
    get: F,
) -> Result<Vec<Result<T, ErrorOwned>>, Error>
where
    T: DeserializeShrinkWrapOwned,
    F: Fn(u32) -> crate::PreparedRead<T>,
{
    let count = multi_idx.len();
    let (commander, prefix, in_each_array_id, _, _) = materialize(
        &multi_idx,
        get,
        PropertyPath::absolute_path,
        |_| None,
        PropertyPath::commander,
    )?;
    let bytes = send_multi(
        commander,
        prefix,
        RequestKindOwned::MultiRead {
            multi_idx,
            in_each_array_id,
        },
        None,
    )
    .await?;
    decode_items(&bytes, count)
}

/// Blocking variant of [read_array].
pub fn read_array_blocking<T, F>(
    multi_idx: MultiIndexOwned,
    get: F,
) -> Result<Vec<Result<T, ErrorOwned>>, Error>
where
    T: DeserializeShrinkWrapOwned,
    F: Fn(u32) -> crate::PreparedRead<T>,
{
    let count = multi_idx.len();
    let (commander, prefix, in_each_array_id, _, _) = materialize(
        &multi_idx,
        get,
        PropertyPath::absolute_path,
        |_| None,
        PropertyPath::commander,
    )?;
    let bytes = send_multi_blocking(
        commander,
        prefix,
        RequestKindOwned::MultiRead {
            multi_idx,
            in_each_array_id,
        },
        None,
    )?;
    decode_items(&bytes, count)
}

/// Call the same method across an array (CLT-15): see [read_array]. Each index answers `Result<T, ErrorOwned>`, or
/// for a method returning `Result<T, D>` the same `Result<T, CallError<D>>` as its single call (CLT-48).
pub async fn call_array<C, F>(multi_idx: MultiIndexOwned, build: F) -> Result<Vec<C::Output>, Error>
where
    C: CallPath,
    F: Fn(u32) -> C,
{
    let count = multi_idx.len();
    let (commander, prefix, in_each_array_id, bytes, first) = materialize(
        &multi_idx,
        build,
        CallPath::absolute_path,
        |l| Some(CallPath::args_bytes(l)),
        CallPath::commander,
    )?;
    let multi_args = merge_args(bytes)?;
    let response = send_multi(
        commander,
        prefix,
        RequestKindOwned::MultiCall {
            multi_idx,
            in_each_array_id,
            multi_args,
        },
        None,
    )
    .await?;
    Ok(call_outputs(&first, decode_items(&response, count)?))
}

/// Blocking variant of [call_array].
pub fn call_array_blocking<C, F>(
    multi_idx: MultiIndexOwned,
    build: F,
) -> Result<Vec<C::Output>, Error>
where
    C: CallPath,
    F: Fn(u32) -> C,
{
    let count = multi_idx.len();
    let (commander, prefix, in_each_array_id, bytes, first) = materialize(
        &multi_idx,
        build,
        CallPath::absolute_path,
        |l| Some(CallPath::args_bytes(l)),
        CallPath::commander,
    )?;
    let multi_args = merge_args(bytes)?;
    let response = send_multi_blocking(
        commander,
        prefix,
        RequestKindOwned::MultiCall {
            multi_idx,
            in_each_array_id,
            multi_args,
        },
        None,
    )?;
    Ok(call_outputs(&first, decode_items(&response, count)?))
}

/// Write the same property/resource across an array (CLT-15): see [read_array]. Answers `()` per index.
pub async fn write_array<F>(
    multi_idx: MultiIndexOwned,
    build: F,
) -> Result<Vec<Result<(), ErrorOwned>>, Error>
where
    F: Fn(u32) -> crate::PreparedWrite,
{
    let count = multi_idx.len();
    let (commander, prefix, in_each_array_id, bytes, _) = materialize(
        &multi_idx,
        build,
        WritePath::absolute_path,
        |l| Some(WritePath::value_bytes(l)),
        WritePath::commander,
    )?;
    let multi_data = merge_args(bytes)?;
    let response = send_multi(
        commander,
        prefix,
        RequestKindOwned::MultiWrite {
            multi_idx,
            in_each_array_id,
            multi_data,
        },
        None,
    )
    .await?;
    decode_items(&response, count)
}

/// Blocking variant of [write_array].
pub fn write_array_blocking<F>(
    multi_idx: MultiIndexOwned,
    build: F,
) -> Result<Vec<Result<(), ErrorOwned>>, Error>
where
    F: Fn(u32) -> crate::PreparedWrite,
{
    let count = multi_idx.len();
    let (commander, prefix, in_each_array_id, bytes, _) = materialize(
        &multi_idx,
        build,
        WritePath::absolute_path,
        |l| Some(WritePath::value_bytes(l)),
        WritePath::commander,
    )?;
    let multi_data = merge_args(bytes)?;
    let response = send_multi_blocking(
        commander,
        prefix,
        RequestKindOwned::MultiWrite {
            multi_idx,
            in_each_array_id,
            multi_data,
        },
        None,
    )?;
    decode_items(&response, count)
}

/// Several resources read in one request (CLT-15): implemented for tuples of 2 to 8 of
/// [PreparedRead](crate::PreparedRead), e.g. `(client.read_x(), client.read_y()).multi_read().await`. Any depth,
/// as long as every path shares a common prefix and diverges at one position, optionally followed by one more
/// shared id (see `split_common`, private).
pub trait MultiRead<Out> {
    fn multi_read(self) -> impl Future<Output = Result<Out, Error>>;
}

/// Several methods called in one request (CLT-15): implemented for tuples of 2 to 8 of
/// [PreparedCall](crate::PreparedCall), e.g. `(client.inc(), client.dec()).multi_call().await`. A method without
/// `Result` answers `Result<T, ErrorOwned>` in its position, a method returning `Result<T, D>` the same
/// `Result<T, CallError<D>>` as its single call (CLT-48); both kinds mix in one tuple.
pub trait MultiCall<Out> {
    fn multi_call(self) -> impl Future<Output = Result<Out, Error>>;
}

/// Several properties or resources written in one request (CLT-15): implemented for tuples of 2 to 8 of
/// [PreparedWrite](crate::PreparedWrite), e.g. `(client.write_x(1), client.write_y(2)).multi_write().await`.
pub trait MultiWrite<Out> {
    fn multi_write(self) -> impl Future<Output = Result<Out, Error>>;
}

/// Expands to `()`: used only so a repeated `$T` appears in the `MultiWrite` output tuple type, which must repeat
/// exactly once per tuple arity even though every position answers `()`.
macro_rules! unit_of {
    ($T:ident) => {
        ()
    };
}

macro_rules! impl_multi_tuple {
    ($($T:ident),+) => {
        impl<$($T),+> MultiRead<($(Result<$T::Output, ErrorOwned>,)+)> for ($($T,)+)
        where
            $($T: PropertyPath,)+
        {
            async fn multi_read(self) -> Result<($(Result<$T::Output, ErrorOwned>,)+), Error> {
                #[allow(non_snake_case)]
                let ($(ref $T,)+) = self;
                let paths = [$(PropertyPath::absolute_path($T).ok_or_else(|| Error::MultiReq("a path".into()))?),+];
                let (prefix, varying, trailing) = split_common(&paths)?;
                let mut commander = PropertyPath::commander(&self.0);
                // the multi request goes at the highest priority one of its items asked for
                commander.priority = [$(PropertyPath::commander($T).priority),+].into_iter().max().unwrap_or(commander.priority);
                let bytes = send_multi(
                    commander,
                    prefix,
                    RequestKindOwned::MultiRead {
                        multi_idx: MultiIndexOwned::List(varying),
                        in_each_array_id: trailing,
                    },
                    None,
                ).await?;
                let mut rd = BufReader::new(&bytes);
                let vec = EitherAnyVec::des_shrink_wrap(&mut rd)?;
                let mut iter = vec.iter();
                Ok(($(iter.next_owned::<ErrorOwned, $T::Output>()?.into(),)+))
            }
        }

        impl<$($T),+> MultiCall<($($T::Output,)+)> for ($($T,)+)
        where
            $($T: CallPath,)+
        {
            async fn multi_call(self) -> Result<($($T::Output,)+), Error> {
                #[allow(non_snake_case)]
                let ($(ref $T,)+) = self;
                let paths = [$(CallPath::absolute_path($T).ok_or_else(|| Error::MultiReq("a path".into()))?),+];
                let (prefix, varying, trailing) = split_common(&paths)?;
                let mut commander = CallPath::commander(&self.0);
                // the multi request goes at the highest priority one of its items asked for
                commander.priority = [$(CallPath::commander($T).priority),+].into_iter().max().unwrap_or(commander.priority);
                let multi_args = merge_args(vec![$(CallPath::args_bytes($T)),+])?;
                let bytes = send_multi(
                    commander,
                    prefix,
                    RequestKindOwned::MultiCall {
                        multi_idx: MultiIndexOwned::List(varying),
                        in_each_array_id: trailing,
                        multi_args,
                    },
                    None,
                ).await?;
                let mut rd = BufReader::new(&bytes);
                let vec = EitherAnyVec::des_shrink_wrap(&mut rd)?;
                let mut iter = vec.iter();
                Ok(($(CallPath::output($T, iter.next_owned::<ErrorOwned, $T::Item>()?.into()),)+))
            }
        }

        impl<$($T),+> MultiWrite<($(Result<unit_of!($T), ErrorOwned>,)+)> for ($($T,)+)
        where
            $($T: WritePath,)+
        {
            async fn multi_write(self) -> Result<($(Result<unit_of!($T), ErrorOwned>,)+), Error> {
                #[allow(non_snake_case)]
                let ($(ref $T,)+) = self;
                let paths = [$(WritePath::absolute_path($T).ok_or_else(|| Error::MultiReq("a path".into()))?),+];
                let (prefix, varying, trailing) = split_common(&paths)?;
                let mut commander = WritePath::commander(&self.0);
                // the multi request goes at the highest priority one of its items asked for
                commander.priority = [$(WritePath::commander($T).priority),+].into_iter().max().unwrap_or(commander.priority);
                let multi_data = merge_args(vec![$(WritePath::value_bytes($T)),+])?;
                let bytes = send_multi(
                    commander,
                    prefix,
                    RequestKindOwned::MultiWrite {
                        multi_idx: MultiIndexOwned::List(varying),
                        in_each_array_id: trailing,
                        multi_data,
                    },
                    None,
                ).await?;
                let mut rd = BufReader::new(&bytes);
                let vec = EitherAnyVec::des_shrink_wrap(&mut rd)?;
                let mut iter = vec.iter();
                Ok(($({ let _ = &$T; iter.next_owned::<ErrorOwned, ()>()?.into() },)+))
            }
        }
    };
}

impl_multi_tuple!(A, B);
impl_multi_tuple!(A, B, C);
impl_multi_tuple!(A, B, C, D);
impl_multi_tuple!(A, B, C, D, E);
impl_multi_tuple!(A, B, C, D, E, F);
impl_multi_tuple!(A, B, C, D, E, F, G);
impl_multi_tuple!(A, B, C, D, E, F, G, H);
