use std::fs;

use crate::Promise;
use crate::Stream;
use crate::device_info::ApiHash;
use crate::event_loop::commander::TransportCommander;
use anyhow::{Result, anyhow};
use tracing::debug;
use tracing::warn;
use wire_weaver::shrink_wrap::DeserializeShrinkWrapOwned;
use ww_client_server::PathKindOwned;
use ww_self::ApiBundleOwned;

pub struct Introspect {
    transport_cmd_tx: TransportCommander,
    hash_no_docs: ApiHash,
    hash_with_docs: ApiHash,
}

impl Introspect {
    pub(crate) fn new(
        transport_cmd_tx: TransportCommander,
        hash_no_docs: ApiHash,
        hash_with_docs: ApiHash,
    ) -> Self {
        Introspect {
            transport_cmd_tx,
            hash_no_docs,
            hash_with_docs,
        }
    }

    /// Load introspect data from local cache at `~/.wire_weaver/` if available.
    /// Otherwise download it from a device and cache.
    ///
    /// Additionally, if introspect data with doc strings is available in the cache, it will be loaded instead.
    ///
    /// See also: [Introspect::download]
    pub async fn get(self) -> Result<ApiBundleOwned> {
        if let Some(bundle) = get_from_cache(&self.hash_no_docs, &self.hash_with_docs)? {
            return Ok(bundle);
        }
        debug!("not found in cache, downloading from device...");
        let bundle = self.download().await?;

        Ok(bundle)
    }

    /// Load introspect data from local cache at `~/.wire_weaver/` if available.
    /// Otherwise download it from a device and cache.
    ///
    /// Additionally, if introspect data with doc strings is available in the cache, it will be loaded instead.
    ///
    /// See also: [Introspect::download_blocking]
    pub fn get_blocking(self) -> Result<ApiBundleOwned> {
        if let Some(bundle) = get_from_cache(&self.hash_no_docs, &self.hash_with_docs)? {
            return Ok(bundle);
        }
        debug!("not found in cache, downloading from device...");
        let bundle = self.download_blocking()?;

        Ok(bundle)
    }

    /// Download introspect data from a remote device.
    ///
    /// See also [Introspect::get] that uses local cache.
    pub async fn download(self) -> Result<ApiBundleOwned> {
        // TODO: set introspect download data timeout
        let rx = self.transport_cmd_tx.send_introspect(None).await?;
        let mut stream = Stream {
            transport_cmd_tx: self.transport_cmd_tx,
            path_kind: PathKindOwned::Absolute { path: vec![] },
            rx,
            _phantom: Default::default(),
        };
        let ww_self_bytes = stream.recv_all_bytes().await?;
        let api_bundle = ApiBundleOwned::from_ww_bytes_owned(&ww_self_bytes)?;
        Ok(api_bundle)
    }

    /// Download introspect data from a remote device.
    ///
    /// See also [Introspect::get_blocking] that uses local cache.
    pub fn download_blocking(self) -> Result<ApiBundleOwned> {
        let rx = self.transport_cmd_tx.send_introspect_blocking(None)?;
        let mut stream = Stream {
            transport_cmd_tx: self.transport_cmd_tx,
            path_kind: PathKindOwned::Absolute { path: vec![] },
            rx,
            _phantom: Default::default(),
        };
        let ww_self_bytes = stream.recv_all_bytes_blocking()?;
        let api_bundle = ApiBundleOwned::from_ww_bytes_owned(&ww_self_bytes)?;
        Ok(api_bundle)
    }

    #[must_use = "Promise does nothing, unless it is polled"]
    pub fn download_promise(self) -> Promise<ApiBundleOwned> {
        Promise::new_introspect(self.transport_cmd_tx, "introspect")
    }
}

fn get_from_cache(no_docs: &ApiHash, with_docs: &ApiHash) -> Result<Option<ApiBundleOwned>> {
    match get_from_cache_inner(no_docs, with_docs) {
        Ok(Some(bundle)) => Ok(Some(bundle)),
        Err(e) => {
            warn!("Failed to read API bundle from cache: {e:?}");
            Ok(None)
        }
        Ok(None) => Ok(None),
    }
}

fn get_from_cache_inner(no_docs: &ApiHash, with_docs: &ApiHash) -> Result<Option<ApiBundleOwned>> {
    let hash = if with_docs.0.is_empty() {
        format!("{}.ron", no_docs.to_string())
    } else {
        format!("{}+docs.ron", with_docs.to_string())
    };
    debug!("{hash}");
    let local_registry_path = std::env::home_dir()
        .ok_or(anyhow!("no home directory"))?
        .join(".wire_weaver");
    for entry in fs::read_dir(local_registry_path)? {
        let entry = entry?;
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        if file_name.contains(&hash) {
            let contents = fs::read_to_string(entry.path())?;
            let api_bundle: ApiBundleOwned = ron::from_str(&contents)?;
            debug!("got API bundle from cache");
            return Ok(Some(api_bundle));
        }
        debug!("{entry:?}");
    }
    Ok(None)
}
