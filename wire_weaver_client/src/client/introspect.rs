use std::time::Duration;

use crate::Promise;
use crate::Stream;
use crate::event_loop::commander::TransportCommander;
use crate::local_registry;
use anyhow::{Context, Result};
use tracing::{debug, warn};
use wire_weaver::shrink_wrap::{DeserializeShrinkWrapOwned, SerializeShrinkWrapOwned};
use ww_client_server::PathKindOwned;
use ww_self::ApiBundleOwned;
use ww_snapshots::Resolver;
use ww_version::ApiHashPairOwned;

pub struct Introspect {
    transport_cmd_tx: TransportCommander,
    hash: ApiHashPairOwned,
    timeout: Duration,
    /// The connection's resolver: puts back what the device left out, and its cache root holds the cached data.
    resolver: &'static Resolver,
}

impl Introspect {
    pub(crate) fn new(
        transport_cmd_tx: TransportCommander,
        hash: ApiHashPairOwned,
        resolver: &'static Resolver,
    ) -> Self {
        let timeout = transport_cmd_tx.default_timeout();
        Introspect {
            transport_cmd_tx,
            hash,
            timeout,
            resolver,
        }
    }

    /// Use a provided timeout instead of the default one propagated from Commander.
    /// Download fails if no data is received from a device during this time,
    /// it is restarted after each received chunk.
    pub fn with_timeout(self, timeout: Duration) -> Self {
        Self { timeout, ..self }
    }

    /// Load introspect data from the local cache (the root of the connection's resolver's cache, `~/.wire_weaver/`
    /// by default) if available.
    /// Otherwise download it from a device and cache.
    ///
    /// Additionally, if introspect data with doc strings is available in the cache, it will be loaded instead.
    ///
    /// See also: [Introspect::download]
    pub async fn get(self) -> Result<Option<ApiBundleOwned>> {
        let resolver = self.resolver;
        match self.get_as_sent().await? {
            Some((bundle, _)) => inlined_blocking(bundle, resolver).await.map(Some),
            None => Ok(None),
        }
    }

    /// Load introspect data from the local cache (the root of the connection's resolver's cache, `~/.wire_weaver/`
    /// by default) if available.
    /// Otherwise download it from a device and cache.
    ///
    /// Additionally, if introspect data with doc strings is available in the cache, it will be loaded instead.
    ///
    /// See also: [Introspect::download_blocking]
    pub fn get_blocking(self) -> Result<Option<ApiBundleOwned>> {
        let resolver = self.resolver;
        self.get_as_sent_blocking()?
            .map(|(bundle, _)| inlined(bundle, resolver))
            .transpose()
    }

    /// Same as [Introspect::get], but returns introspection data as sent by the device (without traits and types
    /// known from snapshots put back) and its size.
    pub(crate) async fn get_as_sent(self) -> Result<Option<(ApiBundleOwned, usize)>> {
        if let Some(sized) = load_cached(self.resolver, &self.hash) {
            return Ok(Some(sized));
        }
        self.cache_miss_msg();
        self.download_as_sent().await
    }

    /// Same as [Introspect::get_blocking], but returns introspection data as sent by the device (without traits and
    /// types known from snapshots put back) and its size.
    pub(crate) fn get_as_sent_blocking(self) -> Result<Option<(ApiBundleOwned, usize)>> {
        if let Some(sized) = load_cached(self.resolver, &self.hash) {
            return Ok(Some(sized));
        }
        self.cache_miss_msg();
        self.download_as_sent_blocking()
    }

    /// Same as [Introspect::get], but returns a Promise that receives data chunks as it is polled.
    /// Resolves to an error if a device has introspection disabled.
    /// Must only be polled from synchronous code, not from async tasks, see [Promise] docs.
    #[must_use = "Promise does nothing, unless it is polled"]
    pub fn get_promise(self) -> Promise<ApiBundleOwned> {
        if let Some((bundle, _)) = load_cached(self.resolver, &self.hash) {
            return match inlined(bundle, self.resolver) {
                Ok(bundle) => Promise::done(bundle, "introspect"),
                Err(e) => Promise::error(crate::Error::Other(format!("{e:#}")), "introspect"),
            };
        }
        self.cache_miss_msg();
        self.download_promise()
    }

