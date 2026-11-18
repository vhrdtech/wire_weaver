//! What one server announces: [ServiceRecord] with its [Txt] keys, checked against the DNS limits on creation.

use std::fmt;
use std::net::IpAddr;
use std::num::NonZeroU16;

use ww_version::{FullVersion, WireVersion};

/// DNS-SD service type every WireWeaver WebSocket server is announced under.
pub const SERVICE_TYPE: &str = "_wireweaver._tcp";

/// Longest instance name in bytes: one DNS label.
pub const INSTANCE_NAME_LEN_MAX: usize = 63;

/// Longest [InstanceId] in characters, so that `ww-<id>` stays one DNS label.
pub const INSTANCE_ID_LEN_MAX: usize = 32;

/// Longest request path in bytes.
pub const PATH_LEN_MAX: usize = 128;

/// Longest `key=value` string of the TXT record in bytes (RFC 6763 §6.1: one length byte).
pub const TXT_ENTRY_LEN_MAX: usize = 255;

/// Most addresses one record announces.
pub const ADDRESSES_MAX: usize = 16;

/// Instance name shown to people browsing the LAN, like `living-room TV`: one DNS label of UTF-8.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct InstanceName(String);

impl InstanceName {
    /// Checks the name: 1 to [INSTANCE_NAME_LEN_MAX] bytes, no control characters.
    pub fn new(name: impl Into<String>) -> Result<Self, RecordError> {
        let name = name.into();
        if name.is_empty() || name.len() > INSTANCE_NAME_LEN_MAX {
            return Err(RecordError::InstanceNameLength(name.len()));
        }
        if name.chars().any(char::is_control) {
            return Err(RecordError::InstanceNameControl);
        }
        Ok(Self(name))
    }

    /// The name as given.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for InstanceName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Stable id of one server instance, kept by the caller across restarts and address changes, TXT key `id`.
///
/// Announced to the whole LAN in clear: never a secret, a serial number one would rather keep private, or the raw
/// `/etc/machine-id` (hash it with an application key first, as `sd_id128_get_machine_app_specific` does).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct InstanceId(String);

impl InstanceId {
    /// Checks the id: 1 to [INSTANCE_ID_LEN_MAX] characters of `a-z`, `0-9` and `-`.
    pub fn new(id: impl Into<String>) -> Result<Self, RecordError> {
        let id = id.into();
        if id.is_empty() || id.len() > INSTANCE_ID_LEN_MAX {
            return Err(RecordError::InstanceIdLength(id.len()));
        }
        if !id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err(RecordError::InstanceIdCharacters);
        }
        Ok(Self(id))
    }

    /// The id as given.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for InstanceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The TXT keys of a record: `path`, `api`, `wire` and `id`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Txt {
    path: String,
    api: String,
    wire: Option<String>,
    id: InstanceId,
}

impl Txt {
    /// TXT keys of a server at `path` (`/ww`) serving the API `api` (the API crate's `*_FULL_GID`, announced as
    /// `name@major.minor.patch`) on wire line `wire` (`*_WIRE_VERSION`, announced as `w1.0`, left out when `None`).
    pub fn new(
        path: &str,
        api: &FullVersion<'_>,
        wire: Option<WireVersion>,
        id: InstanceId,
    ) -> Result<Self, RecordError> {
        if !path.starts_with('/') || path.len() > PATH_LEN_MAX {
            return Err(RecordError::Path);
        }
        if !path.bytes().all(|b| b.is_ascii_graphic()) {
            return Err(RecordError::Path);
        }
        let v = &api.version;
        let mut api_text = format!("{}@{}.{}.{}", api.crate_id, v.major.0, v.minor.0, v.patch.0);
        if let Some(pre) = v.pre {
            api_text.push('-');
            api_text.push_str(pre);
        }
        let txt = Self {
            path: path.to_owned(),
            api: api_text,
            wire: wire.map(|w| w.to_string()),
            id,
        };
        if let Some((key, _)) = txt
            .entries()
            .find(|(k, v)| k.len() + 1 + v.len() > TXT_ENTRY_LEN_MAX)
        {
            return Err(RecordError::TxtEntryLength(key));
        }
        Ok(txt)
    }

