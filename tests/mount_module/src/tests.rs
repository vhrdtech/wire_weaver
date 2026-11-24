//! The TV's hub slot served by modules loaded into the TV's process, seen by the generated and dynamic clients.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::oneshot;
use wire_weaver::prelude::*;
use wire_weaver::ww_api::mount::MountPresenceOwned;
use wire_weaver_client::internal::Command;
use wire_weaver_client::observe::on_change;
use wire_weaver_client::ww_self::ValueOwned;
use wire_weaver_client::{Commander, Error};
use wire_weaver_client::{DynObserver, DynResource};
use ww_client_server::{
    ErrorKindOwned, ErrorOwned, PathKindOwned, RequestKindOwned, RequestOwned, ResourceIndex,
};

use crate::modules::{Broken, HubControl, HubModule, Loaded, TvModule, hub_impl, tv_impl};
use crate::tv::{Control, start};

mod clients {
    use wire_weaver_client::{ClientConfig, Commander, WwClient};

    macro_rules! client {
        ($name:ident, $api:path, $file:literal) => {
            pub mod $name {
                use super::*;

                pub struct Client {
                    pub cmd: Commander,
                }

                impl WwClient for Client {
                    fn default_config() -> ClientConfig {
                        ClientConfig::default()
                    }

                    fn try_from_cmd(cmd: Commander) -> Result<Self, wire_weaver_client::Error> {
                        Ok(Self { cmd })
                    }
                }

                mod api_client {
                    wire_weaver::ww_codegen!($api for super::Client, client = "std_client", debug_to_file = $file);
                }
            }
        };
    }

    client!(
        tv,
        mounts_api::Tv,
        "../../target/tests_mount_module_tv_client.rs"
    );
    client!(
        hub,
        mount_backend_api::Hub,
        "../../target/tests_mount_module_hub_client.rs"
    );
}

/// A resolver without a cache: what the client knows of a mounted API comes through the slot.
fn no_cache() -> &'static wire_weaver_client::snapshots::Resolver {
    Box::leak(Box::new(wire_weaver_client::snapshots::Resolver::offline(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")),
    )))
}

// ids: the TV's `hub` 0, a slot's `presence` 0 and `api` 1; the hub's `add` 0, `hang` 1
const HUB: u32 = 0;
const API: u32 = 1;
const PRESENCE: u32 = 0;
const ADD: u32 = 0;
// the lamp's `toggle` 0 (the hub's `lamp` 2)
const TOGGLE: u32 = 0;

fn path(ids: &[u32]) -> Vec<ResourceIndex> {
    ids.iter().map(|i| ResourceIndex::new(UNib32(*i))).collect()
}

/// Send a request as is and wait for the answer: the value bytes, or the device's error.
async fn raw(cmd: &Commander, ids: &[u32], kind: RequestKindOwned) -> Result<Vec<u8>, Error> {
    raw_at(cmd, PathKindOwned::Absolute { path: path(ids) }, kind).await
}

/// [raw] at any path.
async fn raw_at(
    cmd: &Commander,
    path_kind: PathKindOwned,
    kind: RequestKindOwned,
) -> Result<Vec<u8>, Error> {
    let bytes = RequestOwned {
        seq: UVlq32Backfill(0),
        priority: ww_client_server::Priority::NORMAL.to_wire(),
        context: None,
        path_kind,
        kind,
    }
    .to_ww_bytes_owned()
    .unwrap();
    let (tx, rx) = oneshot::channel();
    cmd.send(Command::SendMessage {
        bytes,
        done_tx: Some((tx, Duration::from_secs(2))),
        trace: None,
    })
    .await?;
    rx.await.expect("event loop answers")
}

fn add_args(a: u32, b: u32) -> RequestKindOwned {
    let mut args = a.to_ww_bytes_owned().unwrap();
    args.extend(b.to_ww_bytes_owned().unwrap());
    RequestKindOwned::Call {
        args: wire_weaver::shrink_wrap::tail_bytes::TailBytesOwned(args),
    }
}

fn kind_of(answer: Result<Vec<u8>, Error>) -> Option<ErrorKindOwned> {
    match answer {
        Err(Error::RemoteError(ErrorOwned { kind, .. })) => Some(kind),
        _ => None,
    }
}

fn hash_of(presence: &MountPresenceOwned) -> Option<Vec<u8>> {
    match presence {
        MountPresenceOwned::Up { hash, .. } => Some(hash.no_docs.hash.clone()),
        _ => None,
    }
}

fn hub_module() -> (Loaded, Arc<Mutex<HubControl>>) {
    let control = Arc::new(Mutex::new(HubControl::default()));
    (Loaded::Hub(HubModule::new(control.clone())), control)
}