    /// Same as [Introspect::download], but returns a Promise that receives data chunks as it is polled: skips the
    /// local cache lookup, and caches what it got. Must only be polled from synchronous code, see [Promise] docs.
    #[must_use = "Promise does nothing, unless it is polled"]
    pub fn download_promise(self) -> Promise<ApiBundleOwned> {
        let hash = self.hash;
        let resolver = self.resolver;
        Promise::new_introspect(
            self.transport_cmd_tx,
            self.timeout,
            Box::new(move |bundle, ww_self_bytes| {
                if let Some(root) = resolver.cache_root() {
                    local_registry::store(root, bundle, ww_self_bytes, &hash);
                }
                inline_known(bundle, resolver).map_err(|e| crate::Error::Other(format!("{e:#}")))
            }),
            "introspect",
        )
    }

    /// Download introspect data from a remote device, skipping the local cache lookup, and cache it.
    /// Returns None if a device has introspection disabled.
    ///
    /// See also [Introspect::get] that uses local cache.
    pub async fn download(self) -> Result<Option<ApiBundleOwned>> {
        let resolver = self.resolver;
        match self.download_as_sent().await? {
            Some((bundle, _)) => inlined_blocking(bundle, resolver).await.map(Some),
            None => Ok(None),
        }
    }

    /// Download introspect data from a remote device, skipping the local cache lookup, and cache it.
    /// Returns None if a device has introspection disabled.
    ///
    /// See also [Introspect::get_blocking] that uses local cache.
    pub fn download_blocking(self) -> Result<Option<ApiBundleOwned>> {
        let resolver = self.resolver;
        self.download_as_sent_blocking()?
            .map(|(bundle, _)| inlined(bundle, resolver))
            .transpose()
    }

    async fn download_as_sent(self) -> Result<Option<(ApiBundleOwned, usize)>> {
        let rx = self.transport_cmd_tx.send_introspect(None).await?;
        let mut stream = Stream {
            transport_cmd_tx: self.transport_cmd_tx,
            path_kind: PathKindOwned::Absolute { path: vec![] },
            rx,
            unpacker: crate::client::batch::Unpacker::new(None),
            pending: Default::default(),
            _phantom: Default::default(),
        };
        let ww_self_bytes = stream.recv_all_bytes_timeout(self.timeout).await?;
        decode_and_cache(&ww_self_bytes, &self.hash, self.resolver)
    }

    fn download_as_sent_blocking(self) -> Result<Option<(ApiBundleOwned, usize)>> {
        let rx = self.transport_cmd_tx.send_introspect_blocking(None)?;
        let mut stream = Stream {
            transport_cmd_tx: self.transport_cmd_tx,
            path_kind: PathKindOwned::Absolute { path: vec![] },
            rx,
            unpacker: crate::client::batch::Unpacker::new(None),
            pending: Default::default(),
            _phantom: Default::default(),
        };
        let ww_self_bytes = stream.recv_all_bytes_timeout_blocking(self.timeout)?;
        decode_and_cache(&ww_self_bytes, &self.hash, self.resolver)
    }

    fn cache_miss_msg(&self) {
        debug!("not found in cache, downloading from device...");
    }
}

/// Introspection data as a device sends it (the bytes of its introspection stream, concatenated), decoded, with the
/// traits and types it left out put back (snapshots, definition cache) and generic types instantiated: the API of a
/// device whose traffic a program forwards rather than calls, as the ww hub does to find claimable resources (SRV-34).
/// `None` for an empty stream (a device with introspection disabled); an error for malformed data. Compressed data
/// (INT-16) is decompressed first. `resolver` finds what the device left out ([Resolver::global] for a host's).
pub fn decode_introspection(
    ww_self_bytes: &[u8],
    resolver: &Resolver,
) -> Result<Option<ApiBundleOwned>> {
    if ww_self_bytes.is_empty() {
        return Ok(None);
    }
    let ww_self_bytes = ww_self::compressed::bundle_bytes(ww_self_bytes)?;
    inlined(
        ApiBundleOwned::from_ww_bytes_owned(&ww_self_bytes)?,
        resolver,
    )
    .map(Some)
}