    /// Request path of the WebSocket upgrade, like `/ww`.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The API, `blinky_api@0.1.0`.
    pub fn api(&self) -> &str {
        &self.api
    }

    /// Wire version of the API root, `w1.0`.
    pub fn wire(&self) -> Option<&str> {
        self.wire.as_deref()
    }

    /// The instance id.
    pub fn id(&self) -> &InstanceId {
        &self.id
    }

    /// The `key`, `value` pairs in announcement order.
    pub fn entries(&self) -> impl Iterator<Item = (&'static str, &str)> {
        [
            Some(("path", self.path.as_str())),
            Some(("api", self.api.as_str())),
            self.wire.as_deref().map(|w| ("wire", w)),
            Some(("id", self.id.as_str())),
        ]
        .into_iter()
        .flatten()
    }

    /// The TXT record's strings, `key=value` each.
    pub fn strings(&self) -> Vec<Vec<u8>> {
        self.entries()
            .map(|(k, v)| format!("{k}={v}").into_bytes())
            .collect()
    }
}

/// One server as announced: instance name, port, TXT keys and the addresses clients reach it at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceRecord {
    instance: InstanceName,
    port: NonZeroU16,
    txt: Txt,
    addresses: Vec<IpAddr>,
}

impl ServiceRecord {
    /// A record without addresses: [crate::Advertiser] announces it once addresses are set.
    pub fn new(instance: InstanceName, port: NonZeroU16, txt: Txt) -> Self {
        Self {
            instance,
            port,
            txt,
            addresses: Vec::new(),
        }
    }

    /// The same record with `addresses`, see [Self::set_addresses].
    pub fn with_addresses(
        mut self,
        addresses: impl IntoIterator<Item = IpAddr>,
    ) -> Result<Self, RecordError> {
        self.set_addresses(addresses)?;
        Ok(self)
    }

    /// Replaces the addresses: at most [ADDRESSES_MAX], none unspecified or multicast; duplicates are dropped and
    /// the order is kept. Each address is announced only on the interface that has it.
    pub fn set_addresses(
        &mut self,
        addresses: impl IntoIterator<Item = IpAddr>,
    ) -> Result<(), RecordError> {
        let mut list: Vec<IpAddr> = Vec::new();
        for a in addresses {
            if a.is_unspecified() || a.is_multicast() {
                return Err(RecordError::Address(a));
            }
            if list.contains(&a) {
                continue;
            }
            if list.len() == ADDRESSES_MAX {
                return Err(RecordError::TooManyAddresses);
            }
            list.push(a);
        }
        self.addresses = list;
        Ok(())
    }

    /// Instance name.
    pub fn instance(&self) -> &InstanceName {
        &self.instance
    }

    /// TCP port of the WebSocket server.
    pub fn port(&self) -> NonZeroU16 {
        self.port
    }

    /// TXT keys.
    pub fn txt(&self) -> &Txt {
        &self.txt
    }

    /// Addresses announced, in the caller's order.
    pub fn addresses(&self) -> &[IpAddr] {
        &self.addresses
    }

    /// Host name the addresses are announced under, `ww-<id>.local`: one of its own, so it never collides with the
    /// machine's host name that another responder on the same box (Avahi) owns.
    pub fn host_name(&self) -> String {
        format!("ww-{}.local", self.txt.id)
    }
}

/// A record value outside the DNS limits or the shape the TXT keys need.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RecordError {
    /// Instance name empty or longer than [INSTANCE_NAME_LEN_MAX] bytes.
    #[error("instance name is {0} bytes, 1 to {INSTANCE_NAME_LEN_MAX} fit")]
    InstanceNameLength(usize),
    /// Instance name with a control character.
    #[error("instance name has a control character")]
    InstanceNameControl,
    /// Instance id empty or longer than [INSTANCE_ID_LEN_MAX].
    #[error("instance id is {0} characters, 1 to {INSTANCE_ID_LEN_MAX} fit")]
    InstanceIdLength(usize),
    /// Instance id with a character other than `a-z`, `0-9` or `-`.
    #[error("instance id takes only a-z, 0-9 and -")]
    InstanceIdCharacters,
    /// Path not starting with `/`, longer than [PATH_LEN_MAX] or with a space or non-ASCII byte.
    #[error("path must start with / and be at most {PATH_LEN_MAX} visible ASCII characters")]
    Path,
    /// A `key=value` string longer than [TXT_ENTRY_LEN_MAX].
    #[error("TXT key {0} is longer than {TXT_ENTRY_LEN_MAX} bytes with its value")]
    TxtEntryLength(&'static str),
    /// Unspecified or multicast address.
    #[error("{0} can't be announced")]
    Address(IpAddr),
    /// More than [ADDRESSES_MAX] addresses.
    #[error("more than {ADDRESSES_MAX} addresses")]
    TooManyAddresses,
}

