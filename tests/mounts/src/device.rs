//! The TV of the mount tests: a hub slot and named extension slots, with blocking `no_alloc` and async servers. A
//! slot's backend is a stand-in: a request under a mounted slot is recorded and answered with the path it arrived
//! with, as a mount forwarder answers with the backend's reply. With a [Backend] the hub forwards for real: the
//! request rewritten below the slot, the backend's events rewritten back (SRV-77).

use std::sync::{Arc, Mutex};
use tests_common::TestDevice;
use wire_weaver::prelude::*;
use wire_weaver::ww_api::mount::{MountName, MountPresence};
use wire_weaver::ww_version::{ApiHash, ApiHashPair, Version, WireVersion};
use ww_client_server::ResourceIndex;
use ww_client_server::mount::{MountAnswer, MountPrefix, MountRequest};

/// Crate, version, hashes and wire version of the API mounted at a slot.
pub const HUB_CRATE: &str = "hub_api";
pub const HUB_VERSION: (u32, u32, u32) = (0, 3, 1);
pub const HUB_HASH_NO_DOCS: [u8; 4] = [1, 2, 3, 4];
pub const HUB_HASH_WITH_DOCS: [u8; 2] = [5, 6];
pub const HUB_WIRE: WireVersion = WireVersion::new(1, 2);

/// A request that reached a slot's handler: the slot (`None`: the hub, `Some(i)`: `ext[i]`), the path below the
/// slot's `api`, and whether it was an `Introspect`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    pub slot: Option<u32>,
    pub path: Vec<u32>,
    pub introspect: bool,
}

/// A backend the hub forwards to: the request as the backend gets it in, the events it answers with out.
pub type Backend = Box<dyn FnMut(&[u8]) -> Vec<Vec<u8>> + Send>;

/// What is mounted where.
#[derive(Default)]
pub struct Slots {
    /// The hub is mounted.
    pub hub: bool,
    /// Extensions by index: name, mounted; `None` is a free index.
    pub ext: Vec<Option<(String, bool)>>,
    pub seen: Vec<Seen>,
    /// The hub's backend; `None`: the stand-in answers with the path.
    pub hub_backend: Option<Backend>,
    /// Another revision of the hub's backend: its presence names other hashes (the async server only).
    pub hub_revision: u8,
    /// The hub's presence changed: the async server tells observers at its next poll.
    pub hub_changed: bool,
}

impl Slots {
    fn indices(&self) -> Vec<UNib32> {
        (0u32..)
            .zip(&self.ext)
            .filter(|(_, e)| e.is_some())
            .map(|(i, _)| UNib32(i))
            .collect()
    }

    fn ext(&self, index_chain: [ResourceIndex; 1]) -> &(String, bool) {
        self.ext[index_chain[0].get().0 as usize]
            .as_ref()
            .expect("valid indices are the occupied ones")
    }

    /// Records the request and says whether the slot forwards it.
    fn route(&mut self, slot: Option<u32>, request: &MountRequest<'_, '_>) -> bool {
        let mounted = match slot {
            None => self.hub,
            Some(i) => self.ext[i as usize].as_ref().is_some_and(|(_, m)| *m),
        };
        self.seen.push(Seen {
            slot,
            path: request.path.iter().map(|i| i.get().0).collect(),
            introspect: matches!(
                request.request.kind,
                ww_client_server::RequestKind::Introspect
            ),
        });
        mounted
    }

    /// The events the hub's backend answers `request` with, as the front's client gets them; `None` without a
    /// backend.
    fn forward(&mut self, request: &MountRequest<'_, '_>) -> Option<Vec<Vec<u8>>> {
        let backend = self.hub_backend.as_mut()?;
        let mut buf = [0u8; 512];
        let bytes = request.forward_into(&mut buf).expect("the request fits");
        let mut prefix = MountPrefix::from_vec(request.prefix().expect("an absolute path"));
        let events = backend(bytes)
            .iter()
            .map(|event| {
                let mut out = [0u8; 512];
                prefix
                    .event_out(event, &mut out)
                    .expect("the event rewrites")
                    .to_vec()
            })
            .collect();
        Some(events)
    }
}

/// A blocking server's events, collected.
#[derive(Default)]
pub struct Collect(pub Vec<Vec<u8>>);