/// Put a device's introspection data into the cache under `resolver`'s cache root ([Resolver::cache_root]), as a
/// download from the device would: the bytes of its `api_impl::WW_SELF_BYTES` (compressed or not) and the hashes of
/// its `api_impl::api_hash()`. For a host that knows the device's build, also when the device doesn't send them
/// (introspection off); a connection through `resolver` then finds the API without asking the device. An error when
/// the bytes are malformed, match neither hash, or the resolver has no cache.
pub fn cache_introspection(
    resolver: &Resolver,
    ww_self_bytes: &[u8],
    hash: &ApiHashPairOwned,
) -> Result<()> {
    let root = resolver
        .cache_root()
        .context("the resolver has no cache to keep introspection data in")?;
    let ww_self_bytes = &*ww_self::compressed::bundle_bytes(ww_self_bytes)?;
    let api_bundle = ApiBundleOwned::from_ww_bytes_owned(ww_self_bytes)?;
    local_registry::store_checked(root, &api_bundle, ww_self_bytes, hash)
}

/// The bundle a device sent and its size, uncompressed (INT-16): the API hashes and the cache are over these bytes.
fn decode_and_cache(
    ww_self_bytes: &[u8],
    hash: &ApiHashPairOwned,
    resolver: &Resolver,
) -> Result<Option<(ApiBundleOwned, usize)>> {
    if ww_self_bytes.is_empty() {
        return Ok(None);
    }
    let ww_self_bytes = &*ww_self::compressed::bundle_bytes(ww_self_bytes)?;
    let api_bundle = ApiBundleOwned::from_ww_bytes_owned(ww_self_bytes)?;
    // cached as received, so that it matches the device reported hash
    if let Some(root) = resolver.cache_root() {
        local_registry::store(root, &api_bundle, ww_self_bytes, hash);
    }
    Ok(Some((api_bundle, ww_self_bytes.len())))
}

/// Cached bundle (as it was received) and its size, from the root of `resolver`'s cache.
fn load_cached(resolver: &Resolver, hash: &ApiHashPairOwned) -> Option<(ApiBundleOwned, usize)> {
    let bundle = local_registry::load(resolver.cache_root()?, hash)?;
    let sent_size = bundle.to_ww_bytes_owned().ok()?.len();
    Some((bundle, sent_size))
}

fn inlined(mut bundle: ApiBundleOwned, resolver: &Resolver) -> Result<ApiBundleOwned> {
    inline_known(&mut bundle, resolver)?;
    Ok(bundle)
}

/// [inlined] off the runtime's workers: the resolver reads files, and may fetch a snapshot, on the way.
async fn inlined_blocking(
    bundle: ApiBundleOwned,
    resolver: &'static Resolver,
) -> Result<ApiBundleOwned> {
    tokio::task::spawn_blocking(move || inlined(bundle, resolver))
        .await
        .context("inlining introspection data")?
}

/// Put back traits and types that a device left out of its introspection data, because they are known from
/// snapshots `resolver` finds (a connection's, [Resolver::global] by default, see [crate::snapshots]), or by
/// signature from its definition cache ([ww_self::definition_cache]). The ones that are not found are logged and left
/// skipped. Then checks the bundle and instantiates its generic types ([ww_self::generics::prepare]): introspection
/// data comes from a device and is not trusted, a malformed one is an error.
pub(crate) fn inline_known(bundle: &mut ApiBundleOwned, resolver: &Resolver) -> Result<()> {
    inline_known_with(bundle, resolver);
    ww_self::generics::prepare(bundle).context("malformed introspection data")
}

/// [inline_known] through `resolver`, without the check that follows.
pub(crate) fn inline_known_with(
    bundle: &mut ApiBundleOwned,
    resolver: &Resolver,
) -> ww_self::inline::Inlined {
    let inlined = resolver.inline_skipped(bundle);
    for from_cache in &inlined.from_cache {
        warn!(
            "Device API has {from_cache}: likely a build from another revision of an unpublished crate, its \
             definition is from the definition cache and is compared by the evolution rules"
        );
    }
    for not_inlined in &inlined.not_inlined {
        warn!("Device API refers to {not_inlined}");
    }
    // a device built with other doc comments of a dependency: the same definition (EVO-69)
    for docs_differ in &inlined.docs_differ {
        tracing::info!("Device API has {docs_differ}");
    }
    inlined
}