#[cfg(test)]
mod tests {
    use super::*;
    use ww_version::Version;

    const API: FullVersion<'static> = FullVersion::new("blinky_api", Version::new(0, 1, 2));

    fn txt() -> Txt {
        Txt::new(
            "/ww",
            &API,
            Some(WireVersion::new(1, 0)),
            InstanceId::new("tv-1").unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn txt_strings_are_key_value_in_order() {
        let strings: Vec<String> = txt()
            .strings()
            .into_iter()
            .map(|s| String::from_utf8(s).unwrap())
            .collect();
        assert_eq!(
            strings,
            ["path=/ww", "api=blinky_api@0.1.2", "wire=w1.0", "id=tv-1"]
        );
    }

    #[test]
    fn wire_is_left_out_when_unknown() {
        let txt = Txt::new("/", &API, None, InstanceId::new("a").unwrap()).unwrap();
        assert_eq!(txt.wire(), None);
        assert!(txt.entries().all(|(k, _)| k != "wire"));
    }

    #[test]
    fn limits_are_checked() {
        assert_eq!(
            InstanceName::new(""),
            Err(RecordError::InstanceNameLength(0))
        );
        assert!(InstanceName::new("x".repeat(INSTANCE_NAME_LEN_MAX)).is_ok());
        assert_eq!(
            InstanceName::new("x".repeat(INSTANCE_NAME_LEN_MAX + 1)),
            Err(RecordError::InstanceNameLength(INSTANCE_NAME_LEN_MAX + 1))
        );
        assert_eq!(
            InstanceName::new("a\nb"),
            Err(RecordError::InstanceNameControl)
        );
        assert_eq!(
            InstanceId::new("TV"),
            Err(RecordError::InstanceIdCharacters)
        );
        assert_eq!(
            InstanceId::new("a".repeat(INSTANCE_ID_LEN_MAX + 1)),
            Err(RecordError::InstanceIdLength(INSTANCE_ID_LEN_MAX + 1))
        );
        let id = || InstanceId::new("a").unwrap();
        assert_eq!(Txt::new("ww", &API, None, id()), Err(RecordError::Path));
        assert_eq!(Txt::new("/w w", &API, None, id()), Err(RecordError::Path));
        let long_crate = "c".repeat(TXT_ENTRY_LEN_MAX);
        let api = FullVersion::new(&long_crate, Version::new(1, 0, 0));
        assert_eq!(
            Txt::new("/", &api, None, id()),
            Err(RecordError::TxtEntryLength("api"))
        );
    }

    #[test]
    fn addresses_are_checked_and_deduplicated() {
        let record = ServiceRecord::new(
            InstanceName::new("tv").unwrap(),
            NonZeroU16::new(7590).unwrap(),
            txt(),
        );
        let a: IpAddr = "192.168.1.5".parse().unwrap();
        let b: IpAddr = "fd00::5".parse().unwrap();
        let r = record.clone().with_addresses([a, b, a]).unwrap();
        assert_eq!(r.addresses(), [a, b]);
        assert_eq!(r.host_name(), "ww-tv-1.local");
        let any: IpAddr = "0.0.0.0".parse().unwrap();
        assert_eq!(
            record.clone().with_addresses([any]),
            Err(RecordError::Address(any))
        );
        let many = (1..=ADDRESSES_MAX as u8 + 1).map(|i| IpAddr::from([10, 0, 0, i]));
        assert_eq!(
            record.with_addresses(many),
            Err(RecordError::TooManyAddresses)
        );
    }
}
