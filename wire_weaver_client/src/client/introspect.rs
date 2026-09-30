use std::time::Duration;

use crate::Promise;
use crate::Stream;
use crate::event_loop::commander::TransportCommander;
use crate::local_registry;
use anyhow::Result;
use tracing::debug;
use wire_weaver::shrink_wrap::DeserializeShrinkWrapOwned;
use ww_client_server::PathKindOwned;
use ww_self::ApiBundleOwned;
use ww_version::ApiHashPairOwned;

pub struct Introspect {
    transport_cmd_tx: TransportCommander,
    hash: ApiHashPairOwned,
    timeout: Duration,
}

impl Introspect {
    pub(crate) fn new(transport_cmd_tx: TransportCommander, hash: ApiHashPairOwned) -> Self {
        let timeout = transport_cmd_tx.default_timeout();
        Introspect {
            transport_cmd_tx,
            hash,
            timeout,
        }
    }

    /// Use a provided timeout instead of the default one propagated from Commander.
    /// Download fails if no data is received from a device during this time,
    /// it is restarted after each received chunk.
    pub fn with_timeout(self, timeout: Duration) -> Self {
        Self { timeout, ..self }
    }

    /// Load introspect data from local cache at `~/.wire_weaver/` if available.
    /// Otherwise download it from a device and cache.
    ///
    /// Additionally, if introspect data with doc strings is available in the cache, it will be loaded instead.
    ///
    /// See also: [Introspect::download]
    pub async fn get(self) -> Result<Option<ApiBundleOwned>> {
        if let Some(bundle) = local_registry::load(&self.hash) {
            return Ok(Some(bundle));
        }
        self.cache_miss_msg();
        self.download().await
    }

    /// Load introspect data from local cache at `~/.wire_weaver/` if available.
    /// Otherwise download it from a device and cache.
    ///
    /// Additionally, if introspect data with doc strings is available in the cache, it will be loaded instead.
    ///
    /// See also: [Introspect::download_blocking]
    pub fn get_blocking(self) -> Result<Option<ApiBundleOwned>> {
        if let Some(bundle) = local_registry::load(&self.hash) {
            return Ok(Some(bundle));
        }
        self.cache_miss_msg();
        self.download_blocking()
    }

    /// Same as [Introspect::get], but returns a Promise that receives data chunks as it is polled.
    /// Resolves to an error if a device has introspection disabled.
    #[must_use = "Promise does nothing, unless it is polled"]
    pub fn get_promise(self) -> Promise<ApiBundleOwned> {
        if let Some(bundle) = local_registry::load(&self.hash) {
            return Promise::done(bundle, "introspect");
        }
        self.cache_miss_msg();
        let hash = self.hash;
        Promise::new_introspect(
            self.transport_cmd_tx,
            self.timeout,
            Box::new(move |bundle, ww_self_bytes| {
                local_registry::store(bundle, ww_self_bytes, &hash)
            }),
            "introspect",
        )
    }

    /// Download introspect data from a remote device, skipping the local cache lookup, and cache it.
    /// Returns None if a device has introspection disabled.
    ///
    /// See also [Introspect::get] that uses local cache.
    pub async fn download(self) -> Result<Option<ApiBundleOwned>> {
        let rx = self.transport_cmd_tx.send_introspect(None).await?;
        let mut stream = Stream {
            transport_cmd_tx: self.transport_cmd_tx,
            path_kind: PathKindOwned::Absolute { path: vec![] },
            rx,
            _phantom: Default::default(),
        };
        let ww_self_bytes = stream.recv_all_bytes_timeout(self.timeout).await?;
        decode_and_cache(&ww_self_bytes, &self.hash)
    }

    /// Download introspect data from a remote device, skipping the local cache lookup, and cache it.
    /// Returns None if a device has introspection disabled.
    ///
    /// See also [Introspect::get_blocking] that uses local cache.
    pub fn download_blocking(self) -> Result<Option<ApiBundleOwned>> {
        let rx = self.transport_cmd_tx.send_introspect_blocking(None)?;
        let mut stream = Stream {
            transport_cmd_tx: self.transport_cmd_tx,
            path_kind: PathKindOwned::Absolute { path: vec![] },
            rx,
            _phantom: Default::default(),
        };
        let ww_self_bytes = stream.recv_all_bytes_timeout_blocking(self.timeout)?;
        decode_and_cache(&ww_self_bytes, &self.hash)
    }

    fn cache_miss_msg(&self) {
        debug!("not found in cache, downloading from device...");
    }
}

fn decode_and_cache(
    ww_self_bytes: &[u8],
    hash: &ApiHashPairOwned,
) -> Result<Option<ApiBundleOwned>> {
    if ww_self_bytes.is_empty() {
        return Ok(None);
    }
    let api_bundle = ApiBundleOwned::from_ww_bytes_owned(ww_self_bytes)?;
    local_registry::store(&api_bundle, ww_self_bytes, hash);
    Ok(Some(api_bundle))
}
