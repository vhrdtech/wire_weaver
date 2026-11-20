//! Keyed resource arrays (CLT-72): a client addresses an element of `flow[]` by its key, through the array's
//! `keyed!(flow by FlowKey)` companion, whose level (`index_of`, `key[]`, `keys`) is [ww_self::keys].
//!
//! A [Keyed] handle asks the device for an index once per key and keeps the answer in a bounded cache per keyed
//! array, shared by a [Commander] and its clones. The cache learns from `index_of` answers, key reads and the `keys`
//! stream ([KeyStream]); a `Removed` event, a `BadIndex` answer to a request under the element, or a reconnect drops
//! entries. Keys are compared as their shrink_wrap bytes, so generated and dynamic clients share entries.
//!
//! A device that gives a removed element's index to another key while the host doesn't follow the `keys` stream can
//! still leave a stale entry until a request under it is refused with `BadIndex`: a device that never reuses an
//! index within a session closes that gap.

use crate::locked::Locked;
use crate::{Commander, Error, Stream, StreamError};
use std::collections::{HashMap, VecDeque};
use std::marker::PhantomData;
use std::sync::{Arc, Mutex};
use wire_weaver::prelude::{DeserializeShrinkWrapOwned, UNib32};
use wire_weaver::shrink_wrap::SerializeShrinkWrapOwned;
use wire_weaver::shrink_wrap::tail_bytes::TailBytesOwned;
use ww_client_server::{ErrorKindOwned, PathKind, PathKindOwned, ResourceIndex};
use ww_keyed::KeyEvent;
use ww_self::keys::{INDEX_OF, KEY, KEYS};

/// Most keys a keyed array's cache holds; learning one more forgets the one learned first.
pub const KEY_CACHE_ENTRIES_MAX: usize = 1024;

/// Keys and indices of one keyed array the client learned.
#[derive(Default)]
struct KeyCache {
    by_key: HashMap<Vec<u8>, u32>,
    by_index: HashMap<u32, Vec<u8>>,
    /// Indices in the order they were learned, the first is forgotten first.
    order: VecDeque<u32>,
}

impl KeyCache {
    fn learn(&mut self, index: u32, key: Vec<u8>) {
        self.forget(index);
        if let Some(old) = self.by_key.remove(&key) {
            self.forget(old);
        }
        while self.by_index.len() >= KEY_CACHE_ENTRIES_MAX {
            let Some(oldest) = self.order.front().copied() else {
                break;
            };
            self.forget(oldest);
        }
        self.by_key.insert(key.clone(), index);
        self.by_index.insert(index, key);
        self.order.push_back(index);
    }

    fn forget(&mut self, index: u32) {
        if let Some(key) = self.by_index.remove(&index) {
            self.by_key.remove(&key);
            self.order.retain(|&i| i != index);
        }
    }
}

/// Path of a keyed array as requests reach it: the form of the path (absolute or through a trait's global id) and
/// the ids and indices to the array item.
#[derive(Clone, Debug, PartialEq)]
struct ArrayPath {
    /// The path kind with an empty path, serialized: equal for paths of one form and trait.
    head: Vec<u8>,
    path: Vec<u32>,
}

impl ArrayPath {
    fn new(path_kind: &PathKindOwned) -> Result<Self, Error> {
        let (path, head) = split(path_kind);
        Ok(ArrayPath {
            head: head.to_ww_bytes_owned()?,
            path,
        })
    }

    /// Index of the element `path_kind` is under, when it is under this array.
    fn element_of(&self, path_kind: &PathKindOwned) -> Option<u32> {
        let (path, head) = split(path_kind);
        let index = *path.get(self.path.len())?;
        (path.starts_with(&self.path) && head.to_ww_bytes_owned().ok()? == self.head)
            .then_some(index)
    }
}

/// Ids and indices of a path, and the path kind without them.
fn split(path_kind: &PathKindOwned) -> (Vec<u32>, PathKindOwned) {
    let mut head = path_kind.clone();
    let path = match &mut head {
        PathKindOwned::Absolute { path } => std::mem::take(path),
        PathKindOwned::GlobalCompact {
            path_from_trait, ..
        }
        | PathKindOwned::GlobalFull {
            path_from_trait, ..
        } => std::mem::take(path_from_trait),
    };
    (path.iter().map(|i| i.get().0).collect(), head)
}

/// Key caches of one connection, one per keyed array, shared by a [Commander], its clones and the requests they
/// send.
#[derive(Clone, Default)]
pub(crate) struct KeyCaches(Arc<Mutex<Vec<(ArrayPath, KeyCache)>>>);