/// Read the hub's presence until it is `Up` (`up`) or `Absent`: the TV applies a load at its next poll.
async fn wait_presence(tv: &clients::tv::Client, up: bool) {
    loop {
        let presence = tv.hub().unwrap().presence().read().await.unwrap();
        if hash_of(&presence).is_some() == up {
            return;
        }
        tokio::task::yield_now().await;
    }
}

async fn connect(device: &tests_common::TestDevice) -> clients::tv::Client {
    clients::tv::Client::config(|c| c.in_process_path(device.path()).resolver(no_cache()))
        .connect()
        .await
        .expect("connect")
}

/// The slot's presence hash is the hash of the model the module handed over, computed by the slot: a server's
/// introspection data without docs hashes to its own `API_HASH_NO_DOCS`, dependency types left out or not.
#[test]
fn presence_hash_is_the_servers() {
    assert_eq!(
        ww_self::hash::api_hash(&hub_impl::WW_SELF_BYTES_NO_DOCS),
        hub_impl::API_HASH_NO_DOCS
    );
    // the TV's model leaves `ww_keyed`'s key events out, as its introspection data does
    assert_eq!(
        ww_self::hash::api_hash(&tv_impl::WW_SELF_BYTES_NO_DOCS),
        tv_impl::API_HASH_NO_DOCS
    );
}

/// A module serves the hub slot: a generated client of the hub on a view of the slot calls it, its stream items
/// reach the client under the slot's path, its presence names the model's crate and hash.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_module_serves_the_slot() {
    let control = Arc::new(Mutex::new(Control::default()));
    let device = start("mount_module/serves", control.clone());
    let tv = connect(&device).await;
    let (hub, ticks) = hub_module();
    control.lock().unwrap().next = Some(Some(hub));

    let line = clients::hub::Client::client_line();
    let view = tv.hub().unwrap().view_for(&line).await.unwrap();
    let mut status = view.status();
    let up = status
        .wait_for(|p| hash_of(p).is_some())
        .await
        .unwrap()
        .clone();
    let MountPresenceOwned::Up { api, hash, wire } = &up else {
        unreachable!("waited for Up");
    };
    assert_eq!(api.crate_id, "mount_backend_api");
    assert_eq!(hash.no_docs.hash, hub_impl::API_HASH_NO_DOCS);
    assert_eq!(*wire, Some(mount_backend_api::HUB_WIRE_VERSION));

    let hub = clients::hub::Client::on_mount(&view).unwrap();
    assert_eq!(hub.add(2, 3).call().await.unwrap(), 5);
    assert!(hub.lamp().toggle().call().await.unwrap());

    let mut stream = hub.ticks().call().await.unwrap();
    ticks.lock().unwrap().ticks.extend([7, 8]);
    let mut got = vec![];
    while got.len() < 2 {
        let item = tokio::time::timeout(Duration::from_secs(5), stream.recv())
            .await
            .expect("ticks arrive")
            .expect("the stream runs");
        got.extend(item.0);
    }
    assert_eq!(got, [7, 8]);
    assert_eq!(control.lock().unwrap().counters, Default::default());
}

/// Both slots' presences observed at once on the async `no_alloc` TV, whose getters give a presence borrowed from
/// the server: the hub's level and `ext`'s element level each send theirs, and a module loaded into the hub slot
/// reaches its observer while `ext` stays empty.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn both_presences_are_observed() {
    let control = Arc::new(Mutex::new(Control::default()));
    let device = start("mount_module/observe_both", control.clone());
    let tv = connect(&device).await;
    let bundle = tv
        .cmd
        .device_introspect()
        .expect("introspection enabled")
        .api_bundle
        .clone();
    let root = DynResource::root(bundle);
    let entries = || BTreeMap::from([(PRESENCE, on_change(Duration::ZERO))]);
    let mut hub = root
        .child("hub")
        .unwrap()
        .observe(&tv.cmd, entries())
        .await
        .unwrap();
    let mut ext = root
        .child("ext")
        .unwrap()
        .index(0)
        .unwrap()
        .observe(&tv.cmd, entries())
        .await
        .unwrap();
    assert_eq!(observed_variant(&mut hub).await, "Absent");
    assert_eq!(observed_variant(&mut ext).await, "Absent");

    let (module, _) = hub_module();
    control.lock().unwrap().next = Some(Some(module));
    assert_eq!(observed_variant(&mut hub).await, "Up");
    assert!(
        ext.try_recv().unwrap().is_none(),
        "ext's presence didn't change"
    );
}

