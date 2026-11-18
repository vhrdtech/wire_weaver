//! DNS-SD announcements end to end (TR-32): `ww_dns_sd`'s responders driven through an `Advertiser`, seen from the
//! other side. The in-process responder is browsed by a second `mdns-sd` daemon on loopback, on a port of its own,
//! so the LAN's mDNS sees nothing; the Avahi responder talks to a fake Avahi over a peer-to-peer D-Bus connection.

#[cfg(test)]
mod avahi;
#[cfg(test)]
mod mdns;

#[cfg(test)]
mod common {
    use std::num::NonZeroU16;

    use ww_dns_sd::{InstanceId, InstanceName, ServiceRecord, Txt};
    use ww_version::{FullVersion, Version, WireVersion};

    pub(crate) const PORT: u16 = 7590;

    pub(crate) fn record(id: &str) -> ServiceRecord {
        let txt = Txt::new(
            "/ww",
            &FullVersion::new("tv_api", Version::new(0, 3, 1)),
            Some(WireVersion::new(1, 2)),
            InstanceId::new(id).unwrap(),
        )
        .unwrap();
        ServiceRecord::new(
            InstanceName::new("Living room TV").unwrap(),
            NonZeroU16::new(PORT).unwrap(),
            txt,
        )
    }
}