impl KeyCaches {
    fn with<R>(&self, array: &ArrayPath, f: impl FnOnce(&mut KeyCache) -> R) -> R {
        let mut caches = self.0.locked();
        let at = match caches.iter().position(|(path, _)| path == array) {
            Some(at) => at,
            None => {
                caches.push((array.clone(), KeyCache::default()));
                caches.len() - 1
            }
        };
        f(&mut caches[at].1)
    }

    /// The keyed arrays and indices a request to `path_kind` is under, to forget them if the device answers
    /// `BadIndex`; empty without keyed arrays.
    pub(crate) fn elements(&self, path_kind: &PathKindOwned) -> Vec<(usize, u32)> {
        let caches = self.0.locked();
        if caches.is_empty() {
            return vec![];
        }
        caches
            .iter()
            .enumerate()
            .filter_map(|(at, (array, _))| Some((at, array.element_of(path_kind)?)))
            .collect()
    }

    /// Forgets `elements` (from [Self::elements]) when `result` is a `BadIndex` answer.
    pub(crate) fn answered<T>(&self, elements: &[(usize, u32)], result: &Result<T, Error>) {
        if elements.is_empty() || !is_bad_index(result) {
            return;
        }
        let mut caches = self.0.locked();
        for &(at, index) in elements {
            if let Some((_, cache)) = caches.get_mut(at) {
                cache.forget(index);
            }
        }
    }

    /// Forgets every key, after a reconnect: the device may give its elements other indices.
    pub(crate) fn clear(&self) {
        self.0.locked().clear();
    }
}

/// The cache of one keyed array, for what follows announcements and reads away from a [Keyed] (a [KeyStream], the
/// dynamic client's paths through the companion).
#[derive(Clone)]
pub(crate) struct KeyNotes {
    caches: KeyCaches,
    array: ArrayPath,
}

impl KeyNotes {
    pub(crate) fn learn(&self, index: u32, key: Vec<u8>) {
        self.caches
            .with(&self.array, |cache| cache.learn(index, key));
    }

    pub(crate) fn forget(&self, index: u32) {
        self.caches.with(&self.array, |cache| cache.forget(index));
    }

    fn index(&self, key: &[u8]) -> Option<u32> {
        self.caches
            .with(&self.array, |cache| cache.by_key.get(key).copied())
    }

    fn key(&self, index: u32) -> Option<Vec<u8>> {
        self.caches
            .with(&self.array, |cache| cache.by_index.get(&index).cloned())
    }

    /// Follows an announcement of the `keys` stream.
    fn note<I: Clone + Into<u32>, K: SerializeShrinkWrapOwned>(&self, event: &KeyEvent<I, K>) {
        match event {
            KeyEvent::Added { index, key } => {
                // a key the host can't serialize again can't be looked up either
                if let Ok(bytes) = key.to_ww_bytes_owned() {
                    self.learn(index.clone().into(), bytes);
                }
            }
            KeyEvent::Removed { index } => self.forget(index.clone().into()),
            // an announcement of a newer device: nothing the cache can follow
            KeyEvent::Unknown(_) => {}
        }
    }

    /// Follows a read of `key[index]`: learns the key, or forgets the index the device refused.
    pub(crate) fn key_read<T: std::ops::Deref<Target = [u8]>>(
        &self,
        index: u32,
        result: &Result<T, Error>,
    ) {
        match result {
            Ok(key) => self.learn(index, key.to_vec()),
            Err(_) if is_bad_index(result) => self.forget(index),
            Err(_) => {}
        }
    }
}

fn is_bad_index<T>(result: &Result<T, Error>) -> bool {
    matches!(result, Err(Error::RemoteError(e)) if matches!(e.kind, ErrorKindOwned::BadIndex))
}

/// A keyed array of the connected device, reached through its `keyed!` companion: generated `<array>_by`,
/// `<array>_key` and `<array>_keys` use it, so does the dynamic client.
pub struct Keyed<'c> {
    cmd: &'c Commander,
    /// Path of the companion as the client names it, before the Commander resolves it.
    keys: PathKindOwned,
    /// The array's cache, by its resolved path. `None` when the path can't be resolved, then the requests fail with
    /// the reason.
    notes: Option<KeyNotes>,
}

