//! Server and client of mount slots (SRV-76): the TV of `mounts_api` answers the presence of its hub and extension
//! slots, hands requests under a slot's `api` to the slot's handler with the slot's path taken off (introspection
//! below the root included), refuses an absent slot with `Unavailable` and a slot inside a multi request, and names
//! its extensions through `ext_keys`. The generated and dynamic clients reach the slots.

pub mod device;
pub mod forwarder;

#[cfg(test)]
mod tests {
    use crate::device::*;
    use std::sync::Mutex;
    use std::time::Duration;

    use tests_common::TestDevice;
    use tokio::sync::oneshot;
    use wire_weaver::prelude::*;
    use wire_weaver::shrink_wrap::BufReader;
    use wire_weaver::shrink_wrap::either_any_vec::EitherAnyVec;
    use wire_weaver::shrink_wrap::tail_bytes::TailBytesOwned;
    use wire_weaver::ww_api::mount::{MountNameOwned, MountPresenceOwned};
    use wire_weaver_client::internal::Command;
    use wire_weaver_client::{ClientConfig, Commander, DynClient, DynResource, Error};
    use ww_client_server::observe::ObserveRequestOwned;
    use ww_client_server::{
        ErrorKindOwned, ErrorOwned, MultiIndexOwned, PathKindOwned, RequestKindOwned, RequestOwned,
        ResourceIndex,
    };
    use ww_self::ApiItemKindOwned;

    /// Same wire format as the generated arguments of `index_of`.
    #[derive_shrink_wrap(owned)]
    struct IndexOfArgs {
        key: MountNameOwned,
    }

    // ids in the API: `hub` 0, `ext[]` 1, `ext_keys` 2, `mute` 3; a slot's level: `presence` 0, `api` 1; the keys
    // companion's level: `index_of` 0, `key[]` 1
    const HUB: u32 = 0;
    const EXT: u32 = 1;
    const EXT_KEYS: u32 = 2;
    const PRESENCE: u32 = 0;
    const API: u32 = 1;
    const INDEX_OF: u32 = 0;
    const KEY: u32 = 1;

    async fn connect(device: &TestDevice) -> DynClient {
        DynClient::from_config(
            ClientConfig::new()
                .in_process_path(device.path())
                .resolver(tests_common::resolver()),
        )
        .connect()
        .await
        .expect("connect")
    }

    fn resource_path(path: &[u32]) -> Vec<ResourceIndex> {
        path.iter()
            .copied()
            .map(|i| ResourceIndex::new(UNib32(i)))
            .collect()
    }

    fn request(path: &[u32], kind: RequestKindOwned) -> Vec<u8> {
        RequestOwned {
            seq: UVlq32Backfill(0),
            priority: ww_client_server::Priority::NORMAL.to_wire(),
            context: None,
            path_kind: PathKindOwned::Absolute {
                path: resource_path(path),
            },
            kind,
        }
        .to_ww_bytes_owned()
        .unwrap()
    }

    /// Send a request as is and wait for the answer: the value bytes, or the device's error.
    async fn raw(cmd: &Commander, path: &[u32], kind: RequestKindOwned) -> Result<Vec<u8>, Error> {
        let (tx, rx) = oneshot::channel();
        cmd.send(Command::SendMessage {
            bytes: request(path, kind),
            done_tx: Some((tx, Duration::from_secs(2))),
            trace: None,
        })
        .await?;
        rx.await.expect("event loop answers")
    }

    fn presence(bytes: &[u8]) -> MountPresenceOwned {
        MountPresenceOwned::from_ww_bytes_owned(bytes).unwrap()
    }

    fn assert_up(presence: &MountPresenceOwned) {
        let MountPresenceOwned::Up { api, hash, wire } = presence else {
            panic!("not mounted: {presence:?}");
        };
        let (major, minor, patch) = HUB_VERSION;
        assert_eq!(api.crate_id, HUB_CRATE);
        assert_eq!(
            (
                api.version.major.0,
                api.version.minor.0,
                api.version.patch.0
            ),
            (major, minor, patch)
        );
        assert_eq!(hash.no_docs.hash, HUB_HASH_NO_DOCS);
        assert_eq!(hash.with_docs.hash, HUB_HASH_WITH_DOCS);
        assert_eq!(*wire, Some(HUB_WIRE));
    }

