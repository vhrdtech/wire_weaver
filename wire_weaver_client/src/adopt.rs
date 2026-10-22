//! Live link handover (CLT-44): what a direct holder's event loop hands to the ww hub so the hub goes on with the
//! same link, the device never seeing a reconnect. [LinkSnapshotOwned] is internal state between two builds of the
//! same `wire_weaver_client` minor ([same_minor]), not a stable wire contract: it may change in any release.

use wire_weaver::prelude::*;
use wire_weaver::shrink_wrap::{DeserializeShrinkWrapOwned, SerializeShrinkWrapOwned};

/// Longest snapshot accepted: far above a real one (a few kB with every seq in flight).
pub const MAX_SNAPSHOT_LEN: usize = 256 * 1024;
const MAX_PATH_LEN: usize = 64;
const MAX_STREAMS: usize = 4096;
const MAX_LEVELS: usize = 4096;
const MAX_FULL_ONLY: usize = 1024;
/// Largest seq number a snapshot may carry, as [ClientConfig::max_seq](crate::ClientConfig::max_seq) allows.
const MAX_SEQ: u32 = 1 << 16;
/// Largest USB packet.
const MAX_PACKET: u16 = 1024;

/// The holder's link to its device, taken while the link is paused (nothing goes out, nothing is being received).
#[derive_shrink_wrap(owned, derive(Clone, Debug, PartialEq))]
pub struct LinkSnapshotOwned {
    /// `wire_weaver_client` version of the holder: adopted only by a hub of the same major.minor
    pub client_version: String,
    /// The device's holders registry key
    pub key: String,
    pub medium: MediumOwned,
    /// The holder's next OUT frame: the hub's tx framer goes on from it
    pub tx_frame: Option<FrameAtOwned>,
    /// What the holder's rx framer has between frames
    pub rx_partial: Option<PartialOwned>,
    pub rx_staged: Vec<u8>,
    pub rx_next_frame: Option<FrameAtOwned>,
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

/// Which medium the link is on, and what of it the hub needs to open it the same way.
#[derive_shrink_wrap(owned, derive(Clone, Debug, PartialEq, Eq), ww_repr = unib32)]
pub enum MediumOwned {
    /// A real USB device (Linux)
    Usb {
        interface: u8,
        ep_in: u8,
        ep_out: u8,
        max_packet: u16,
        bulk: bool,
        seq_prefix: bool,
    },
    /// A simulated one on a local socket ([crate::usb_sim])
    UsbSim { max_packet: u16, seq_prefix: bool },
}

impl MediumOwned {
    pub fn max_packet(&self) -> u16 {
        match self {
            MediumOwned::Usb { max_packet, .. } | MediumOwned::UsbSim { max_packet, .. } => {
                *max_packet
            }
            // a medium of a later build: check() refuses it
            MediumOwned::Unknown(_) => 0,
        }
    }

    pub fn seq_prefix(&self) -> bool {
        match self {
            MediumOwned::Usb { seq_prefix, .. } | MediumOwned::UsbSim { seq_prefix, .. } => {
                *seq_prefix
            }
            MediumOwned::Unknown(_) => false,
        }
    }
}

#[derive_shrink_wrap(owned, derive(Copy, Clone, Debug, PartialEq, Eq))]
pub struct FrameAtOwned {
    pub seq: u32,
    pub epoch: u8,
}

/// A split message the holder's rx framer had half assembled.
#[derive_shrink_wrap(owned, derive(Clone, Debug, PartialEq, Eq))]
pub struct PartialOwned {
    pub user_kind: u8,
    pub total: u32,
    pub bytes: Vec<u8>,
}

#[derive_shrink_wrap(owned, derive(Copy, Clone, Debug, PartialEq, Eq))]
pub struct TraitIdOwned {
    pub id: u32,
    pub major: u32,
    pub minor: u32,
}

/// Whether a hub built with `wire_weaver_client` `hub_version` may adopt a link of `holder_version`: the same
/// major.minor (the snapshot is internal state, see the module docs).
pub fn same_minor(holder_version: &str, hub_version: &str) -> bool {
    match (
        semver::Version::parse(holder_version),
        semver::Version::parse(hub_version),
    ) {
        (Ok(a), Ok(b)) => a.major == b.major && a.minor == b.minor,
        _ => false,
    }
}

impl LinkSnapshotOwned {
    pub fn encode(&self) -> Result<Vec<u8>, crate::Error> {
        Ok(self.to_ww_bytes_owned()?)
    }

