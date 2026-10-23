mod connect;
mod event_loop;
pub(crate) use event_loop::{RttTransport, Rx, Tx};

pub(crate) use connect::{try_connect, try_connect_routed};

/// A debug probe seen by probe-rs, for picking one by serial or VID:PID ([ClientConfig::rtt](crate::ClientConfig::rtt)
/// and the filters after it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeInfo {
    pub vid: u16,
    pub pid: u16,
    pub serial: Option<String>,
    /// probe-rs's name of the probe, e.g. `STLink V3`
    pub name: String,
}

/// Debug probes connected to this PC, without opening them.
pub fn list_probes() -> Vec<ProbeInfo> {
    probe_rs::probe::list::Lister::new()
        .list_all()
        .into_iter()
        .map(|p| ProbeInfo {
            vid: p.vendor_id,
            pid: p.product_id,
            serial: p.serial_number,
            name: p.identifier,
        })
        .collect()
}

/// probe-rs chip names whose name or package name starts with `prefix` (case-insensitive, a lowercase `x` in it
/// matches any character, as in probe-rs), sorted, for [ClientConfig::rtt](crate::ClientConfig::rtt)'s `target`.
pub fn chips(prefix: &str) -> Vec<String> {
    let mut chips = probe_rs::config::Registry::from_builtin_families().search_chips(prefix);
    // one per package variant
    chips.sort();
    chips.dedup();
    chips
}