    fn assert_refused(answer: Result<Vec<u8>, Error>, kind: ErrorKindOwned) {
        match answer {
            Err(Error::RemoteError(ErrorOwned { kind: got, .. }))
                if std::mem::discriminant(&got) == std::mem::discriminant(&kind) => {}
            other => panic!("expected {kind:?}, got {other:?}"),
        }
    }

    /// `presence` of the hub and of the extension slots, mounted and not.
    async fn presences(cmd: &Commander) {
        let hub = raw(cmd, &[HUB, PRESENCE], RequestKindOwned::Read).await;
        assert_up(&presence(&hub.unwrap()));
        let cast = raw(cmd, &[EXT, 0, PRESENCE], RequestKindOwned::Read).await;
        assert_up(&presence(&cast.unwrap()));
        let adwatch = raw(cmd, &[EXT, 2, PRESENCE], RequestKindOwned::Read).await;
        assert!(matches!(
            presence(&adwatch.unwrap()),
            MountPresenceOwned::Absent
        ));
        let free = raw(cmd, &[EXT, 1, PRESENCE], RequestKindOwned::Read).await;
        assert_refused(free, ErrorKindOwned::BadIndex);
    }

    /// A request under a slot's `api` reaches the slot's handler with the slot's part of the path taken off, the
    /// backend's reply reaches the client under the request's seq; an `Introspect` at `api` reaches it with an empty
    /// path.
    async fn forwarding(cmd: &Commander, slots: &Mutex<Slots>) {
        let answer = raw(cmd, &[HUB, API, 5, 7], RequestKindOwned::Read).await;
        assert_eq!(
            Vec::<u32>::from_ww_bytes_owned(&answer.unwrap()).unwrap(),
            [5, 7]
        );
        let answer = raw(cmd, &[EXT, 0, API, 3], RequestKindOwned::Read).await;
        assert_eq!(
            Vec::<u32>::from_ww_bytes_owned(&answer.unwrap()).unwrap(),
            [3]
        );

        // the device handles requests in order: the read's answer comes after the introspect and the observes were
        // handled; an observe under a slot is the mounted API's, not the front's own (SRV-78)
        let observe = || RequestKindOwned::Observe {
            cfg: Box::new(ObserveRequestOwned {
                entries: vec![],
                patch: true,
                budget: None,
            }),
        };
        for bytes in [
            request(&[HUB, API], RequestKindOwned::Introspect),
            request(&[HUB, API, 2], observe()),
            request(&[EXT, 0, API], observe()),
        ] {
            cmd.send(Command::SendMessage {
                bytes,
                done_tx: None,
                trace: None,
            })
            .await
            .unwrap();
        }
        raw(cmd, &[HUB, PRESENCE], RequestKindOwned::Read)
            .await
            .unwrap();

        let seen = slots.lock().unwrap().seen.clone();
        let seen_at = |slot, path: &[u32], introspect| Seen {
            slot,
            path: path.to_vec(),
            introspect,
        };
        assert_eq!(
            seen,
            [
                seen_at(None, &[5, 7], false),
                seen_at(Some(0), &[3], false),
                seen_at(None, &[], true),
                seen_at(None, &[2], false),
                seen_at(Some(0), &[], false),
            ]
        );
    }