    /// Decodes a snapshot another process sent and checks every size and count in it ([Self::check]).
    pub fn decode(bytes: &[u8]) -> Result<LinkSnapshotOwned, String> {
        if bytes.len() > MAX_SNAPSHOT_LEN {
            return Err(format!("snapshot of {} bytes", bytes.len()));
        }
        let s = LinkSnapshotOwned::from_ww_bytes_owned(bytes).map_err(|e| format!("{e:?}"))?;
        s.check()?;
        Ok(s)
    }

    /// The device's `DeviceInfo`.
    pub fn device_info(&self) -> Result<ww_link::DeviceInfoOwned, String> {
        ww_link::DeviceInfoOwned::from_ww_bytes_owned(&self.device_info)
            .map_err(|e| format!("device info: {e:?}"))
    }

    /// Bounds every size and count against the medium and the device's `DeviceInfo`, so that a snapshot from another
    /// process can't make the hub allocate much or hold state the link can't have.
    pub fn check(&self) -> Result<(), String> {
        let max_packet = self.medium.max_packet();
        if !(8..=MAX_PACKET).contains(&max_packet) {
            return Err(format!("max packet {max_packet}"));
        }
        let info = self.device_info()?;
        if info.token != self.token || self.token == 0 {
            return Err("token doesn't match the device info".into());
        }
        let host_max = crate::DEFAULT_MAX_MESSAGE_SIZE;
        if let Some(p) = &self.rx_partial {
            let total = p.total as usize;
            if total > host_max || p.bytes.len() >= total {
                return Err(format!(
                    "partial message of {} / {} bytes",
                    p.bytes.len(),
                    p.total
                ));
            }
        }
        if self.rx_staged.len() > max_packet as usize {
            return Err(format!("{} staged bytes", self.rx_staged.len()));
        }
        if self.max_seq == 0 || self.max_seq > MAX_SEQ {
            return Err(format!("max seq {}", self.max_seq));
        }
        if self.next_seq == 0 || self.next_seq > self.max_seq {
            return Err(format!("next seq {}", self.next_seq));
        }
        if self.in_flight.len() > self.max_seq as usize {
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

#[cfg(test)]
mod tests {
    use super::*;
    use ww_version::{ApiHashPair, CompactVersion, FullVersion, GlobalTypeId, Version};

    pub(crate) fn snapshot() -> LinkSnapshotOwned {
        let info = ww_link::DeviceInfo {
            dev_link_version: ww_link::LINK_VERSION,
            api_model_version: CompactVersion::new(GlobalTypeId::new(513), 0, 2, 0),
            user_api_version: FullVersion::new("test", Version::new(0, 1, 3)),
            hash: ApiHashPair::empty(),
            dev_max_message_len: 512,
            packet_accumulation_time_us: 500,
            token: 7,
        };
        LinkSnapshotOwned {
            client_version: env!("CARGO_PKG_VERSION").into(),
            key: "usbsim:/tmp/x".into(),
            medium: MediumOwned::UsbSim {
                max_packet: 64,
                seq_prefix: true,
            },
            tx_frame: Some(FrameAtOwned { seq: 9, epoch: 1 }),
            rx_partial: Some(PartialOwned {
                user_kind: 0,
                total: 100,
                bytes: vec![1; 60],
            }),
            rx_staged: vec![],
            rx_next_frame: Some(FrameAtOwned { seq: 3, epoch: 0 }),
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
    fn round_trip_and_bounds() {
        let s = snapshot();
        let bytes = s.encode().unwrap();
        assert_eq!(LinkSnapshotOwned::decode(&bytes).unwrap(), s);
        let bad = |f: &dyn Fn(&mut LinkSnapshotOwned)| {
            let mut s = snapshot();
            f(&mut s);
            LinkSnapshotOwned::decode(&s.encode().unwrap()).unwrap_err()
        };
        bad(&|s| s.in_flight.push(3));
        bad(&|s| s.in_flight.push(2000));
        bad(&|s| s.next_seq = 0);
        bad(&|s| s.token = 8);
        bad(&|s| s.rx_partial.as_mut().unwrap().total = 1 << 20);
        bad(&|s| s.rx_partial.as_mut().unwrap().bytes = vec![0; 100]);
        bad(&|s| s.rx_staged = vec![0; 65]);
        bad(&|s| s.streams.push(vec![0; 65]));
        bad(&|s| s.device_info.truncate(3));
        bad(&|s| s.max_seq = 1 << 20);
        assert!(LinkSnapshotOwned::decode(&[0; 3]).is_err());
        assert!(same_minor("0.5.1", "0.5.7"));
        assert!(!same_minor("0.5.1", "0.6.0"));
        assert!(!same_minor("x", "0.5.0"));
    }
}
