//! The first bytes of a connection, before any `ww_link` message. Frozen: never changed in place, a new layout gets
//! a new [PREAMBLE_VERSION] and the old one stays readable, so any client and any hub can at least exchange what they
//! are.
//!
//! Client to hub: `| "WWHB" | version: u8 | target: u8 | key_len: u16 LE | key: UTF-8 |`, target 0 is the hub's own
//! API (`key_len` 0), 1 a device by key (`usb:c0de:cafe:<serial>`; empty: the one device a process serves).
//!
//! Hub to client: `| "WWHB" | version: u8 | status: u8 | hub_version_len: u8 | hub_version: UTF-8 |`. On any status
//! but [Status::Ok] the hub closes the connection after it.

use std::fmt;
use std::io;

use tokio::io::{AsyncRead, AsyncReadExt};

pub const MAGIC: [u8; 4] = *b"WWHB";
/// Preamble layout this crate writes.
pub const PREAMBLE_VERSION: u8 = 1;
/// Longest device key accepted, so that a peer can't make the other side allocate much.
pub const MAX_KEY_LEN: usize = 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// The hub's own API
    Hub,
    /// A device by its key, empty for the one device a process serves
    Device(String),
}

/// What a client sends first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preamble {
    pub version: u8,
    pub target: Target,
}

#[derive(Debug)]
pub enum PreambleError {
    Io(io::Error),
    /// Not a WireWeaver client (or not the start of its connection)
    BadMagic,
    /// A later layout this side can't read: answer [Status::PreambleVersion]
    UnsupportedVersion(u8),
    UnknownTarget(u8),
    KeyTooLong(usize),
    KeyNotUtf8,
}

impl Preamble {
    pub fn new(target: Target) -> Self {
        Preamble {
            version: PREAMBLE_VERSION,
            target,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let (kind, key) = match &self.target {
            Target::Hub => (0u8, ""),
            Target::Device(key) => (1u8, key.as_str()),
        };
        let key = &key.as_bytes()[..key.len().min(MAX_KEY_LEN)];
        let mut bytes = Vec::with_capacity(8 + key.len());
        bytes.extend_from_slice(&MAGIC);
        bytes.push(self.version);
        bytes.push(kind);
        bytes.extend_from_slice(&(key.len() as u16).to_le_bytes());
        bytes.extend_from_slice(key);
        bytes
    }

    /// Reads a preamble, the caller bounds the time it may take (a peer that sends nothing).
    pub async fn read<R: AsyncRead + Unpin>(r: &mut R) -> Result<Preamble, PreambleError> {
        let mut head = [0u8; 6];
        r.read_exact(&mut head).await.map_err(PreambleError::Io)?;
        if head[..4] != MAGIC {
            return Err(PreambleError::BadMagic);
        }
        let version = head[4];
        if version != PREAMBLE_VERSION {
            return Err(PreambleError::UnsupportedVersion(version));
        }
        let kind = head[5];
        let mut len = [0u8; 2];
        r.read_exact(&mut len).await.map_err(PreambleError::Io)?;
        let len = u16::from_le_bytes(len) as usize;
        if len > MAX_KEY_LEN {
            return Err(PreambleError::KeyTooLong(len));
        }
        let mut key = vec![0u8; len];
        r.read_exact(&mut key).await.map_err(PreambleError::Io)?;
        let key = String::from_utf8(key).map_err(|_| PreambleError::KeyNotUtf8)?;
        let target = match kind {
            0 => Target::Hub,
            1 => Target::Device(key),
            k => return Err(PreambleError::UnknownTarget(k)),
        };
        Ok(Preamble { version, target })
    }
}

/// The hub's answer to a [Preamble].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Ok,
    /// No device with this key
    UnknownDevice,
    /// Another program holds the device directly
    HeldElsewhere,
    /// The client's preamble version is not served
    PreambleVersion,
    /// The client's `ww_link` / API model versions are not served by this hub's build
    Versions,
    /// A status this side doesn't know (from a later hub)
    Other(u8),
}

impl Status {
    pub fn to_u8(self) -> u8 {
        match self {
            Status::Ok => 0,
            Status::UnknownDevice => 1,
            Status::HeldElsewhere => 2,
            Status::PreambleVersion => 3,
            Status::Versions => 4,
            Status::Other(s) => s,
        }
    }

