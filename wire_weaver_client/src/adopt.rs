//! Live link handover (CLT-44): what a direct holder's event loop hands to the ww hub so the hub goes on with the
//! same link, the device never seeing a reconnect. A [HandoverOwned] is the link's residual in two parts:
//!
//! - [LinkResidualOwned]: everything about the link itself (token, `DeviceInfo`, seqs, requests in flight, streams,
//!   observed levels, ping timers), the same on every medium: any event loop goes on from it, whichever medium it
//!   runs on;
//! - [MediumResidualOwned]: a kind tag and bytes only the medium of that kind reads (for USB its endpoints and what
//!   its framers hold between frames): the medium that takes the link over decodes them, nothing else looks inside.
//!
//! Both are internal state between two builds of the same `wire_weaver_client` minor ([same_minor]), not a stable
//! wire contract: they may change in any release.

use wire_weaver::prelude::*;
use wire_weaver::shrink_wrap::{DeserializeShrinkWrapOwned, SerializeShrinkWrapOwned};

/// This crate's version: a handover is adopted by the same major.minor ([same_minor]).
pub const CLIENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Longest handover accepted: far above a real one (a few kB with every seq in flight).
pub const MAX_SNAPSHOT_LEN: usize = 256 * 1024;
const MAX_PATH_LEN: usize = 64;
const MAX_STREAMS: usize = 4096;
const MAX_LEVELS: usize = 4096;
const MAX_FULL_ONLY: usize = 1024;
/// Largest seq number a residual may carry, as [ClientConfig::max_seq](crate::ClientConfig::max_seq) allows (a seq
/// takes up to 5 bytes on the wire).
const MAX_SEQ: u32 = u32::MAX >> 4;
/// Most requests in flight a residual may carry.
const MAX_IN_FLIGHT: usize = 1 << 16;

/// The holder's link to its device, taken while the link is paused (nothing goes out, nothing is being received):
/// what `Adopt::prepare` takes.
#[derive_shrink_wrap(owned, derive(Clone, Debug, PartialEq))]
pub struct HandoverOwned {
    /// `wire_weaver_client` version of the holder: adopted only by a hub of the same major.minor
    pub client_version: String,
    /// The device's key in the hub (`usb:<vid>:<pid>:<serial>`, `usbsim:<path>`)
    pub key: String,
    pub link: LinkResidualOwned,
    pub medium: MediumResidualOwned,
}

/// The link, without anything of the medium it runs on: any event loop goes on from it.
#[derive_shrink_wrap(owned, derive(Clone, Debug, PartialEq))]
pub struct LinkResidualOwned {
    /// Link setup token of the link
    pub token: u64,
    /// The device's `DeviceInfo` message, as received at link setup
    pub device_info: Vec<u8>,
    /// Milliseconds until the next ping is due
    pub ping_in_ms: u32,
    /// Milliseconds since the device sent anything
    pub last_rx_ms_ago: u32,
    pub next_seq: u32,
    pub max_seq: u32,
    /// Seq numbers in use: requests waiting for their answer, and timed out ones within the late reply window
    pub in_flight: Vec<u32>,
    /// Traits the device only takes with a `GlobalFull` path (API-20): `ww_global` id, major, minor
    pub full_only: Vec<TraitIdOwned>,
    /// Streams the holder has open on the device, by path
    pub streams: Vec<Vec<u32>>,
    /// Observed trait levels: the `Observe` request (seq 0) as last sent for each
    pub observed: Vec<Vec<u8>>,
}

/// What the medium the link ran on leaves for the medium that takes it over: only a medium of `kind` reads `bytes`.
#[derive_shrink_wrap(owned, derive(Clone, Debug, PartialEq, Eq))]
pub struct MediumResidualOwned {
    pub kind: MediumKindOwned,
    pub bytes: Vec<u8>,
}