impl BlockingEventOut for Collect {
    fn send_with_blocking<F>(&mut self, ser: F) -> Result<(), SendError>
    where
        F: FnOnce(&mut [u8]) -> Result<usize, ShrinkWrapError>,
    {
        let mut scratch = [0u8; 512];
        let len = ser(&mut scratch).map_err(SendError::Serialize)?;
        self.0.push(scratch[..len].to_vec());
        Ok(())
    }
}

/// A TV with nothing mounted as the hub's backend, served by the blocking server with introspection on: the hub's
/// `presence` then names its API.
pub fn tv_backend() -> Backend {
    let mut tv = backend_server::Server {
        slots: Arc::default(),
        valid: vec![],
        observe: backend_server::api_impl::ObserveState::new(Default::default()),
    };
    Box::new(move |request| {
        let mut out = Collect::default();
        let mut scratch = [0u8; 512];
        let reply = tv
            .process_request_bytes(request, &mut scratch, &mut out, ())
            .expect("the backend answers");
        if !reply.is_empty() {
            out.0.push(reply.to_vec());
        }
        out.0
    })
}

fn up() -> MountPresence<'static> {
    let (major, minor, patch) = HUB_VERSION;
    MountPresence::Up {
        api: FullVersion::new(HUB_CRATE, Version::new(major, minor, patch)),
        hash: ApiHashPair {
            no_docs: ApiHash {
                hash: RefVec::Slice {
                    slice: &HUB_HASH_NO_DOCS,
                },
            },
            with_docs: ApiHash {
                hash: RefVec::Slice {
                    slice: &HUB_HASH_WITH_DOCS,
                },
            },
        },
        wire: Some(HUB_WIRE),
    }
}

/// The presence of the hub when [tv_backend] is mounted there: its crate, version, hashes and wire version.
fn backend_up() -> MountPresence<'static> {
    MountPresence::Up {
        api: mounts_api::TV_FULL_GID,
        hash: backend_server::api_impl::api_hash(),
        wire: Some(mounts_api::TV_WIRE_VERSION),
    }
}

/// The answer of the stand-in backend: the path the request arrived with, as a value under the request's seq.
fn reply(scratch: &mut [u8], request: &MountRequest<'_, '_>) -> Result<usize, ShrinkWrapError> {
    let path: Vec<u32> = request.path.iter().map(|i| i.get().0).collect();
    let data = path.to_ww_bytes_owned()?;
    ww_client_server::util::ser_value_event(scratch, request.request.seq.0, &data).map(<[u8]>::len)
}

/// Same handlers for the blocking and async servers: `$($async)?`, the event out trait, how an event is sent and
/// how a presence and a name are made differ.
macro_rules! handlers {
    ($($async:ident)?; $out:path; $send:ident; $presence:path; $name:path; $($await:tt)*) => {
        $($async)? fn get_hub_presence(
            &mut self,
            _cx: &mut Context<'_, impl $out>,
        ) -> GetResult<$presence, CommonError<'_>> {
            let slots = self.slots.lock().unwrap();
            if slots.hub && slots.hub_backend.is_some() {
                return Value(backend_presence(slots.hub_revision));
            }
            Value(presence(slots.hub))
        }

        $($async)? fn hub_request(
            &mut self,
            cx: &mut Context<'_, impl $out>,
            request: MountRequest<'_, '_>,
        ) -> MountAnswer {
            let forwarded = {
                let mut slots = self.slots.lock().unwrap();
                if !slots.route(None, &request) {
                    return MountAnswer::Unavailable;
                }
                slots.forward(&request)
            };
            match forwarded {
                Some(events) => {
                    for event in events {
                        cx.$send(|scratch| {
                            scratch[..event.len()].copy_from_slice(&event);
                            Ok(event.len())
                        })$($await)*.expect("event sent");
                    }
                }
                None => cx.$send(|scratch| reply(scratch, &request))$($await)*.expect("reply sent"),
            }
            MountAnswer::Forwarded
        }

        fn valid_indices_root_ext(&mut self) -> ValidIndices<'_> {
            self.valid = self.slots.lock().unwrap().indices();
            ValidIndices::List(RefVec::Slice { slice: &self.valid })
        }

        $($async)? fn get_ext_presence(
            &mut self,
            _cx: &mut Context<'_, impl $out>,
            index_chain: [ResourceIndex; 1],
        ) -> GetResult<$presence, CommonError<'_>> {
            let mounted = self.slots.lock().unwrap().ext(index_chain).1;
            Value(presence(mounted))
        }

        $($async)? fn ext_request(
            &mut self,
            cx: &mut Context<'_, impl $out>,
            index_chain: [ResourceIndex; 1],
            request: MountRequest<'_, '_>,
        ) -> MountAnswer {
            if !self.slots.lock().unwrap().route(Some(index_chain[0].get().0), &request) {
                return MountAnswer::Unavailable;
            }
            cx.$send(|scratch| reply(scratch, &request))$($await)*.expect("reply sent");
            MountAnswer::Forwarded
        }

        $($async)? fn ext_index_of(
            &mut self,
            _cx: &mut Context<'_, impl $out>,
            key: $name,
        ) -> RpcResult<Option<UNib32>> {
            let slots = self.slots.lock().unwrap();
            let found = (0u32..)
                .zip(&slots.ext)
                .find(|(_, e)| e.as_ref().is_some_and(|(n, _)| n.as_str() == &**key.0));
            Ready(found.map(|(i, _)| UNib32(i)))
        }

        $($async)? fn get_ext_key(
            &mut self,
            _cx: &mut Context<'_, impl $out>,
            index_chain: [ResourceIndex; 1],
        ) -> GetResult<$name, CommonError<'_>> {
            let name = self.slots.lock().unwrap().ext(index_chain).0.clone();
            Value(name_of(name))
        }

        $($async)? fn mute(&mut self, _cx: &mut Context<'_, impl $out>) -> RpcResult<()> {
            Ready(())
        }

        fn observe_state(&mut self) -> &mut api_impl::ObserveState {
            &mut self.observe
        }
    };
}

