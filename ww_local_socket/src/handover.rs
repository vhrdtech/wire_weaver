//! What a program asks a direct holder of a device on the holder's socket (CLT-43): hand the device to the hub.
//! Frozen like the [Preamble](crate::Preamble): never changed in place, a new layout gets a new
//! [HANDOVER_VERSION], so holders and askers of any two versions can at least say no to each other.
//!
//! A holder that offers it listens on `<runtime>/ww/holders/<key>.sock` (its lock file names the socket in a
//! `handover = <path>` line) for as long as it holds the device directly.
//!
//! Asker to holder, version 1: `| "WWHO" | version: u8 | op: u8 | trailer_len: u16 LE | entries |`, op 1 is
//! [Op::HandToHub]. Holder to asker: the same with the status instead of the op. The trailer is the preamble's: each
//! entry `| tag: u8 | len: u8 | value |`, a reader takes the entries it knows and skips the others, so later fields
//! come as new entries without a new version. Entries: [TAG_META], the sender's client metadata (`ww_link::ClientMeta`,
//! shrink_wrap bytes), split over consecutive entries of that tag when longer than 255 bytes (concatenated in order).
//!
//! [HandoverStatus::Released] comes once the holder has let go of the device (the hub may open it now),
//! [HandoverStatus::Refused] at once from a holder that doesn't hand over (it never releases). The holder closes the
//! connection after its answer.

use std::fmt;
use std::io;

use tokio::io::{AsyncRead, AsyncReadExt};

pub const HANDOVER_MAGIC: [u8; 4] = *b"WWHO";
/// Layout this crate writes.
pub const HANDOVER_VERSION: u8 = 1;
/// Trailer entry: the sender's client metadata, possibly split over several entries.
pub const TAG_META: u8 = 1;
/// Longest trailer accepted, so that a peer can't make the other side allocate much.
pub const MAX_HANDOVER_TRAILER_LEN: usize = 2048;
/// Longest client metadata sent: what fits the trailer in entries of at most 255 bytes.
pub const MAX_META_LEN: usize = 1536;

/// What the asker wants.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Op {
    /// Let go of the device so the ww hub can open it, then go on through the hub.
    HandToHub,
    /// An op this side doesn't know (from a later asker)
    Other(u8),
}

impl Op {
    pub fn to_u8(self) -> u8 {
        match self {
            Op::HandToHub => 1,
            Op::Other(op) => op,
        }
    }

    pub fn from_u8(op: u8) -> Self {
        match op {
            1 => Op::HandToHub,
            op => Op::Other(op),
        }
    }
}

/// The holder's answer.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum HandoverStatus {
    /// The device is released: the hub can open it now, the holder goes on through the hub.
    Released,
    /// This holder doesn't hand its device over (a GUI or service by default, or switched off).
    Refused,
    /// The holder agreed but couldn't let go of the device in time, or didn't understand the request.
    Failed,
    /// A status this side doesn't know (from a later holder)
    Other(u8),
}

impl HandoverStatus {
    pub fn to_u8(self) -> u8 {
        match self {
            HandoverStatus::Released => 0,
            HandoverStatus::Refused => 1,
            HandoverStatus::Failed => 2,
            HandoverStatus::Other(s) => s,
        }
    }

    pub fn from_u8(s: u8) -> Self {
        match s {
            0 => HandoverStatus::Released,
            1 => HandoverStatus::Refused,
            2 => HandoverStatus::Failed,
            s => HandoverStatus::Other(s),
        }
    }
}

/// A request or an answer: the same layout, `code` is the op or the status.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Frame {
    version: u8,
    code: u8,
    meta: Vec<u8>,
}

#[derive(Debug)]
pub enum HandoverError {
    Io(io::Error),
    BadMagic,
    UnsupportedVersion(u8),
    TrailerTooLong(usize),
    BadTrailer,
}

impl fmt::Display for HandoverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HandoverError::Io(e) => write!(f, "{e}"),
            HandoverError::BadMagic => write!(f, "not a WireWeaver holder socket"),
            HandoverError::UnsupportedVersion(v) => write!(f, "handover version {v} not served"),
            HandoverError::TrailerTooLong(l) => write!(f, "handover trailer of {l} bytes"),
            HandoverError::BadTrailer => write!(f, "handover trailer entry cut short"),
        }
    }
}

impl std::error::Error for HandoverError {}

impl Frame {
    fn encode(&self) -> Vec<u8> {
        let meta = &self.meta[..self.meta.len().min(MAX_META_LEN)];
        let mut trailer = Vec::with_capacity(meta.len() + meta.len() / 255 * 2 + 2);
        for chunk in meta.chunks(u8::MAX as usize) {
            trailer.extend_from_slice(&[TAG_META, chunk.len() as u8]);
            trailer.extend_from_slice(chunk);
        }
        let mut bytes = Vec::with_capacity(8 + trailer.len());
        bytes.extend_from_slice(&HANDOVER_MAGIC);
        bytes.push(self.version);
        bytes.push(self.code);
        bytes.extend_from_slice(&(trailer.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&trailer);
        bytes
    }

    async fn read<R: AsyncRead + Unpin>(r: &mut R) -> Result<Frame, HandoverError> {
        let mut head = [0u8; 8];
        r.read_exact(&mut head).await.map_err(HandoverError::Io)?;
        if head[..4] != HANDOVER_MAGIC {
            return Err(HandoverError::BadMagic);
        }
        // every later version keeps this head: magic, version, code, trailer
        if head[4] == 0 {
            return Err(HandoverError::UnsupportedVersion(0));
        }
        let len = u16::from_le_bytes([head[6], head[7]]) as usize;
        if len > MAX_HANDOVER_TRAILER_LEN {
            return Err(HandoverError::TrailerTooLong(len));
        }
        let mut trailer = vec![0u8; len];
        r.read_exact(&mut trailer)
            .await
            .map_err(HandoverError::Io)?;
        let mut meta = vec![];
        let mut rest = &trailer[..];
        while let [tag, len, tail @ ..] = rest {
            let len = *len as usize;
            if tail.len() < len {
                return Err(HandoverError::BadTrailer);
            }
            if *tag == TAG_META {
                meta.extend_from_slice(&tail[..len]);
            }
            rest = &tail[len..];
        }
        if !rest.is_empty() {
            return Err(HandoverError::BadTrailer);
        }
        Ok(Frame {
            version: head[4],
            code: head[5],
            meta,
        })
    }
}

/// Asker to holder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandoverRequest {
    pub version: u8,
    pub op: Op,
    /// The asker's client metadata, shrink_wrap bytes; empty if it sent none
    pub meta: Vec<u8>,
}