    pub fn from_u8(s: u8) -> Self {
        match s {
            0 => Status::Ok,
            1 => Status::UnknownDevice,
            2 => Status::HeldElsewhere,
            3 => Status::PreambleVersion,
            4 => Status::Versions,
            s => Status::Other(s),
        }
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Status::Ok => write!(f, "ok"),
            Status::UnknownDevice => write!(f, "no such device"),
            Status::HeldElsewhere => write!(f, "the device is held by another program"),
            Status::PreambleVersion => write!(f, "preamble version not served"),
            Status::Versions => write!(f, "this program's WireWeaver versions are not served"),
            Status::Other(s) => write!(f, "status {s}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answer {
    pub version: u8,
    pub status: Status,
    /// Version of the hub (or whatever serves the socket), for messages and the handover decision
    pub hub_version: String,
}

#[derive(Debug)]
pub enum AnswerError {
    Io(io::Error),
    BadMagic,
    NotUtf8,
}

impl fmt::Display for PreambleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PreambleError::Io(e) => write!(f, "{e}"),
            PreambleError::BadMagic => write!(f, "not a WireWeaver connection"),
            PreambleError::UnsupportedVersion(v) => write!(f, "preamble version {v} not served"),
            PreambleError::UnknownTarget(t) => write!(f, "unknown target {t}"),
            PreambleError::KeyTooLong(l) => write!(f, "device key of {l} bytes"),
            PreambleError::KeyNotUtf8 => write!(f, "device key is not UTF-8"),
        }
    }
}

impl fmt::Display for AnswerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AnswerError::Io(e) => write!(f, "{e}"),
            AnswerError::BadMagic => write!(f, "the other side is not a WireWeaver hub or device"),
            AnswerError::NotUtf8 => write!(f, "hub version is not UTF-8"),
        }
    }
}

impl std::error::Error for PreambleError {}
impl std::error::Error for AnswerError {}

impl Answer {
    pub fn new(status: Status, hub_version: &str) -> Self {
        Answer {
            version: PREAMBLE_VERSION,
            status,
            hub_version: hub_version.to_string(),
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let v = &self.hub_version.as_bytes()[..self.hub_version.len().min(u8::MAX as usize)];
        let mut bytes = Vec::with_capacity(7 + v.len());
        bytes.extend_from_slice(&MAGIC);
        bytes.push(self.version);
        bytes.push(self.status.to_u8());
        bytes.push(v.len() as u8);
        bytes.extend_from_slice(v);
        bytes
    }

    /// Reads an answer of any preamble version: the layout up to the hub version stays the same in every one.
    pub async fn read<R: AsyncRead + Unpin>(r: &mut R) -> Result<Answer, AnswerError> {
        let mut head = [0u8; 7];
        r.read_exact(&mut head).await.map_err(AnswerError::Io)?;
        if head[..4] != MAGIC {
            return Err(AnswerError::BadMagic);
        }
        let mut v = vec![0u8; head[6] as usize];
        r.read_exact(&mut v).await.map_err(AnswerError::Io)?;
        Ok(Answer {
            version: head[4],
            status: Status::from_u8(head[5]),
            hub_version: String::from_utf8(v).map_err(|_| AnswerError::NotUtf8)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn preamble_bytes_and_round_trip() {
        let p = Preamble::new(Target::Device("usb:c0de:cafe:42".into()));
        let bytes = p.encode();
        assert_eq!(&bytes[..8], b"WWHB\x01\x01\x10\x00");
        assert_eq!(Preamble::read(&mut &bytes[..]).await.unwrap(), p);
        let hub = Preamble::new(Target::Hub).encode();
        assert_eq!(hub, b"WWHB\x01\x00\x00\x00");
        assert_eq!(
            Preamble::read(&mut &hub[..]).await.unwrap().target,
            Target::Hub
        );
    }

    #[tokio::test]
    async fn bad_preambles() {
        assert!(matches!(
            Preamble::read(&mut &b"GET / HTTP/1.1"[..]).await,
            Err(PreambleError::BadMagic)
        ));
        assert!(matches!(
            Preamble::read(&mut &b"WWHB\x02\x00\x00\x00"[..]).await,
            Err(PreambleError::UnsupportedVersion(2))
        ));
        assert!(matches!(
            Preamble::read(&mut &b"WWHB\x01\x01\xff\xff"[..]).await,
            Err(PreambleError::KeyTooLong(0xffff))
        ));
        assert!(matches!(
            Preamble::read(&mut &b"WWHB\x01\x07\x00\x00"[..]).await,
            Err(PreambleError::UnknownTarget(7))
        ));
        assert!(matches!(
            Preamble::read(&mut &b"WWHB\x01"[..]).await,
            Err(PreambleError::Io(_))
        ));
    }

    #[tokio::test]
    async fn answer_round_trip_and_later_status() {
        let a = Answer::new(Status::HeldElsewhere, "0.5.0");
        let bytes = a.encode();
        assert_eq!(bytes, b"WWHB\x01\x02\x050.5.0");
        assert_eq!(Answer::read(&mut &bytes[..]).await.unwrap(), a);
        let later = b"WWHB\x02\x09\x010";
        let a = Answer::read(&mut &later[..]).await.unwrap();
        assert_eq!((a.version, a.status), (2, Status::Other(9)));
    }
}