pub mod blocking_server {
    use super::*;
    use tests_common::TestProcessEvents;

    pub struct Server {
        pub slots: Arc<Mutex<Slots>>,
        pub valid: Vec<UNib32>,
        pub observe: api_impl::ObserveState,
    }

    fn presence(mounted: bool) -> MountPresence<'static> {
        if mounted { up() } else { MountPresence::Absent }
    }

    fn backend_presence(_revision: u8) -> MountPresence<'static> {
        backend_up()
    }

    fn name_of(name: String) -> MountName<'static> {
        let name: &'static str = Box::leak(name.into_boxed_str());
        MountName(MaxLen::new(name).expect("test names are short"))
    }

    impl Server {
        handlers!(; BlockingEventOut; send_with_blocking; MountPresence<'_>; MountName<'_>;);
    }

    pub mod api_impl {
        wire_weaver::ww_codegen!(
            mounts_api :: Tv for super::Server,
            server = true, no_alloc = true, use_async = false,
            method_model = "_=immediate",
            property_model = "_=get_set",
            multi_req = true,
            debug_to_file = "../../target/tests_mounts_server.rs"
        );
    }

    impl TestProcessEvents for Server {
        type Medium = ();

        fn process_request_bytes<'a>(
            &mut self,
            bytes: &[u8],
            scratch: &'a mut [u8],
            out: &mut impl BlockingEventOut,
            medium: (),
        ) -> Result<&'a [u8], ShrinkWrapError> {
            self.process_request_bytes(bytes, scratch, out, medium)
        }
    }
}

/// The hub's real backend in [tv_backend]: the blocking server with introspection on.
pub mod backend_server {
    use super::*;

    pub struct Server {
        pub slots: Arc<Mutex<Slots>>,
        pub valid: Vec<UNib32>,
        pub observe: api_impl::ObserveState,
    }

    fn presence(mounted: bool) -> MountPresence<'static> {
        if mounted { up() } else { MountPresence::Absent }
    }

    fn backend_presence(_revision: u8) -> MountPresence<'static> {
        backend_up()
    }

    fn name_of(name: String) -> MountName<'static> {
        let name: &'static str = Box::leak(name.into_boxed_str());
        MountName(MaxLen::new(name).expect("test names are short"))
    }

    impl Server {
        handlers!(; BlockingEventOut; send_with_blocking; MountPresence<'_>; MountName<'_>;);
    }

    pub mod api_impl {
        wire_weaver::ww_codegen!(
            mounts_api :: Tv for super::Server,
            server = true, no_alloc = true, use_async = false,
            method_model = "_=immediate",
            property_model = "_=get_set",
            multi_req = true,
            introspect = "no_docs"
        );
    }
}

pub mod async_server {
    use super::*;
    use tests_common::TestProcessEventsAsync;
    use wire_weaver::ww_api::mount::{MountNameOwned, MountPresenceOwned};

    pub struct Server {
        pub slots: Arc<Mutex<Slots>>,
        pub valid: Vec<UNib32>,
        pub observe: api_impl::ObserveState,
        /// The device's clock starts here.
        pub started: std::time::Instant,
    }