impl Commander {
    /// The keyed array whose `keyed!` companion is at `keys`, `of` the array's id in the same level.
    pub fn keyed(&self, keys: PathKind<'_>, of: u32) -> Result<Keyed<'_>, Error> {
        let keys = to_owned(keys)?;
        let mut array = keys.clone();
        let path = path_mut(&mut array);
        path.pop();
        path.push(ResourceIndex::new(UNib32(of)));
        let array = self
            .to_ww_client_server_path(as_ref(&array))
            .and_then(|(array, _)| ArrayPath::new(&array))
            .ok();
        Ok(Keyed {
            cmd: self,
            keys,
            notes: array.map(|array| KeyNotes {
                caches: self.keys.clone(),
                array,
            }),
        })
    }
}

impl Keyed<'_> {
    pub(crate) fn notes(&self) -> Option<&KeyNotes> {
        self.notes.as_ref()
    }

    /// Path of an item of the companion's level, and of an index under it.
    fn sub(&self, tail: &[u32]) -> PathKindOwned {
        let mut path = self.keys.clone();
        path_mut(&mut path).extend(tail.iter().map(|&i| ResourceIndex::new(UNib32(i))));
        path
    }

    /// Index of the element with `key` (its shrink_wrap bytes) the client learned, without asking the device.
    pub fn cached_index(&self, key: &[u8]) -> Option<u32> {
        self.notes()?.index(key)
    }

    /// Key (shrink_wrap bytes) of the element at `index` the client learned, without asking the device.
    pub fn cached_key(&self, index: u32) -> Option<Vec<u8>> {
        self.notes()?.key(index)
    }

    /// Notes that the element at `index` has `key` (its shrink_wrap bytes).
    pub fn learn(&self, index: u32, key: Vec<u8>) {
        if let Some(notes) = self.notes() {
            notes.learn(index, key);
        }
    }

    /// Notes that the element at `index` is gone.
    pub fn forget(&self, index: u32) {
        if let Some(notes) = self.notes() {
            notes.forget(index);
        }
    }

    /// Notes what an announcement on the `keys` stream says.
    pub fn note<I: Clone + Into<u32>, K: SerializeShrinkWrapOwned>(&self, event: &KeyEvent<I, K>) {
        if let Some(notes) = self.notes() {
            notes.note(event);
        }
    }

    /// The `index_of` request: from the cache when the key is in it, else asked of the device. `key` is the key's
    /// shrink_wrap bytes, `args` the arguments of `index_of`, `decode` reads its answer (`Option<I>`) as an index.
    pub async fn index_of_with(
        &self,
        key: Vec<u8>,
        args: Result<Vec<u8>, Error>,
        decode: impl FnOnce(&[u8]) -> Result<Option<u32>, Error>,
    ) -> Result<Option<u32>, Error> {
        if let Some(index) = self.cached_index(&key) {
            return Ok(Some(index));
        }
        let answer = self
            .cmd
            .prepare_call::<TailBytesOwned, Error>(
                as_ref(&self.sub(&[INDEX_OF])),
                args,
                "Keys::index_of",
            )
            .call()
            .await?;
        self.found(key, decode(&answer)?)
    }

    /// Blocking variant of [Self::index_of_with].
    pub fn index_of_with_blocking(
        &self,
        key: Vec<u8>,
        args: Result<Vec<u8>, Error>,
        decode: impl FnOnce(&[u8]) -> Result<Option<u32>, Error>,
    ) -> Result<Option<u32>, Error> {
        if let Some(index) = self.cached_index(&key) {
            return Ok(Some(index));
        }
        let answer = self
            .cmd
            .prepare_call::<TailBytesOwned, Error>(
                as_ref(&self.sub(&[INDEX_OF])),
                args,
                "Keys::index_of",
            )
            .blocking_call()?;
        self.found(key, decode(&answer)?)
    }

    fn found(&self, key: Vec<u8>, index: Option<u32>) -> Result<Option<u32>, Error> {
        if let Some(index) = index {
            self.learn(index, key);
        }
        Ok(index)
    }

    /// Index of the element with `key`: asked of the device once, then taken from the cache. `I` is the array's
    /// index type, `args` the arguments of `index_of`.
    pub async fn index_of<K, I>(
        &self,
        key: &K,
        args: Result<Vec<u8>, Error>,
    ) -> Result<Option<u32>, Error>
    where
        K: SerializeShrinkWrapOwned,
        I: DeserializeShrinkWrapOwned + Into<u32>,
    {
        self.index_of_with(key.to_ww_bytes_owned()?, args, decode_index::<I>)
            .await
    }

    /// Blocking variant of [Self::index_of].
    pub fn index_of_blocking<K, I>(
        &self,
        key: &K,
        args: Result<Vec<u8>, Error>,
    ) -> Result<Option<u32>, Error>
    where
        K: SerializeShrinkWrapOwned,
        I: DeserializeShrinkWrapOwned + Into<u32>,
    {
        self.index_of_with_blocking(key.to_ww_bytes_owned()?, args, decode_index::<I>)
    }

