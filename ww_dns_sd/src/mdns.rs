//! [MdnsResponder]: the in-process responder, the `mdns-sd` crate on its own thread.

use std::collections::HashMap;
use std::time::Duration;

use mdns_sd::{ServiceDaemon, ServiceInfo, TxtProperty};

use crate::{Error, InstanceName, Responder, SERVICE_TYPE, ServiceRecord};

/// The standard mDNS port.
pub const MDNS_PORT: u16 = 5353;

/// How long a withdraw or shutdown waits for the responder's thread to confirm.
const CONFIRM_TIMEOUT: Duration = Duration::from_secs(2);

/// Answers mDNS queries for the records it publishes, in this process: no Avahi or Bonjour needed. Joins the mDNS
/// group on every interface, and announces each address only on the interface whose subnet holds it.
pub struct MdnsResponder {
    daemon: ServiceDaemon,
    /// Instance → full DNS-SD name of what is published
    published: HashMap<InstanceName, String>,
}

impl MdnsResponder {
    /// Starts the responder's thread on the standard port, 5353.
    pub fn new() -> Result<Self, Error> {
        Self::with_port(MDNS_PORT)
    }

    /// Starts the responder's thread on `port`: only browsers on the same port see it, which keeps tests off the
    /// LAN's mDNS.
    pub fn with_port(port: u16) -> Result<Self, Error> {
        let daemon = ServiceDaemon::new_with_port(port)?;
        Ok(Self {
            daemon,
            published: HashMap::new(),
        })
    }

    /// The `mdns-sd` daemon, to restrict its interfaces or to browse with it.
    pub fn daemon(&self) -> &ServiceDaemon {
        &self.daemon
    }

    fn unregister(&mut self, fullname: &str) -> Result<(), Error> {
        let status = self.daemon.unregister(fullname)?;
        match status.recv_timeout(CONFIRM_TIMEOUT) {
            Ok(_) => Ok(()),
            Err(_) => Err(Error::ResponderGone),
        }
    }
}

/// `mdns-sd`'s service type, `_wireweaver._tcp.local.`.
fn ty_domain() -> String {
    format!("{SERVICE_TYPE}.local.")
}

impl Responder for MdnsResponder {
    fn publish(&mut self, record: &ServiceRecord) -> Result<(), Error> {
        let properties: Vec<TxtProperty> = record
            .txt()
            .entries()
            .map(|(k, v)| TxtProperty::from((k, v)))
            .collect();
        // mdns-sd announces each address only on the interface whose subnet holds it
        let mut info = ServiceInfo::new(
            &ty_domain(),
            record.instance().as_str(),
            &format!("{}.", record.host_name()),
            record.addresses(),
            record.port().get(),
            properties,
        )?;
        // An update: a goodbye for the old records, so that browsers drop the old addresses a second later (a
        // cache-flush announcement alone flushes only records older than a second), then the new ones. The
        // service itself stays: its PTR, SRV and TXT come back before the goodbye's second is over. No probing
        // for a name already ours (RFC 6762 §8.4): the probe would take the old records still in flight for a
        // conflict and rename the host.
        if let Some(old) = self.published.remove(record.instance()) {
            self.unregister(&old)?;
            info.set_requires_probe(false);
        }
        let fullname = info.get_fullname().to_owned();
        self.daemon.register(info)?;
        self.published.insert(record.instance().clone(), fullname);
        Ok(())
    }

    fn withdraw(&mut self, instance: &InstanceName) -> Result<(), Error> {
        match self.published.remove(instance) {
            Some(fullname) => self.unregister(&fullname),
            None => Ok(()),
        }
    }
}

impl Drop for MdnsResponder {
    fn drop(&mut self) {
        let names: Vec<String> = self.published.drain().map(|(_, n)| n).collect();
        for name in names {
            if let Err(e) = self.unregister(&name) {
                tracing::warn!("withdrawing {name} failed: {e}");
            }
        }
        match self.daemon.shutdown() {
            Ok(done) => {
                if done.recv_timeout(CONFIRM_TIMEOUT).is_err() {
                    tracing::warn!("the mDNS responder's thread did not confirm its shutdown");
                }
            }
            Err(e) => tracing::warn!("stopping the mDNS responder failed: {e}"),
        }
    }
}