    /// An absent slot answers `Unavailable`, a slot inside a multi request is refused before its handler.
    async fn refusals(cmd: &Commander, slots: &Mutex<Slots>) {
        let absent = raw(cmd, &[EXT, 2, API, 1], RequestKindOwned::Read).await;
        assert_refused(absent, ErrorKindOwned::Unavailable);
        assert_eq!(slots.lock().unwrap().seen.last().unwrap().slot, Some(2));

        let before = slots.lock().unwrap().seen.len();
        let multi = RequestKindOwned::MultiRead {
            multi_idx: MultiIndexOwned::List(resource_path(&[0])),
            in_each_array_id: None,
        };
        let answer = raw(cmd, &[HUB, API], multi).await.unwrap();
        let mut rd = BufReader::new(&answer);
        let items = EitherAnyVec::des_shrink_wrap(&mut rd).unwrap();
        let item: Result<(), ErrorOwned> =
            items.iter().next_owned::<ErrorOwned, ()>().unwrap().into();
        assert_refused(
            item.map(|()| vec![]).map_err(Error::RemoteError),
            ErrorKindOwned::OperationNotSupported,
        );
        assert_eq!(slots.lock().unwrap().seen.len(), before, "no handler call");
    }

    /// Extensions by name: `index_of`, `key[i]` and the valid indices.
    async fn names(cmd: &Commander) {
        let index_of = |name: &str| {
            let args = IndexOfArgs {
                key: MountNameOwned(MaxLen::new(name.to_owned()).unwrap()),
            }
            .to_ww_bytes_owned()
            .unwrap();
            raw(
                cmd,
                &[EXT_KEYS, INDEX_OF],
                RequestKindOwned::Call {
                    args: TailBytesOwned(args),
                },
            )
        };
        let found = index_of("cast").await.unwrap();
        assert_eq!(
            Option::<UNib32>::from_ww_bytes(&found).unwrap(),
            Some(UNib32(0))
        );
        let missing = index_of("radio").await.unwrap();
        assert_eq!(Option::<UNib32>::from_ww_bytes(&missing).unwrap(), None);

        let key = raw(cmd, &[EXT_KEYS, KEY, 2], RequestKindOwned::Read).await;
        let name = MountNameOwned::from_ww_bytes_owned(&key.unwrap()).unwrap();
        assert_eq!(name.0.as_str(), "adwatch");

        let indices = raw(cmd, &[EXT], RequestKindOwned::Read).await.unwrap();
        let indices = ValidIndicesOwned::from_ww_bytes_owned(&indices).unwrap();
        assert_eq!(indices.iter().collect::<Vec<_>>(), [0, 2]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn blocking_no_alloc_server() {
        let slots = slots();
        let device = start_blocking("mounts/blocking", slots.clone());
        let client = connect(&device).await;
        presences(client.cmd()).await;
        forwarding(client.cmd(), &slots).await;
        refusals(client.cmd(), &slots).await;
        names(client.cmd()).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn async_alloc_server() {
        let slots = slots();
        let device = start_async("mounts/async", slots.clone());
        let client = connect(&device).await;
        presences(client.cmd()).await;
        forwarding(client.cmd(), &slots).await;
        refusals(client.cmd(), &slots).await;
        names(client.cmd()).await;
    }

    /// The device's introspection data holds the slots' declarations only: the mounted APIs have their own.
    /// The hub forwards to a real backend, another TV with nothing mounted (SRV-77): the request reaches it with the
    /// slot's path taken off, its replies and errors reach the client under the request's seq.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn forward_to_a_backend() {
        const MUTE: u32 = 3;
        let slots = slots();
        slots.lock().unwrap().hub_backend = Some(tv_backend());
        let device = start_blocking("mounts/forward", slots.clone());
        let client = connect(&device).await;
        let cmd = client.cmd();
        let muted = raw(cmd, &[HUB, API, MUTE], call()).await;
        assert_eq!(muted.unwrap(), Vec::<u8>::new());
        // the backend's own hub slot: nothing mounted there
        let inner = raw(cmd, &[HUB, API, HUB, PRESENCE], RequestKindOwned::Read).await;
        assert!(matches!(
            presence(&inner.unwrap()),
            MountPresenceOwned::Absent
        ));
        let absent = raw(cmd, &[HUB, API, HUB, API, MUTE], call()).await;
        assert_refused(absent, ErrorKindOwned::Unavailable);
        let missing = raw(cmd, &[HUB, API, 9], RequestKindOwned::Read).await;
        assert_refused(missing, ErrorKindOwned::BadPath);
        // the front's own items stay its own
        assert_eq!(raw(cmd, &[MUTE], call()).await.unwrap(), Vec::<u8>::new());
    }

    fn call() -> RequestKindOwned {
        RequestKindOwned::Call {
            args: TailBytesOwned(vec![]),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn bundle_has_declarations_only() {
        let device = start_blocking("mounts/bundle", slots());
        let client = connect(&device).await;
        let bundle = &client.device_introspect().unwrap().api_bundle;
        let root = DynResource::root(bundle.clone());
        let wire_weaver_client::DynResourceKind::Level(level) = root.kind().unwrap() else {
            panic!("the root is a level");
        };
        let items: Vec<_> = level.items.iter().map(|i| i.ident.as_str()).collect();
        assert_eq!(items, ["hub", "ext", "ext_keys", "mute"]);
        let hub = &level.items[0];
        assert!(
            matches!(&hub.kind, ApiItemKindOwned::Mount { expects: Some(c), .. } if c == HUB_CRATE),
            "{:?}",
            hub.kind
        );
        assert!(matches!(
            &level.items[1].kind,
            ApiItemKindOwned::Mount { expects: None, .. }
        ));
    }

    /// The dynamic client reads a slot's `presence` by path; the mounted API is not in the bundle.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dynamic_client() {
        let device = start_blocking("mounts/dynamic", slots());
        let client = connect(&device).await;
        let cmd = client.cmd();
        let root = DynResource::root(client.device_introspect().unwrap().api_bundle.clone());
        root.walk("hub.presence").unwrap().read(cmd).await.unwrap();
        root.walk("ext[2].presence")
            .unwrap()
            .read(cmd)
            .await
            .unwrap();
        let api = root.walk("hub.api").unwrap_err().to_string();
        assert!(api.contains("run time"), "{api}");
    }

    mod std_client {
        use wire_weaver_client::{ClientConfig, Commander, WwClient};

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
            wire_weaver::ww_codegen!(
                mounts_api :: Tv for super::Client,
                client = "std_client",
                debug_to_file = "../../target/tests_mounts_client.rs"
            );
        }
    }

    /// The generated client hands out slots by accessor, by index and by name, with the path of the mounted API.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn generated_client() {
        let device = start_async("mounts/client", slots());
        let client = std_client::Client::config(|c| {
            c.in_process_path(device.path())
                .resolver(tests_common::resolver())
        })
        .connect()
        .await
        .expect("connect");

        let hub = client.hub().unwrap();
        assert_up(&hub.presence().read().await.unwrap());
        let PathKindOwned::Absolute { path } = hub.api_path() else {
            panic!("absolute path");
        };
        assert_eq!(path, resource_path(&[HUB, API]));

        assert_up(&client.ext(0).unwrap().presence().read().await.unwrap());
        let name = MountNameOwned(MaxLen::new("adwatch".to_owned()).unwrap());
        let adwatch = client.ext_by(&name).await.unwrap().expect("named slot");
        assert!(matches!(
            adwatch.presence().read().await.unwrap(),
            MountPresenceOwned::Absent
        ));
        let PathKindOwned::Absolute { path } = adwatch.api_path() else {
            panic!("absolute path");
        };
        assert_eq!(path, resource_path(&[EXT, 2, API]));
        let radio = MountNameOwned(MaxLen::new("radio".to_owned()).unwrap());
        assert!(client.ext_by(&radio).await.unwrap().is_none());
    }

    /// The blocking generated client.
    #[test]
    fn generated_client_blocking() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let device = start_blocking("mounts/client_blocking", slots());
        let client = std_client::Client::config(|c| {
            c.in_process_path(device.path())
                .resolver(tests_common::resolver())
        })
        .connect_blocking()
        .expect("connect");
        assert_up(&client.hub().unwrap().presence().blocking_read().unwrap());
        let cast = MountNameOwned(MaxLen::new("cast".to_owned()).unwrap());
        let slot = client.ext_by_blocking(&cast).unwrap().expect("named slot");
        assert_up(&slot.presence().blocking_read().unwrap());
    }
}