    /// Key bytes of the element at `index`, read from the device: the cache learns them, a `BadIndex` answer
    /// forgets the index.
    pub async fn key_bytes(&self, index: u32) -> Result<Vec<u8>, Error> {
        let result = self
            .cmd
            .prepare_read::<TailBytesOwned>(as_ref(&self.sub(&[KEY, index])))
            .read()
            .await;
        self.key_read(index, result)
    }

    /// Blocking variant of [Self::key_bytes].
    pub fn key_bytes_blocking(&self, index: u32) -> Result<Vec<u8>, Error> {
        let result = self
            .cmd
            .prepare_read::<TailBytesOwned>(as_ref(&self.sub(&[KEY, index])))
            .blocking_read();
        self.key_read(index, result)
    }

    fn key_read(
        &self,
        index: u32,
        result: Result<TailBytesOwned, Error>,
    ) -> Result<Vec<u8>, Error> {
        if let Some(notes) = self.notes() {
            notes.key_read(index, &result);
        }
        Ok(result?.0)
    }

    /// Key of the element at `index`, read from the device, see [Self::key_bytes].
    pub async fn key<K: DeserializeShrinkWrapOwned>(&self, index: u32) -> Result<K, Error> {
        Ok(K::from_ww_bytes_owned(&self.key_bytes(index).await?)?)
    }

    /// Blocking variant of [Self::key].
    pub fn key_blocking<K: DeserializeShrinkWrapOwned>(&self, index: u32) -> Result<K, Error> {
        Ok(K::from_ww_bytes_owned(&self.key_bytes_blocking(index)?)?)
    }

    /// Path of the `keys` stream, for a client that opens it itself (the dynamic client).
    pub fn keys_path(&self) -> PathKindOwned {
        self.sub(&[KEYS])
    }

    /// Opens the `keys` stream: elements as they appear and go away, the cache follows them.
    pub async fn keys<I, K>(&self) -> Result<KeyStream<I, K>, Error>
    where
        I: DeserializeShrinkWrapOwned + SerializeShrinkWrapOwned + Clone + Into<u32>,
        K: DeserializeShrinkWrapOwned + SerializeShrinkWrapOwned,
    {
        let stream = self
            .cmd
            .prepare_stream_open::<(), Stream<KeyEvent<I, K>>>(
                as_ref(&self.keys_path()),
                None,
                Ok(vec![]),
            )
            .call()
            .await?;
        Ok(self.key_stream(stream))
    }

    /// Blocking variant of [Self::keys].
    pub fn keys_blocking<I, K>(&self) -> Result<KeyStream<I, K>, Error>
    where
        I: DeserializeShrinkWrapOwned + SerializeShrinkWrapOwned + Clone + Into<u32>,
        K: DeserializeShrinkWrapOwned + SerializeShrinkWrapOwned,
    {
        let stream = self
            .cmd
            .prepare_stream_open::<(), Stream<KeyEvent<I, K>>>(
                as_ref(&self.keys_path()),
                None,
                Ok(vec![]),
            )
            .blocking_call()?;
        Ok(self.key_stream(stream))
    }

    fn key_stream<I, K>(&self, stream: Stream<KeyEvent<I, K>>) -> KeyStream<I, K> {
        KeyStream {
            stream,
            notes: self.notes.clone(),
            _phantom: PhantomData,
        }
    }
}

fn decode_index<I: DeserializeShrinkWrapOwned + Into<u32>>(
    bytes: &[u8],
) -> Result<Option<u32>, Error> {
    Ok(Option::<I>::from_ww_bytes_owned(bytes)?.map(Into::into))
}

/// The `keys` stream of a keyed array: what the device announces, which the connection's key cache follows.
pub struct KeyStream<I, K> {
    stream: Stream<KeyEvent<I, K>>,
    notes: Option<KeyNotes>,
    _phantom: PhantomData<(I, K)>,
}

