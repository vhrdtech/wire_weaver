//! Client API for multi requests (`MultiRead`, `MultiCall`, `MultiWrite`, CLT-15): several resources answered in
//! one request/response pair, each on its own (`Either<ErrorOwned, value>` per index, in index order).
//!
//! Two shapes are possible, both reduced to the same wire shape by [split_common]: a request path (the common
//! prefix), a `multi_idx` (the one path element that differs between indices) and an optional trailing
//! `in_each_array_id` (one more path element, the same for every index).
//! - **Different resources at one level**: [MultiRead], [MultiCall], [MultiWrite] are implemented for tuples of
//!   2 to 8 of the generated client's `PreparedRead`/`PreparedCall`/`PreparedWrite` builders, e.g.
//!   `(client.read_x(), client.read_y(), client.read_mode()).multi_read().await`.
//! - **The same resource across an array**: [read_array], [call_array] and [write_array] take a `MultiIndexOwned`
//!   selector (`Range`/`List`/`Mask32`, sent as given - not rebuilt as a `List`) and a closure that is just the
//!   existing generated per-index accessor, e.g. `read_array(MultiIndexOwned::Range(UNib32(0)..UNib32(4)), |i|
//!   client.pin(i).read_mode())`.
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
    type Output: DeserializeShrinkWrapOwned;

    fn absolute_path(&self) -> Option<Vec<UNib32>>;
    fn args_bytes(&self) -> Vec<u8>;
    fn commander(&self) -> TransportCommander;
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
/// all of them (`in_each_array_id`). Returns `(prefix, varying values in `paths` order, trailing id)`.
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
    let Some(k) = diverges_at else {
        return Err(Error::MultiReq(
            "resources are identical, nothing to multi-request".into(),
        ));
    };
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

/// `(commander, prefix, trailing id, per-index bytes)`, see [materialize].
type MaterializeResult = (
    TransportCommander,
    Vec<UNib32>,
    Option<UNib32>,
    Vec<Vec<u8>>,
);

/// Materializes every index of `multi_idx` into (path, bytes, commander) triples via `leaf` (the existing
/// generated per-index accessor, e.g. `|i| client.pin(i).read_mode()`), locally only - no network traffic - then
/// finds the common path shape with [split_common]. `bytes` is `None` for a read.
fn materialize<L>(
    multi_idx: &MultiIndexOwned,
    leaf: impl Fn(u32) -> L,
    path_of: impl Fn(&L) -> Option<Vec<UNib32>>,
    bytes_of: impl Fn(&L) -> Option<Vec<u8>>,
    commander_of: impl Fn(&L) -> TransportCommander,
) -> Result<MaterializeResult, Error> {
    let mut paths = Vec::with_capacity(multi_idx.len());
    let mut bytes = Vec::with_capacity(multi_idx.len());
    let mut commander = None;
    for i in multi_idx.iter_u32() {
        let item = leaf(i);
        let path = path_of(&item).ok_or_else(|| Error::MultiReq("a path".into()))?;
        if let Some(b) = bytes_of(&item) {
            bytes.push(b);
        }
        if commander.is_none() {
            commander = Some(commander_of(&item));
        }
        paths.push(path);
    }
    let (prefix, _varying, trailing) = split_common(&paths)?;
    let commander = commander.expect("split_common already checked paths is non-empty");
    Ok((commander, prefix, trailing, bytes))
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
    let (commander, prefix, in_each_array_id, _) = materialize(
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
    let (commander, prefix, in_each_array_id, _) = materialize(
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

/// Call the same method across an array (CLT-15): see [read_array].
pub async fn call_array<T, F>(
    multi_idx: MultiIndexOwned,
    build: F,
) -> Result<Vec<Result<T, ErrorOwned>>, Error>
where
    T: DeserializeShrinkWrapOwned,
    F: Fn(u32) -> crate::PreparedCall<T>,
{
    let count = multi_idx.len();
    let (commander, prefix, in_each_array_id, bytes) = materialize(
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
    decode_items(&response, count)
}

/// Blocking variant of [call_array].
pub fn call_array_blocking<T, F>(
    multi_idx: MultiIndexOwned,
    build: F,
) -> Result<Vec<Result<T, ErrorOwned>>, Error>
where
    T: DeserializeShrinkWrapOwned,
    F: Fn(u32) -> crate::PreparedCall<T>,
{
    let count = multi_idx.len();
    let (commander, prefix, in_each_array_id, bytes) = materialize(
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
    decode_items(&response, count)
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
    let (commander, prefix, in_each_array_id, bytes) = materialize(
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
    let (commander, prefix, in_each_array_id, bytes) = materialize(
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
/// shared id (see [split_common]).
pub trait MultiRead<Out> {
    fn multi_read(self) -> impl Future<Output = Result<Out, Error>>;
}

/// Several methods called in one request (CLT-15): implemented for tuples of 2 to 8 of
/// [PreparedCall](crate::PreparedCall), e.g. `(client.inc(), client.dec()).multi_call().await`.
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
                let commander = PropertyPath::commander(&self.0);
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

        impl<$($T),+> MultiCall<($(Result<$T::Output, ErrorOwned>,)+)> for ($($T,)+)
        where
            $($T: CallPath,)+
        {
            async fn multi_call(self) -> Result<($(Result<$T::Output, ErrorOwned>,)+), Error> {
                #[allow(non_snake_case)]
                let ($(ref $T,)+) = self;
                let paths = [$(CallPath::absolute_path($T).ok_or_else(|| Error::MultiReq("a path".into()))?),+];
                let (prefix, varying, trailing) = split_common(&paths)?;
                let commander = CallPath::commander(&self.0);
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
                Ok(($(iter.next_owned::<ErrorOwned, $T::Output>()?.into(),)+))
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
                let commander = WritePath::commander(&self.0);
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