/// The medium a [MediumResidualOwned] is of.
#[derive_shrink_wrap(owned, derive(Clone, Debug, PartialEq, Eq), ww_repr = unib32)]
pub enum MediumKindOwned {
    /// A real USB device (Linux): its interface and endpoints, and what the framers hold between frames
    Usb,
    /// A simulated one on a local socket ([crate::usb_sim]): the same, without endpoints
    UsbSim,
}

impl MediumKindOwned {
    /// The scheme of the device keys of this medium in the hub.
    pub fn scheme(&self) -> Option<&'static str> {
        match self {
            MediumKindOwned::Usb => Some("usb"),
            MediumKindOwned::UsbSim => Some("usbsim"),
            // a medium of a later build
            MediumKindOwned::Unknown(_) => None,
        }
    }
}

#[derive_shrink_wrap(owned, derive(Copy, Clone, Debug, PartialEq, Eq))]
pub struct TraitIdOwned {
    pub id: u32,
    pub major: u32,
    pub minor: u32,
}

/// A random non-zero-most-of-the-time id (an adoption's), from the same sources as a link setup token.
pub fn random_id() -> u64 {
    crate::event_loop::core::new_setup_token()
}

/// Whether a hub built with `wire_weaver_client` `hub_version` may adopt a link of `holder_version`: the same
/// major.minor (the residual is internal state, see the module docs).
pub fn same_minor(holder_version: &str, hub_version: &str) -> bool {
    match (
        semver::Version::parse(holder_version),
        semver::Version::parse(hub_version),
    ) {
        (Ok(a), Ok(b)) => a.major == b.major && a.minor == b.minor,
        _ => false,
    }
}

impl HandoverOwned {
    pub fn encode(&self) -> Result<Vec<u8>, crate::Error> {
        Ok(self.to_ww_bytes_owned()?)
    }

    /// Decodes a handover another process sent and checks every size and count in it ([Self::check]).
    pub fn decode(bytes: &[u8]) -> Result<HandoverOwned, String> {
        if bytes.len() > MAX_SNAPSHOT_LEN {
            return Err(format!("handover of {} bytes", bytes.len()));
        }
        let h = HandoverOwned::from_ww_bytes_owned(bytes).map_err(|e| format!("{e:?}"))?;
        h.check()?;
        Ok(h)
    }

    /// Bounds both residuals, see [LinkResidualOwned::check] and [MediumResidualOwned::check].
    pub fn check(&self) -> Result<(), String> {
        self.link.check()?;
        self.medium.check()
    }
}

impl LinkResidualOwned {
    /// The device's `DeviceInfo`.
    pub fn device_info(&self) -> Result<ww_link::DeviceInfoOwned, String> {
        ww_link::DeviceInfoOwned::from_ww_bytes_owned(&self.device_info)
            .map_err(|e| format!("device info: {e:?}"))
    }

    /// Bounds every size and count against the device's `DeviceInfo`, so that a residual from another process can't
    /// make the hub allocate much or hold state the link can't have.
    pub fn check(&self) -> Result<(), String> {
        let info = self.device_info()?;
        if info.token != self.token || self.token == 0 {
            return Err("token doesn't match the device info".into());
        }
        if self.max_seq == 0 || self.max_seq > MAX_SEQ {
            return Err(format!("max seq {}", self.max_seq));
        }
        if self.next_seq == 0 || self.next_seq > self.max_seq {
            return Err(format!("next seq {}", self.next_seq));
        }
        if self.in_flight.len() > (self.max_seq as usize).min(MAX_IN_FLIGHT) {
            return Err(format!("{} seqs in flight", self.in_flight.len()));
        }
        let mut seen = std::collections::HashSet::new();
        for seq in &self.in_flight {
            if *seq == 0 || *seq > self.max_seq || !seen.insert(*seq) {
                return Err(format!("seq {seq} in flight"));
            }
        }
        if self.full_only.len() > MAX_FULL_ONLY {
            return Err(format!("{} full-path traits", self.full_only.len()));
        }
        if self.streams.len() > MAX_STREAMS || self.streams.iter().any(|p| p.len() > MAX_PATH_LEN) {
            return Err("too many streams, or a path too long".into());
        }
        if self.observed.len() > MAX_LEVELS
            || self
                .observed
                .iter()
                .any(|r| r.len() > info.dev_max_message_len as usize + 5)
        {
            return Err(
                "too many observed levels, or a request longer than the device takes".into(),
            );
        }
        Ok(())
    }
}