/// The variant of the presence in the observer's next update.
async fn observed_variant(observer: &mut DynObserver) -> String {
    let update = tokio::time::timeout(Duration::from_secs(5), observer.recv())
        .await
        .expect("an update arrives")
        .expect("the observer runs");
    match &update.values[..] {
        [(name, ValueOwned::Enum { variant, .. })] if name == "presence" => variant.clone(),
        other => panic!("not one presence: {other:?}"),
    }
}

/// Presence follows load and unload: a request into an empty slot is `Unavailable` at once, a stream open through
/// the slot ends when its module goes, a request the module held is answered `Unavailable`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn presence_follows_load_and_unload() {
    let control = Arc::new(Mutex::new(Control::default()));
    let device = start("mount_module/presence", control.clone());
    let tv = connect(&device).await;
    let cmd = tv.cmd.clone();
    let view = tv.hub().unwrap().view().await.unwrap();
    let mut status = view.status();
    assert!(hash_of(&status.borrow()).is_none());
    let empty = raw(&cmd, &[HUB, API, ADD], add_args(1, 1)).await;
    assert!(matches!(kind_of(empty), Some(ErrorKindOwned::Unavailable)));

    let (hub, hub_control) = hub_module();
    control.lock().unwrap().next = Some(Some(hub));
    status.wait_for(|p| hash_of(p).is_some()).await.unwrap();
    let sum = raw(&cmd, &[HUB, API, ADD], add_args(1, 2)).await.unwrap();
    assert_eq!(u32::from_ww_bytes(&sum).unwrap(), 3);

    let line = clients::hub::Client::client_line();
    let hub_view = tv.hub().unwrap().view_for(&line).await.unwrap();
    let hub = clients::hub::Client::on_mount(&hub_view).unwrap();
    let mut stream = hub.ticks().call().await.unwrap();
    let held = tokio::spawn(async move { hub.hang().call().await });
    while hub_control.lock().unwrap().hung == 0 {
        tokio::task::yield_now().await;
    }

    control.lock().unwrap().next = Some(None);
    status.wait_for(|p| hash_of(p).is_none()).await.unwrap();
    let ended = tokio::time::timeout(Duration::from_secs(5), async {
        while stream.recv().await.is_ok() {}
    })
    .await;
    assert!(ended.is_ok(), "the stream ends with its module");
    let held = held.await.unwrap();
    assert!(
        matches!(
            &held,
            Err(Error::RemoteError(ErrorOwned {
                kind: ErrorKindOwned::Unavailable,
                ..
            }))
        ),
        "{held:?}"
    );
    let gone = raw(&cmd, &[HUB, API, ADD], add_args(1, 1)).await;
    assert!(matches!(kind_of(gone), Some(ErrorKindOwned::Unavailable)));
}

/// A reload with another API makes the client relearn that slot only: the view learns the new model through the
/// slot, the TV's own facts stay.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reload_relearns_only_its_slot() {
    let control = Arc::new(Mutex::new(Control::default()));
    let device = start("mount_module/reload", control.clone());
    let tv = connect(&device).await;
    let own = tv.cmd.device_api_info();
    control.lock().unwrap().next = Some(Some(hub_module().0));
    let view = tv.hub().unwrap().view().await.unwrap();
    let mut status = view.status();
    let first = status
        .wait_for(|p| hash_of(p).is_some())
        .await
        .unwrap()
        .clone();
    assert_eq!(
        view.commander().device_api_info().user_api_version.crate_id,
        "mount_backend_api"
    );

    control.lock().unwrap().next = Some(Some(Loaded::Tv(Box::new(TvModule::linked()))));
    let second = status
        .wait_for(|p| hash_of(p).is_some_and(|h| Some(&h) != hash_of(&first).as_ref()))
        .await
        .unwrap()
        .clone();
    assert_eq!(hash_of(&second), Some(tv_impl::API_HASH_NO_DOCS.to_vec()));
    let vcmd = view.commander();
    assert_eq!(
        vcmd.device_api_info().user_api_version.crate_id,
        "mounts_api"
    );
    assert!(
        vcmd.device_introspect().is_some(),
        "learned through the slot"
    );
    // the mounted TV's `mute`, id 3, through the view
    vcmd.prepare_call::<(), Error>(
        ww_client_server::PathKind::Absolute {
            path: wire_weaver::shrink_wrap::RefVec::Slice { slice: &path(&[3]) },
        },
        Ok(vec![]),
        "mute",
    )
    .call()
    .await
    .unwrap();
    let after = tv.cmd.device_api_info();
    assert_eq!(after.user_api_hash.no_docs, own.user_api_hash.no_docs);
    assert_eq!(after.user_api_version, own.user_api_version);
}