impl HandoverRequest {
    pub fn new(op: Op, meta: Vec<u8>) -> Self {
        HandoverRequest {
            version: HANDOVER_VERSION,
            op,
            meta,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        Frame {
            version: self.version,
            code: self.op.to_u8(),
            meta: self.meta.clone(),
        }
        .encode()
    }

    pub async fn read<R: AsyncRead + Unpin>(r: &mut R) -> Result<Self, HandoverError> {
        let f = Frame::read(r).await?;
        Ok(HandoverRequest {
            version: f.version,
            op: Op::from_u8(f.code),
            meta: f.meta,
        })
    }
}

/// Holder to asker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandoverAnswer {
    pub version: u8,
    pub status: HandoverStatus,
    /// The holder's client metadata, shrink_wrap bytes; empty if it has none
    pub meta: Vec<u8>,
}

impl HandoverAnswer {
    pub fn new(status: HandoverStatus, meta: Vec<u8>) -> Self {
        HandoverAnswer {
            version: HANDOVER_VERSION,
            status,
            meta,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        Frame {
            version: self.version,
            code: self.status.to_u8(),
            meta: self.meta.clone(),
        }
        .encode()
    }

    pub async fn read<R: AsyncRead + Unpin>(r: &mut R) -> Result<Self, HandoverError> {
        let f = Frame::read(r).await?;
        Ok(HandoverAnswer {
            version: f.version,
            status: HandoverStatus::from_u8(f.code),
            meta: f.meta,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The frozen bytes of version 1.
    #[tokio::test]
    async fn bytes_and_round_trip() {
        let request = HandoverRequest::new(Op::HandToHub, vec![0xaa, 0xbb]);
        let bytes = request.encode();
        assert_eq!(bytes, b"WWHO\x01\x01\x04\x00\x01\x02\xaa\xbb");
        assert_eq!(
            HandoverRequest::read(&mut &bytes[..]).await.unwrap(),
            request
        );

        let answer = HandoverAnswer::new(HandoverStatus::Refused, vec![]);
        let bytes = answer.encode();
        assert_eq!(bytes, b"WWHO\x01\x01\x00\x00");
        assert_eq!(HandoverAnswer::read(&mut &bytes[..]).await.unwrap(), answer);
        assert_eq!(
            HandoverAnswer::read(&mut &b"WWHO\x01\x00\x00\x00"[..])
                .await
                .unwrap()
                .status,
            HandoverStatus::Released
        );
    }

    /// Metadata over 255 bytes goes in several entries, read back in one piece.
    #[tokio::test]
    async fn long_meta_is_split() {
        let meta: Vec<u8> = (0..600u32).map(|i| i as u8).collect();
        let bytes = HandoverAnswer::new(HandoverStatus::Released, meta.clone()).encode();
        assert_eq!(&bytes[6..8], &(600u16 + 6).to_le_bytes());
        assert_eq!(&bytes[8..10], &[TAG_META, 255]);
        assert_eq!(&bytes[265..267], &[TAG_META, 255]);
        assert_eq!(&bytes[522..524], &[TAG_META, 90]);
        assert_eq!(
            HandoverAnswer::read(&mut &bytes[..]).await.unwrap().meta,
            meta
        );
    }

    /// A later version's frame reads with what this side knows: entries it doesn't know are skipped, ops and
    /// statuses it doesn't know stay numbers.
    #[tokio::test]
    async fn later_versions_and_unknown_entries() {
        let r = HandoverRequest::read(
            &mut &b"WWHO\x07\x09\x08\x00\x09\x02\xcc\xdd\x01\x02\xaa\xbb"[..],
        )
        .await
        .unwrap();
        assert_eq!(
            (r.version, r.op, r.meta),
            (7, Op::Other(9), vec![0xaa, 0xbb])
        );
        let a = HandoverAnswer::read(&mut &b"WWHO\x02\x05\x00\x00"[..])
            .await
            .unwrap();
        assert_eq!(a.status, HandoverStatus::Other(5));
    }

    #[tokio::test]
    async fn refuses_garbage() {
        assert!(matches!(
            HandoverRequest::read(&mut &b"WWHB\x01\x01\x00\x00"[..]).await,
            Err(HandoverError::BadMagic)
        ));
        assert!(matches!(
            HandoverRequest::read(&mut &b"WWHO\x01\x01\xff\xff"[..]).await,
            Err(HandoverError::TrailerTooLong(0xffff))
        ));
        assert!(matches!(
            HandoverRequest::read(&mut &b"WWHO\x01\x01\x02\x00\x01\x05"[..]).await,
            Err(HandoverError::BadTrailer)
        ));
    }
}
