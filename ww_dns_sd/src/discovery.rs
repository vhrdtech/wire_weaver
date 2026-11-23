//! [Discovery]: the `_wireweaver._tcp` servers on the LAN, kept current by a [Browser] on a thread of its own as
//! servers come, change address and go. `wire_weaver_client`'s `discovery::urls` turns one of them into the URLs
//! of each connect attempt.
//!
//! On Android an app must hold a `WifiManager.MulticastLock` while discovery runs, or the Wi-Fi driver drops the
//! multicast answers.
//!
//! What the LAN announces is unauthenticated: any device can announce any name. Link setup still checks the API.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::{Browsed, Browser, Error, FoundServer, MdnsOptions};

/// Servers kept at most; more found are left out until some go.
pub const SERVERS_MAX: usize = 256;

/// Callbacks [Discovery::on_change] keeps at most.
pub const CALLBACKS_MAX: usize = 16;

/// How often the browsing thread looks at the stop flag.
const STOP_POLL: Duration = Duration::from_millis(200);

/// Which found server [Discovery::find] picks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Select {
    /// The instance name, `Living room TV`.
    Name(String),
    /// The TXT key `id`: stays the same across renames.
    Id(String),
    /// The first server, by name, that serves this API crate (`ww_niri_api`).
    Api(String),
}

impl Select {
    /// Whether `server` is the one selected.
    pub fn matches(&self, server: &FoundServer) -> bool {
        match self {
            Self::Name(name) => server.instance() == name,
            Self::Id(id) => server.id().is_some_and(|i| i.as_str() == id),
            Self::Api(api) => server.api_crate() == Some(api),
        }
    }
}

impl std::fmt::Display for Select {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Name(name) => write!(f, "{name}"),
            Self::Id(id) => write!(f, "id {id}"),
            Self::Api(api) => write!(f, "API {api}"),
        }
    }
}

type Callback = Box<dyn Fn() + Send + Sync>;

struct Shared {
    servers: Mutex<Arc<[FoundServer]>>,
    found: Mutex<BTreeMap<String, FoundServer>>,
    callbacks: Mutex<Vec<Callback>>,
    stop: AtomicBool,
}

impl Shared {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            servers: Mutex::new(Arc::from([])),
            found: Mutex::new(BTreeMap::new()),
            callbacks: Mutex::new(Vec::new()),
            stop: AtomicBool::new(false),
        })
    }

    fn apply(&self, change: Browsed) {
        let list: Arc<[FoundServer]> = {
            let mut found = lock(&self.found);
            let changed = match change {
                Browsed::Found(server) => {
                    let known = found.contains_key(server.instance());
                    if !known && found.len() >= SERVERS_MAX {
                        tracing::debug!(
                            name = server.instance(),
                            "left out: {SERVERS_MAX} servers known"
                        );
                        false
                    } else {
                        found.insert(server.instance().to_owned(), server.clone()) != Some(server)
                    }
                }
                Browsed::Gone(name) => found.remove(&name).is_some(),
            };
            if !changed {
                return;
            }
            found.values().cloned().collect()
        };
        *lock(&self.servers) = list;
        for callback in lock(&self.callbacks).iter() {
            callback();
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock()
        .expect("discovery's locks are never held across a panic")
}

/// Stops the browsing thread when the last [Discovery] clone goes.
struct Stopper {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Stopper {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::warn!("the DNS-SD browsing thread panicked");
        }
    }
}

/// The `_wireweaver._tcp` servers on the LAN, kept current by a thread of its own. Cheap to clone; browsing stops
/// when the last clone is dropped.
#[derive(Clone)]
pub struct Discovery {
    shared: Arc<Shared>,
    _stopper: Arc<Stopper>,
}

impl Discovery {
    /// Browses the LAN on every interface, port 5353.
    pub fn start() -> Result<Self, Error> {
        Self::start_with(&MdnsOptions::default())
    }

    /// Browses with `options`: a port and interfaces of its own in tests.
    pub fn start_with(options: &MdnsOptions) -> Result<Self, Error> {
        let browser = Browser::with_options(options)?;
        let shared = Shared::new();
        let thread = {
            let shared = shared.clone();
            std::thread::Builder::new()
                .name("ww-dns-sd".to_owned())
                .spawn(move || browse(&browser, &shared))
                .map_err(Error::Thread)?
        };
        Ok(Self {
            _stopper: Arc::new(Stopper {
                shared: shared.clone(),
                thread: Some(thread),
            }),
            shared,
        })
    }

