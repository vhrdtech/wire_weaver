//! Global trait paths resolved on the host (SRV-53).
//!
//! A device with introspection resolves no global trait path itself (`PathKind::GlobalFull`, `GlobalCompact`,
//! API-20): it answers them with `TraitNotFound`, and the bytes a lookup would take go to its introspection data.
//! The client finds the trait in that data instead and sends the request with the trait's absolute path:
//!
//! - when the device's API is known (downloaded at connect, from the cache, or the client's own), a request with a
//!   global path goes out with the absolute path right away ([GlobalPaths::resolve], called by the Commander);
//! - when it isn't (a `traits_only` connection), the request goes out as it is, and on `TraitNotFound` the event
//!   loop hands it here: the introspection data is fetched once, and the request sent again with the absolute path
//!   ([GlobalPaths::resend_absolute]). From then on the Commander resolves the connection's global paths itself.
//!
//! A trait matches when its crate is the requested one, on the same wire line when the device's introspection data and
//! the crate's embedded snapshot both give its wire version (EVO-26), in a protocol compatible version
//! (`FullVersion::is_protocol_compatible`) otherwise, and its name is the requested one; the first match depth first, in item
//! order, as the device would find it. A trait under a resource array is reached at index 0, which a device's own
//! lookup can't do. One [GlobalPaths] per connection, so per device API; reconnects keep the same API.

use crate::config::IntrospectBundle;
use crate::event_loop::command::{Command, StreamOpenHook};
use crate::event_loop::commander::WeakTransport;
use crate::event_loop::rx_dispatcher::ResponseSender;
use crate::{Error, Introspect};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::OnceCell;
use tracing::{debug, warn};
use wire_weaver::prelude::{DeserializeShrinkWrap, UNib32, UVlq32Backfill};
use wire_weaver::shrink_wrap::SerializeShrinkWrapOwned;
use ww_client_server::{ErrorOwned, PathKind, PathKindOwned, Request};
use ww_self::{ApiBundleOwned, ApiItemKindOwned, ApiLevelOwned, Multiplicity};
use ww_version::{ApiHashPairOwned, FullVersionOwned, VersionOwned, WireVersion};

/// Crate, trait name and version: what decides whether a trait matches (the whole version, its snapshot gives the
/// crate's wire version).
type TraitKey = (String, String, u32, u32, u32);

/// The global trait paths of one connection: the device's API once known, and the paths found in it.
#[derive(Default)]
pub struct GlobalPaths {
    /// The device's API, with the traits it left out of its introspection data put back; `None` inside: the device
    /// has no introspection data (and none is cached for its API).
    api: OnceCell<Option<Arc<ApiBundleOwned>>>,
    /// How to fetch the introspection data and send requests again, set once connected.
    link: Mutex<Option<Link>>,
    /// Paths found so far, `None` for a trait the device's API doesn't have in a compatible version.
    resolved: Mutex<HashMap<TraitKey, Option<Vec<UNib32>>>>,
}

struct Link {
    transport: WeakTransport,
    api_hash: ApiHashPairOwned,
}

/// A request with a `GlobalFull` path the device answered `TraitNotFound` to, to send again with an absolute path.
pub(crate) struct AbsoluteRetry {
    /// The request as sent, seq placeholder in place
    pub(crate) bytes: Vec<u8>,
    pub(crate) done_tx: ResponseSender,
    pub(crate) timeout: Duration,
    pub(crate) hook: Option<Box<StreamOpenHook>>,
    /// The device's answer, for the caller when the trait can't be found
    pub(crate) error: ErrorOwned,
}

impl std::fmt::Debug for GlobalPaths {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GlobalPaths")
            .field("api_known", &self.api.get().map(Option::is_some))
            .finish_non_exhaustive()
    }
}