    fn presence(mounted: bool) -> MountPresenceOwned {
        if mounted {
            owned(up())
        } else {
            MountPresenceOwned::Absent
        }
    }

    fn backend_presence(revision: u8) -> MountPresenceOwned {
        let mut presence = owned(backend_up());
        if let MountPresenceOwned::Up { hash, .. } = &mut presence
            && let Some(last) = hash.no_docs.hash.last_mut()
        {
            *last ^= revision;
        }
        presence
    }

    fn owned(presence: MountPresence<'_>) -> MountPresenceOwned {
        let mut buf = [0u8; 256];
        let bytes = presence.to_ww_bytes(&mut buf).expect("presence encodes");
        MountPresenceOwned::from_ww_bytes_owned(bytes).expect("presence decodes")
    }

    fn name_of(name: String) -> MountNameOwned {
        MountNameOwned(MaxLen::new(name).expect("test names are short"))
    }

    impl Server {
        handlers!(async; EventOut; send_with; MountPresenceOwned; MountNameOwned; .await);
    }

    pub mod api_impl {
        wire_weaver::ww_codegen!(
            mounts_api :: Tv for super::Server,
            server = true, no_alloc = false, use_async = true,
            method_model = "_=immediate",
            property_model = "_=get_set",
            multi_req = true
        );
    }

    impl TestProcessEventsAsync for Server {
        type Medium = ();

        async fn process_request_bytes<'a>(
            &mut self,
            bytes: &[u8],
            scratch: &'a mut [u8],
            out: &mut impl EventOut,
            medium: (),
        ) -> Result<&'a [u8], ShrinkWrapError> {
            self.process_request_bytes(bytes, scratch, out, medium)
                .await
        }

        /// Tells observers of the hub's presence when the test changed it.
        async fn poll(&mut self, out: &mut impl EventOut, _now_ms: u64) {
            let changed = std::mem::take(&mut self.slots.lock().unwrap().hub_changed);
            if changed {
                self.mark_changed_hub_presence();
            }
            let now_us = self.started.elapsed().as_micros() as u64;
            let mut buf = [0u8; 512];
            self.poll_properties(out, (), now_us, &mut buf)
                .await
                .expect("updates sent");
        }
    }
}

/// The hub mounted, two extensions at indices 0 and 2 (1 is free), only `cast` mounted.
pub fn slots() -> Arc<Mutex<Slots>> {
    Arc::new(Mutex::new(Slots {
        hub: true,
        ext: vec![
            Some(("cast".to_owned(), true)),
            None,
            Some(("adwatch".to_owned(), false)),
        ],
        seen: vec![],
        hub_backend: None,
        hub_revision: 0,
        hub_changed: false,
    }))
}

pub fn start_blocking(path: &str, slots: Arc<Mutex<Slots>>) -> TestDevice {
    let _ = tracing_subscriber::fmt::try_init();
    let server = blocking_server::Server {
        slots,
        valid: vec![],
        observe: blocking_server::api_impl::ObserveState::new(Default::default()),
    };
    tests_common::start_device(
        path,
        server,
        mounts_api::TV_FULL_GID,
        mounts_api::TV_WIRE_VERSION,
        tests_common::api!(blocking_server::api_impl),
    )
}

pub fn start_async(path: &str, slots: Arc<Mutex<Slots>>) -> TestDevice {
    let _ = tracing_subscriber::fmt::try_init();
    let server = async_server::Server {
        slots,
        valid: vec![],
        observe: async_server::api_impl::ObserveState::new(Default::default()),
        started: std::time::Instant::now(),
    };
    tests_common::start_device_async(
        path,
        server,
        mounts_api::TV_FULL_GID,
        mounts_api::TV_WIRE_VERSION,
        tests_common::api!(async_server::api_impl),
    )
}

/// [start_async] on the simulated WebSocket: a device that can go away and come back under the same path.
pub fn start_async_ws(path: &str, slots: Arc<Mutex<Slots>>) -> TestDevice {
    let _ = tracing_subscriber::fmt::try_init();
    let server = async_server::Server {
        slots,
        valid: vec![],
        observe: async_server::api_impl::ObserveState::new(Default::default()),
        started: std::time::Instant::now(),
    };
    tests_common::start_device_async_ws(
        path,
        server,
        mounts_api::TV_FULL_GID,
        mounts_api::TV_WIRE_VERSION,
        tests_common::api!(async_server::api_impl),
    )
}