impl MediumResidualOwned {
    /// Has the medium of [Self::kind] decode and bound its bytes: an error for a medium this build doesn't have.
    pub fn check(&self) -> Result<(), String> {
        match self.kind {
            #[cfg(feature = "usb")]
            MediumKindOwned::Usb => crate::usb::UsbResidualOwned::decode(self).map(drop),
            #[cfg(feature = "local_socket")]
            MediumKindOwned::UsbSim => crate::usb_sim::UsbSimResidualOwned::decode(self).map(drop),
            ref kind => Err(format!("a medium this build doesn't have: {kind:?}")),
        }
    }

    /// The bytes, if they are of `kind`.
    #[cfg_attr(not(any(feature = "usb", feature = "local_socket")), allow(dead_code))]
    pub(crate) fn of(&self, kind: MediumKindOwned) -> Result<&[u8], String> {
        if self.kind == kind {
            Ok(&self.bytes)
        } else {
            Err(format!(
                "a {:?} medium can't take the link of a {:?} one",
                kind, self.kind
            ))
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use ww_version::{ApiHashPair, CompactVersion, FullVersion, GlobalTypeId, Version};

    pub(crate) fn link() -> LinkResidualOwned {
        let info = ww_link::DeviceInfo {
            dev_link_version: ww_link::LINK_VERSION,
            api_model_version: CompactVersion::new(GlobalTypeId::new(513), 0, 2, 0),
            user_api_version: FullVersion::new("test", Version::new(0, 1, 3)),
            hash: ApiHashPair::empty(),
            dev_max_message_len: 512,
            packet_accumulation_time_us: 500,
            token: 7,
        };
        LinkResidualOwned {
            token: 7,
            device_info: info.make_owned().to_ww_bytes_owned().unwrap(),
            ping_in_ms: 1000,
            last_rx_ms_ago: 3,
            next_seq: 5,
            max_seq: 1024,
            in_flight: vec![3, 4],
            full_only: vec![],
            streams: vec![vec![1, 2]],
            observed: vec![],
        }
    }

    #[test]
    fn link_round_trip_and_bounds() {
        let l = link();
        assert_eq!(
            LinkResidualOwned::from_ww_bytes_owned(&l.to_ww_bytes_owned().unwrap()).unwrap(),
            l
        );
        l.check().unwrap();
        let bad = |f: &dyn Fn(&mut LinkResidualOwned)| {
            let mut l = link();
            f(&mut l);
            l.check().unwrap_err()
        };
        bad(&|l| l.in_flight.push(3));
        bad(&|l| l.in_flight.push(2000));
        bad(&|l| l.next_seq = 0);
        bad(&|l| l.token = 8);
        bad(&|l| l.streams.push(vec![0; 65]));
        bad(&|l| l.device_info.truncate(3));
        bad(&|l| l.max_seq = u32::MAX);
        assert!(same_minor("0.5.1", "0.5.7"));
        assert!(!same_minor("0.5.1", "0.6.0"));
        assert!(!same_minor("x", "0.5.0"));
    }

    #[test]
    fn a_medium_reads_only_its_own_bytes() {
        let m = MediumResidualOwned {
            kind: MediumKindOwned::Usb,
            bytes: vec![1, 2, 3],
        };
        assert_eq!(m.of(MediumKindOwned::Usb).unwrap(), &[1, 2, 3]);
        assert!(m.of(MediumKindOwned::UsbSim).is_err());
        let later = MediumResidualOwned {
            kind: MediumKindOwned::Unknown(9),
            bytes: vec![],
        };
        assert!(later.check().is_err());
        assert!(HandoverOwned::decode(&[0; 3]).is_err());
    }
}