impl GlobalPaths {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Connected: how to reach the device, and its API if the connection already has it (`Some(None)`: the device
    /// has no introspection data). Called again by a reconnect, which keeps the API.
    pub(crate) fn attach(
        &self,
        transport: WeakTransport,
        api_hash: ApiHashPairOwned,
        api: Option<Option<&IntrospectBundle>>,
    ) {
        *self.link.lock().unwrap_or_else(|e| e.into_inner()) = Some(Link {
            transport,
            api_hash,
        });
        if let Some(api) = api {
            _ = self.api.set(api.map(|b| b.api_bundle.clone()));
        }
    }

    /// Absolute path of the trait `trait_name` of crate `crate_id` in a version compatible with `version`, from the
    /// device's API if it is known; None when it isn't known yet or has no such trait.
    pub(crate) fn resolve(
        &self,
        crate_id: &str,
        version: &VersionOwned,
        trait_name: &str,
    ) -> Option<Vec<UNib32>> {
        let api = self.api.get()?.as_ref()?;
        let key = (
            crate_id.to_string(),
            trait_name.to_string(),
            version.major.0,
            version.minor.0,
            version.patch.0,
        );
        let mut resolved = self.resolved.lock().unwrap_or_else(|e| e.into_inner());
        resolved
            .entry(key)
            .or_insert_with(|| {
                let wanted = FullVersionOwned::new(crate_id.into(), version.clone());
                find_trait(api, &wanted, trait_name)
            })
            .clone()
    }

    /// The device's API: known, or fetched now (from the cache or the device) by the first caller, the others wait
    /// for it. None when the device has no introspection data or the connection is gone; a failed download is tried
    /// again by the next caller.
    async fn api(&self) -> Option<Arc<ApiBundleOwned>> {
        let fetched = self
            .api
            .get_or_try_init(|| async {
                let (transport, api_hash) = {
                    let link = self.link.lock().unwrap_or_else(|e| e.into_inner());
                    let link = link.as_ref().ok_or(Error::Disconnected)?;
                    (link.transport.upgrade(), link.api_hash.clone())
                };
                let transport = transport.ok_or(Error::EventLoopNotRunning)?;
                debug!("resolving a global trait path: getting the device's introspection data");
                let sent = Introspect::new(transport, api_hash.clone())
                    .get_as_sent()
                    .await
                    .map_err(|e| Error::Other(format!("{e:#}")))?;
                let Some((sent, size)) = sent else {
                    return Ok::<_, Error>(None);
                };
                let bundle = IntrospectBundle::from_sent(sent, api_hash, size)
                    .map_err(|e| Error::Other(format!("{e:#}")))?;
                Ok(Some(bundle.api_bundle))
            })
            .await;
        match fetched {
            Ok(api) => api.clone(),
            Err(e) => {
                warn!("no introspection data to resolve a global trait path with: {e}");
                None
            }
        }
    }

    /// Send `retry` again with the absolute path of its trait, found in the device's introspection data (fetched
    /// first if needed), from a task of its own; answer it with the device's `TraitNotFound` when the data has no
    /// such trait or can't be had.
    pub(crate) fn resend_absolute(self: Arc<Self>, retry: AbsoluteRetry) {
        tokio::spawn(async move {
            self.api().await;
            match self.absolute_request(&retry.bytes) {
                Some(bytes) => {
                    let transport = {
                        let link = self.link.lock().unwrap_or_else(|e| e.into_inner());
                        link.as_ref().and_then(|l| l.transport.upgrade())
                    };
                    let Some(transport) = transport else {
                        _ = retry.done_tx.send(Err(Error::EventLoopNotRunning));
                        return;
                    };
                    let cmd = match retry.hook {
                        Some(hook) => Command::OpenStream {
                            bytes,
                            done_tx: retry.done_tx,
                            timeout: retry.timeout,
                            hook,
                        },
                        None => Command::SendMessage {
                            bytes,
                            done_tx: Some((retry.done_tx, retry.timeout)),
                        },
                    };
                    // fails only when the event loop is gone, which answers the request itself
                    _ = transport.send_command(cmd).await;
                }
                None => _ = retry.done_tx.send(Err(Error::RemoteError(retry.error))),
            }
        });
    }

    /// `bytes`, a request with a `GlobalFull` path, with its trait's absolute path instead, seq placeholder in place.
    fn absolute_request(&self, bytes: &[u8]) -> Option<Vec<u8>> {
        let request = Request::from_ww_bytes(bytes).ok()?;
        let PathKind::GlobalFull {
            gid,
            trait_name,
            path_from_trait,
        } = &request.path_kind
        else {
            return None;
        };
        let gid = gid.make_owned();
        let mut path = self.resolve(&gid.crate_id, &gid.version, trait_name)?;
        for id in path_from_trait.iter() {
            path.push(id.ok()?);
        }
        let mut absolute = request.make_owned().ok()?;
        absolute.seq = UVlq32Backfill(0);
        absolute.path_kind = PathKindOwned::Absolute { path };
        absolute.to_ww_bytes_owned().ok()
    }
}

/// Absolute path of the first trait in `api` depth first (the root first, then items in order) whose crate is
/// protocol compatible with `wanted` and whose name is `trait_name`. A trait under a resource array is reached at
/// index 0; items the device leaves out (API-16) are skipped.
pub(crate) fn find_trait(
    api: &ApiBundleOwned,
    wanted: &FullVersionOwned,
    trait_name: &str,
) -> Option<Vec<UNib32>> {
    let omitted: Vec<u32> = api.root_omitted.iter().flatten().map(|id| id.0).collect();
    let wanted = Wanted {
        version: wanted,
        wire: wire_weaver_snapshots::get(wanted).and_then(|s| s.wire),
    };
    find_in_level(api, &api.root, &omitted, &wanted, trait_name, &mut vec![])
}

/// The crate a global path asks for, with its wire version when the host knows it (EVO-26): from the crate's
/// embedded snapshot, so for `ww_stdlib` crates.
struct Wanted<'a> {
    version: &'a FullVersionOwned,
    wire: Option<WireVersion>,
}