impl<I, K> KeyStream<I, K>
where
    I: DeserializeShrinkWrapOwned + SerializeShrinkWrapOwned + Clone + Into<u32>,
    K: DeserializeShrinkWrapOwned + SerializeShrinkWrapOwned,
{
    /// The next announcement.
    pub async fn recv(&mut self) -> Result<KeyEvent<I, K>, StreamError> {
        let event = self.stream.recv().await?;
        self.note(&event);
        Ok(event)
    }

    /// Blocking variant of [Self::recv].
    pub fn recv_blocking(&mut self) -> Result<KeyEvent<I, K>, StreamError> {
        let event = self.stream.recv_blocking()?;
        self.note(&event);
        Ok(event)
    }

    /// An announcement already received, `None` when there is none yet.
    pub fn try_recv(&mut self) -> Result<Option<KeyEvent<I, K>>, StreamError> {
        let event = self.stream.try_recv()?;
        if let Some(event) = &event {
            self.note(event);
        }
        Ok(event)
    }

    /// Closes the stream on the device.
    pub async fn close(&self) -> Result<(), StreamError> {
        self.stream.close().await
    }

    /// Blocking variant of [Self::close].
    pub fn close_blocking(&self) -> Result<(), StreamError> {
        self.stream.close_blocking()
    }

    fn note(&self, event: &KeyEvent<I, K>) {
        if let Some(notes) = &self.notes {
            notes.note(event);
        }
    }
}

fn path_mut(path_kind: &mut PathKindOwned) -> &mut Vec<ResourceIndex> {
    match path_kind {
        PathKindOwned::Absolute { path } => path,
        PathKindOwned::GlobalCompact {
            path_from_trait, ..
        }
        | PathKindOwned::GlobalFull {
            path_from_trait, ..
        } => path_from_trait,
    }
}

fn to_owned(path: PathKind<'_>) -> Result<PathKindOwned, Error> {
    let collect = |p: wire_weaver::shrink_wrap::RefVec<'_, ResourceIndex>| {
        p.iter().collect::<Result<Vec<_>, _>>()
    };
    Ok(match path {
        PathKind::Absolute { path } => PathKindOwned::Absolute {
            path: collect(path)?,
        },
        PathKind::GlobalCompact {
            gid,
            path_from_trait,
        } => PathKindOwned::GlobalCompact {
            gid,
            path_from_trait: collect(path_from_trait)?,
        },
        PathKind::GlobalFull {
            gid,
            trait_name,
            path_from_trait,
        } => PathKindOwned::GlobalFull {
            gid: gid.make_owned(),
            trait_name: trait_name.to_owned(),
            path_from_trait: collect(path_from_trait)?,
        },
    })
}

fn as_ref(path: &PathKindOwned) -> PathKind<'_> {
    use wire_weaver::shrink_wrap::RefVec;
    match path {
        PathKindOwned::Absolute { path } => PathKind::Absolute {
            path: RefVec::Slice { slice: path },
        },
        PathKindOwned::GlobalCompact {
            gid,
            path_from_trait,
        } => PathKind::GlobalCompact {
            gid: *gid,
            path_from_trait: RefVec::Slice {
                slice: path_from_trait,
            },
        },
        PathKindOwned::GlobalFull {
            gid,
            trait_name,
            path_from_trait,
        } => PathKind::GlobalFull {
            gid: gid.as_ref(),
            trait_name,
            path_from_trait: RefVec::Slice {
                slice: path_from_trait,
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn learning_a_key_again_moves_it() {
        let mut cache = KeyCache::default();
        cache.learn(3, vec![1]);
        cache.learn(4, vec![1]);
        assert_eq!(cache.by_key.get(&vec![1]), Some(&4));
        assert!(!cache.by_index.contains_key(&3));
        cache.learn(4, vec![2]);
        assert!(!cache.by_key.contains_key(&vec![1]));
        assert_eq!(cache.order, [4]);
    }

    #[test]
    fn cache_is_bounded_and_forgets_the_oldest() {
        let mut cache = KeyCache::default();
        for i in 0..KEY_CACHE_ENTRIES_MAX as u32 + 10 {
            cache.learn(i, i.to_le_bytes().to_vec());
        }
        assert_eq!(cache.by_index.len(), KEY_CACHE_ENTRIES_MAX);
        assert_eq!(cache.order.len(), KEY_CACHE_ENTRIES_MAX);
        assert!(!cache.by_index.contains_key(&9));
        assert!(cache.by_index.contains_key(&10));
    }

    #[test]
    fn element_of_matches_the_array_prefix_only() {
        let path = |ids: &[u32]| PathKindOwned::Absolute {
            path: ids.iter().map(|&i| ResourceIndex::new(UNib32(i))).collect(),
        };
        let array = ArrayPath::new(&path(&[2, 0])).unwrap();
        assert_eq!(array.element_of(&path(&[2, 0, 7, 1])), Some(7));
        assert_eq!(array.element_of(&path(&[2, 0])), None);
        assert_eq!(array.element_of(&path(&[2, 1, 7])), None);
    }
}