/// A module that misbehaves never reaches the TV's own tree: its garbage and stray replies are dropped and counted,
/// its unreadable answer is answered `ResponseSerFailed`, and when it fails it is unloaded while the TV goes on.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_misbehaving_module_never_reaches_the_parent() {
    let control = Arc::new(Mutex::new(Control::default()));
    let device = start("mount_module/broken", control.clone());
    let tv = connect(&device).await;
    let cmd = tv.cmd.clone();
    control.lock().unwrap().next = Some(Some(Loaded::Broken(Broken)));
    wait_presence(&tv, true).await;

    // the hub's `hang`, id 1: not `add`, at which the module fails
    let answer = raw(
        &cmd,
        &[HUB, API, 1],
        RequestKindOwned::Call {
            args: wire_weaver::shrink_wrap::tail_bytes::TailBytesOwned(vec![]),
        },
    )
    .await;
    assert!(matches!(
        kind_of(answer),
        Some(ErrorKindOwned::ResponseSerFailed)
    ));
    tv.mute().call().await.unwrap();

    let failed = raw(&cmd, &[HUB, API, ADD], add_args(1, 1)).await;
    assert!(matches!(kind_of(failed), Some(ErrorKindOwned::Unavailable)));
    wait_presence(&tv, false).await;
    tv.mute().call().await.unwrap();
    let counters = control.lock().unwrap().counters;
    // `hang` brought a garbage event, a stray reply and an unreadable answer
    assert_eq!(
        (counters.malformed, counters.stray, counters.faults),
        (2, 1, 1)
    );
}

/// A global path to `trait_name` of `crate_version`'s crate, then `from_trait`.
fn global(crate_version: FullVersion<'_>, trait_name: &str, from_trait: &[u32]) -> PathKindOwned {
    PathKindOwned::GlobalFull {
        gid: crate_version.make_owned(),
        trait_name: trait_name.to_owned(),
        path_from_trait: path(from_trait),
    }
}

/// A global path the TV's own tree doesn't have reaches the trait of the module in the hub slot (SRV-82), by name
/// and by the module's root; an empty slot reaches nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn global_paths_reach_into_the_module() {
    let control = Arc::new(Mutex::new(Control::default()));
    let device = start("mount_module/global", control.clone());
    let tv = connect(&device).await;
    let cmd = tv.cmd.clone();
    let call = || RequestKindOwned::Call {
        args: wire_weaver::shrink_wrap::tail_bytes::TailBytesOwned(vec![]),
    };
    let lamp = global(mount_backend_api::HUB_FULL_GID, "Lamp", &[TOGGLE]);
    let empty = raw_at(&cmd, lamp.clone(), call()).await;
    assert!(
        matches!(kind_of(empty), Some(ErrorKindOwned::TraitNotFound)),
        "an empty slot"
    );

    control.lock().unwrap().next = Some(Some(hub_module().0));
    wait_presence(&tv, true).await;
    let toggled = raw_at(&cmd, lamp.clone(), call()).await.unwrap();
    assert!(bool::from_ww_bytes(&toggled).unwrap());
    let toggled = raw_at(&cmd, lamp.clone(), call()).await.unwrap();
    assert!(!bool::from_ww_bytes(&toggled).unwrap());
    let hub = global(mount_backend_api::HUB_FULL_GID, "Hub", &[ADD]);
    let sum = raw_at(&cmd, hub, add_args(2, 3)).await.unwrap();
    assert_eq!(u32::from_ww_bytes(&sum).unwrap(), 5);

    control.lock().unwrap().next = Some(None);
    wait_presence(&tv, false).await;
    let unloaded = raw_at(&cmd, lamp, call()).await;
    assert!(matches!(
        kind_of(unloaded),
        Some(ErrorKindOwned::TraitNotFound)
    ));
    assert_eq!(control.lock().unwrap().counters, Default::default());
}

/// The TV's own tree answers first (SRV-82): the generated `has_global_trait` knows the TV's traits, also with
/// `global_paths` off (introspection on), and none of a module's. A client resolves the TV's own global paths before
/// sending them, so this is where the order shows.
#[test]
fn own_traits_are_known_to_the_tv() {
    let at = |gid, trait_name| ww_client_server::PathKind::GlobalFull {
        gid,
        trait_name,
        path_from_trait: wire_weaver::shrink_wrap::RefVec::Slice { slice: &[] },
    };
    assert!(crate::tv::api_impl::has_global_trait(&at(
        mounts_api::TV_FULL_GID,
        "Tv"
    )));
    assert!(!crate::tv::api_impl::has_global_trait(&at(
        mount_backend_api::HUB_FULL_GID,
        "Lamp"
    )));
    let absolute = ww_client_server::PathKind::Absolute {
        path: wire_weaver::shrink_wrap::RefVec::Slice { slice: &[] },
    };
    assert!(!crate::tv::api_impl::has_global_trait(&absolute));
}