    /// A discovery the caller feeds through the returned [Feed] instead of the LAN: servers known some other way,
    /// or a test's.
    pub fn fed() -> (Self, Feed) {
        let shared = Shared::new();
        let discovery = Self {
            _stopper: Arc::new(Stopper {
                shared: shared.clone(),
                thread: None,
            }),
            shared: shared.clone(),
        };
        (discovery, Feed { shared })
    }

    /// The servers found, by instance name.
    pub fn servers(&self) -> Arc<[FoundServer]> {
        lock(&self.shared.servers).clone()
    }

    /// Calls `callback` from the browsing thread after every change of [Self::servers]: an egui repaint, a wake-up
    /// of a task. It must not call [Self::on_change].
    /// Keeps at most [CALLBACKS_MAX]; returns `false` and drops `callback` when full.
    pub fn on_change(&self, callback: impl Fn() + Send + Sync + 'static) -> bool {
        let mut callbacks = lock(&self.shared.callbacks);
        if callbacks.len() >= CALLBACKS_MAX {
            return false;
        }
        callbacks.push(Box::new(callback));
        true
    }

    /// The server `select` picks now, first by name.
    pub fn find(&self, select: &Select) -> Option<FoundServer> {
        self.servers().iter().find(|s| select.matches(s)).cloned()
    }
}

/// Hands servers to a [Discovery::fed] discovery.
pub struct Feed {
    shared: Arc<Shared>,
}

impl Feed {
    /// A server appeared or changed.
    pub fn found(&self, server: FoundServer) {
        self.shared.apply(Browsed::Found(server));
    }

    /// The server of this instance name went.
    pub fn gone(&self, name: &str) {
        self.shared.apply(Browsed::Gone(name.to_owned()));
    }
}

fn browse(browser: &Browser, shared: &Shared) {
    while !shared.stop.load(Ordering::Relaxed) {
        match browser.next(STOP_POLL) {
            Ok(Some(change)) => shared.apply(change),
            Ok(None) => {}
            Err(e) => {
                tracing::warn!("DNS-SD discovery stopped: {e}");
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;
    use std::num::NonZeroU16;
    use std::sync::atomic::AtomicUsize;

    use super::*;

    fn tv(name: &str, ip: &str) -> FoundServer {
        let ip: IpAddr = ip.parse().unwrap();
        FoundServer::new(name, NonZeroU16::new(7590).unwrap(), "/ww", [ip]).with_txt(
            Some("ww_niri_api@0.3.1".to_owned()),
            None,
            Some(crate::InstanceId::new("tv-1").unwrap()),
        )
    }

    #[test]
    fn a_fed_discovery_keeps_the_list_and_calls_back() {
        let (discovery, feed) = Discovery::fed();
        let calls = Arc::new(AtomicUsize::new(0));
        let counted = calls.clone();
        assert!(discovery.on_change(move || {
            counted.fetch_add(1, Ordering::Relaxed);
        }));
        feed.found(tv("Living room TV", "192.168.1.5"));
        feed.found(tv("Living room TV", "192.168.1.5"));
        assert_eq!(
            calls.load(Ordering::Relaxed),
            1,
            "the same server again is no change"
        );
        feed.found(tv("Kitchen TV", "192.168.1.6"));
        let servers = discovery.servers();
        let names: Vec<&str> = servers.iter().map(FoundServer::instance).collect();
        assert_eq!(names, ["Kitchen TV", "Living room TV"]);
        feed.gone("Kitchen TV");
        assert_eq!(discovery.servers().len(), 1);
        assert_eq!(calls.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn servers_are_selected_by_name_id_or_api() {
        let (discovery, feed) = Discovery::fed();
        feed.found(tv("Living room TV", "192.168.1.5"));
        for select in [
            Select::Name("Living room TV".to_owned()),
            Select::Id("tv-1".to_owned()),
            Select::Api("ww_niri_api".to_owned()),
        ] {
            assert_eq!(
                discovery
                    .find(&select)
                    .map(|s| s.instance().to_owned())
                    .as_deref(),
                Some("Living room TV"),
                "{select}"
            );
        }
        assert!(
            discovery
                .find(&Select::Name("Kitchen TV".to_owned()))
                .is_none()
        );
    }
}