impl Wanted<'_> {
    /// The device's crate `crate_idx` talks with the wanted one: the same crate on one wire line when both wire
    /// versions are known, so a Rust-only break of the crate (a new crate major) doesn't hide the trait; protocol
    /// compatible crate versions otherwise.
    fn matches(&self, api: &ApiBundleOwned, crate_idx: u32) -> bool {
        let Ok(version) = api.crate_version(crate_idx) else {
            return false;
        };
        match (api.crate_wire(crate_idx), self.wire) {
            (Some(device), Some(wanted)) => {
                version.crate_id == self.version.crate_id && device.same_line(&wanted)
            }
            _ => version.is_protocol_compatible(self.version),
        }
    }
}

fn find_in_level(
    api: &ApiBundleOwned,
    level: &ApiLevelOwned,
    omitted: &[u32],
    wanted: &Wanted,
    trait_name: &str,
    path: &mut Vec<UNib32>,
) -> Option<Vec<UNib32>> {
    if level.trait_name == trait_name && wanted.matches(api, level.crate_idx.0) {
        return Some(path.clone());
    }
    for item in &level.items {
        let ApiItemKindOwned::Trait {
            omitted: item_omitted,
            ..
        } = &item.kind
        else {
            continue;
        };
        if omitted.contains(&item.id.0) {
            continue;
        }
        let Ok(child) = item.get_as_level(api) else {
            continue;
        };
        let depth = path.len();
        path.push(item.id);
        if matches!(item.multiplicity, Multiplicity::Array { .. }) {
            path.push(UNib32(0));
        }
        let child_omitted: Vec<u32> = item_omitted.iter().flatten().map(|id| id.0).collect();
        let found = find_in_level(api, child, &child_omitted, wanted, trait_name, path);
        path.truncate(depth);
        if found.is_some() {
            return found;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compat::tests::bundle;

    const API: &str = r#"
#[ww_api_root(wire = "w1.0")]
pub trait Device {
    fn ping();
    ww_impl!(info: Info);
    ww_impl!(port[]: Port);
    ww_impl!(spare: Info);
}

#[ww_trait]
pub trait Info {
    fn name() -> u8;
}

#[ww_trait]
pub trait Port {
    fn open();
    ww_impl!(status: Status);
}

#[ww_trait]
pub trait Status {
    fn get() -> u8;
}
"#;

    fn find(api: &ApiBundleOwned, version: (u32, u32), trait_name: &str) -> Option<Vec<u32>> {
        let wanted = FullVersionOwned::new(
            "test_api".into(),
            VersionOwned::new(version.0, version.1, 7),
        );
        find_trait(api, &wanted, trait_name).map(|p| p.iter().map(|id| id.0).collect())
    }

    #[test]
    fn traits_found_depth_first() {
        let api = bundle("global_paths", "0.2.1", API);
        assert_eq!(find(&api, (0, 2), "Device"), Some(vec![]));
        // the first of two, in item order
        assert_eq!(find(&api, (0, 2), "Info"), Some(vec![1]));
        // under a resource array: at index 0
        assert_eq!(find(&api, (0, 2), "Port"), Some(vec![2, 0]));
        assert_eq!(find(&api, (0, 2), "Status"), Some(vec![2, 0, 1]));
        assert_eq!(find(&api, (0, 2), "Missing"), None);
    }

    #[test]
    fn version_must_be_protocol_compatible() {
        let api = bundle("global_paths_v", "0.2.1", API);
        assert_eq!(find(&api, (0, 3), "Info"), None);
        assert_eq!(find(&api, (1, 2), "Info"), None);
        // another patch is compatible
        assert_eq!(find(&api, (0, 2), "Info"), Some(vec![1]));
    }

    /// A `ww_stdlib` trait is found by its crate's wire line (EVO-26): the host asks for `ww_board_info` 0.3.0
    /// (its embedded snapshot: w1.2), the device has 0.4.0, a Rust-only break on the same line. Another line hides it.
    #[test]
    fn stdlib_trait_found_by_wire_line() {
        let board_info = format!(
            "ww_board_info = {{ path = {:?} }}",
            concat!(env!("CARGO_MANIFEST_DIR"), "/../ww_stdlib/ww_board_info")
        );
        let src = r#"
            #[ww_api_root(wire = "w1.0")]
            pub trait Api {
                fn ping();
                ww_impl!(board: ww_board_info::BoardInfo);
            }
        "#;
        let mut device =
            crate::compat::tests::bundle_with("global_paths_w", "0.1.0", src, &board_info);
        let idx = device
            .ext_crates
            .iter()
            .position(|c| c.crate_id == "ww_board_info")
            .unwrap();
        let wanted = device.ext_crates[idx].clone();
        assert_eq!(
            wire_weaver_snapshots::get(&wanted).and_then(|s| s.wire),
            Some(WireVersion::new(1, 2))
        );
        device.ext_crates[idx].version = VersionOwned::new(0, 4, 0);
        device.set_crate_wire(idx as u32, Some(WireVersion::new(1, 3)));
        let found = find_trait(&device, &wanted, "BoardInfo");
        assert_eq!(found, Some(vec![UNib32(1)]));

        device.set_crate_wire(idx as u32, Some(WireVersion::new(2, 0)));
        assert_eq!(find_trait(&device, &wanted, "BoardInfo"), None);
        // without the device's wire version, the crate versions decide: 0.4.0 is not 0.3.0
        device.set_crate_wire(idx as u32, None);
        assert_eq!(find_trait(&device, &wanted, "BoardInfo"), None);
    }

    #[test]
    fn omitted_trait_is_skipped() {
        let mut api = bundle("global_paths_o", "0.2.1", API);
        api.root_omitted = Some(vec![UNib32(1)]);
        assert_eq!(find(&api, (0, 2), "Info"), Some(vec![3]));
    }

    #[test]
    fn resolve_caches_per_trait() {
        let api = bundle("global_paths_r", "0.2.1", API);
        let paths = GlobalPaths::new();
        let version = VersionOwned::new(0, 2, 0);
        assert_eq!(
            paths.resolve("test_api", &version, "Port"),
            None,
            "API not known yet"
        );
        _ = paths.api.set(Some(Arc::new(api)));
        assert_eq!(
            paths.resolve("test_api", &version, "Port"),
            Some(vec![UNib32(2), UNib32(0)])
        );
        assert_eq!(paths.resolved.lock().unwrap().len(), 1);
        assert_eq!(paths.resolve("other_api", &version, "Port"), None);
        assert_eq!(paths.resolved.lock().unwrap().len(), 2);
    }
}
