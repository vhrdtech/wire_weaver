//! A small stand-in for the chunky plan format: flash geometry, firmware images, the plan header, the steps and their
//! byte form, and a planner. The real formats are defined in octets by the chunky protocol spec (`SPEC.md` in
//! `chunky_mcu_updater`); `ww_dfu` only carries them, so any byte layout works for the scenarios as long as host and
//! device agree. This one is kept simple enough to read in a transcript.
//!
//! Header, 35 bytes, little endian:
//!
//! | offset | size | field                                                          |
//! |--------|------|----------------------------------------------------------------|
//! | 0      | 4    | magic `WWDP`                                                   |
//! | 4      | 1    | format, `1`                                                    |
//! | 5      | 4    | flash geometry id                                              |
//! | 9      | 4    | version of the image the plan produces                         |
//! | 13     | 2    | number of steps                                                |
//! | 15     | 4    | length of the plan after the header (the steps, WRITE data in) |
//! | 19     | 8    | digest of the image the plan produces                          |
//! | 27     | 8    | "signature": a keyed digest of bytes 0..27                     |
//!
//! Steps after the header: ERASE `01 block:u16`, COPY `02 dst:u16 src:u16 hash:[8]`, WRITE `03 dst:u16 hash:[8]
//! len:u16 data`. A chunk hash (and every digest here) is SHA-256 truncated to 8 bytes, as in the spec.

use sha2::{Digest, Sha256};

/// Bytes per chunk: the unit hashed in the manifest and written by one WRITE.
pub const CHUNK: usize = 64;
/// Chunks per slot.
pub const CHUNKS: usize = 8;
/// Bytes per slot.
pub const SLOT: usize = CHUNK * CHUNKS;
/// Bytes per erase block.
pub const ERASE: usize = 2 * CHUNK;
/// Erase blocks per slot.
pub const BLOCKS: usize = SLOT / ERASE;
/// Geometry id of the simulated chip.
pub const GEOMETRY_ID: u32 = 0x5157_0200;
/// Length of the plan header.
pub const HEADER_LEN: usize = 35;
/// Key of the toy release signature (a real one is an Ed25519 or ECDSA signature over the descriptor).
const SIGNING_KEY: &[u8] = b"release key";

/// SHA-256 truncated to 8 bytes.
pub fn digest(bytes: &[u8]) -> [u8; 8] {
    Sha256::digest(bytes)[..8].try_into().unwrap()
}

/// Hash of one chunk, as the manifest and the steps carry it.
pub fn chunk_hash(chunk: &[u8]) -> [u8; 8] {
    digest(chunk)
}

/// True if every byte reads as erased.
pub fn is_blank(bytes: &[u8]) -> bool {
    bytes.iter().all(|b| *b == 0xFF)
}

/// A firmware image of `version`: chunk 0 starts with `FW` and the version, the other chunks are named pieces of
/// "code", `None` is a blank chunk.
pub fn image(version: u32, chunks: [Option<&str>; CHUNKS]) -> Vec<u8> {
    let mut img = Vec::with_capacity(SLOT);
    for (i, name) in chunks.iter().enumerate() {
        let mut chunk = [0xFFu8; CHUNK];
        if let Some(name) = name {
            // deterministic filler: the name, then a counter mixed with the name's digest
            let seed = digest(name.as_bytes());
            for (j, b) in chunk.iter_mut().enumerate() {
                *b = seed[j % 8].wrapping_add(j as u8 * 3);
            }
            let tag = name.as_bytes();
            chunk[..tag.len()].copy_from_slice(tag);
        }
        if i == 0 {
            chunk[..2].copy_from_slice(b"FW");
            chunk[2..6].copy_from_slice(&version.to_le_bytes());
        }
        img.extend_from_slice(&chunk);
    }
    img
}

/// Version of the image in `slot`, if chunk 0 looks like an image header.
pub fn image_version(slot: &[u8]) -> Option<u32> {
    (&slot[..2] == b"FW").then(|| u32::from_le_bytes(slot[2..6].try_into().unwrap()))
}

/// The image the device ships with.
pub fn v1() -> Vec<u8> {
    image(
        1,
        [
            Some("boot"),
            Some("hal"),
            Some("app"),
            Some("net"),
            Some("ui"),
            Some("font"),
            None,
            None,
        ],
    )
}

/// The update: a new header chunk, `hal` and `ui` unchanged, `font` moved, `app` replaced, new code after it.
pub fn v2() -> Vec<u8> {
    image(
        2,
        [
            Some("boot"),
            Some("hal"),
            Some("app v2"),
            Some("font"),
            Some("ui"),
            Some("net v2"),
            Some("ota"),
            None,
        ],
    )
}

/// One plan step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Erase { block: u16 },
    Copy { dst: u16, src: u16, hash: [u8; 8] },
    Write { dst: u16, hash: [u8; 8], data: Vec<u8> },
}

impl Step {
    /// Byte form, as stored in a plan area and sent by `StageBulk`.
    pub fn encode(&self, out: &mut Vec<u8>) {
        match self {
            Step::Erase { block } => {
                out.push(1);
                out.extend_from_slice(&block.to_le_bytes());
            }
            Step::Copy { dst, src, hash } => {
                out.push(2);
                out.extend_from_slice(&dst.to_le_bytes());
                out.extend_from_slice(&src.to_le_bytes());
                out.extend_from_slice(hash);
            }
            Step::Write { dst, hash, data } => {
                out.push(3);
                out.extend_from_slice(&dst.to_le_bytes());
                out.extend_from_slice(hash);
                out.extend_from_slice(&(data.len() as u16).to_le_bytes());
                out.extend_from_slice(data);
            }
        }
    }

    /// Decode the step at the start of `bytes`: `Ok(None)` if more bytes are needed, the step and its length otherwise.
    pub fn decode(bytes: &[u8]) -> Result<Option<(Step, usize)>, ()> {
        let Some(&tag) = bytes.first() else {
            return Ok(None);
        };
        let u16_at = |at: usize| u16::from_le_bytes([bytes[at], bytes[at + 1]]);
        let hash_at = |at: usize| -> [u8; 8] { bytes[at..at + 8].try_into().unwrap() };
        Ok(match tag {
            1 if bytes.len() >= 3 => Some((Step::Erase { block: u16_at(1) }, 3)),
            2 if bytes.len() >= 13 => Some((
                Step::Copy {
                    dst: u16_at(1),
                    src: u16_at(3),
                    hash: hash_at(5),
                },
                13,
            )),
            3 if bytes.len() >= 13 => {
                let len = u16_at(11) as usize;
                if bytes.len() < 13 + len {
                    return Ok(None);
                }
                Some((
                    Step::Write {
                        dst: u16_at(1),
                        hash: hash_at(3),
                        data: bytes[13..13 + len].to_vec(),
                    },
                    13 + len,
                ))
            }
            1..=3 => None,
            _ => return Err(()),
        })
    }
}

/// What a device reads from a plan header.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    pub geometry_id: u32,
    pub version: u32,
    pub steps: u16,
    pub body_len: u32,
    pub image_digest: [u8; 8],
}

/// Why a header is refused, mapped to `ww_dfu::Error` by the device.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum HeaderError {
    Malformed,
    BadSignature,
}

impl Header {
    pub fn parse(bytes: &[u8]) -> Result<Header, HeaderError> {
        if bytes.len() != HEADER_LEN || &bytes[..4] != b"WWDP" || bytes[4] != 1 {
            return Err(HeaderError::Malformed);
        }
        if bytes[27..35] != sign(&bytes[..27]) {
            return Err(HeaderError::BadSignature);
        }
        let u32_at = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        Ok(Header {
            geometry_id: u32_at(5),
            version: u32_at(9),
            steps: u16::from_le_bytes([bytes[13], bytes[14]]),
            body_len: u32_at(15),
            image_digest: bytes[19..27].try_into().unwrap(),
        })
    }
}

fn sign(bytes: &[u8]) -> [u8; 8] {
    digest(&[SIGNING_KEY, bytes].concat())
}

/// A plan as the host has it: header octets, steps, and the steps' byte form.
#[derive(Clone, Debug)]
pub struct Plan {
    pub header: Vec<u8>,
    pub steps: Vec<Step>,
    pub body: Vec<u8>,
}

impl Plan {
    pub fn new(steps: Vec<Step>, new_image: &[u8]) -> Plan {
        let mut body = Vec::new();
        for s in &steps {
            s.encode(&mut body);
        }
        let mut header = Vec::with_capacity(HEADER_LEN);
        header.extend_from_slice(b"WWDP");
        header.push(1);
        header.extend_from_slice(&GEOMETRY_ID.to_le_bytes());
        header.extend_from_slice(&image_version(new_image).unwrap().to_le_bytes());
        header.extend_from_slice(&(steps.len() as u16).to_le_bytes());
        header.extend_from_slice(&(body.len() as u32).to_le_bytes());
        header.extend_from_slice(&digest(new_image));
        let signature = sign(&header);
        header.extend_from_slice(&signature);
        Plan {
            header,
            steps,
            body,
        }
    }

    /// A/B: the inactive slot is erased, every chunk is copied from the running image if it has it anywhere, written
    /// otherwise, blank chunks are left alone.
    pub fn ab(old: &[u8], new: &[u8]) -> Plan {
        let mut steps: Vec<Step> = (0..BLOCKS as u16).map(|block| Step::Erase { block }).collect();
        for (dst, chunk) in new.chunks(CHUNK).enumerate() {
            if is_blank(chunk) {
                continue;
            }
            let hash = chunk_hash(chunk);
            match old.chunks(CHUNK).position(|c| c == chunk) {
                Some(src) => steps.push(Step::Copy {
                    dst: dst as u16,
                    src: src as u16,
                    hash,
                }),
                None => steps.push(Step::Write {
                    dst: dst as u16,
                    hash,
                    data: chunk.to_vec(),
                }),
            }
        }
        Plan::new(steps, new)
    }

    /// A plain full image: erase everything, write every chunk that isn't blank. Works on any device, needs no
    /// manifest.
    pub fn full(new: &[u8]) -> Plan {
        let mut steps: Vec<Step> = (0..BLOCKS as u16).map(|block| Step::Erase { block }).collect();
        for (dst, chunk) in new.chunks(CHUNK).enumerate() {
            if !is_blank(chunk) {
                steps.push(Step::Write {
                    dst: dst as u16,
                    hash: chunk_hash(chunk),
                    data: chunk.to_vec(),
                });
            }
        }
        Plan::new(steps, new)
    }

    /// In place, block by block: a block that doesn't change is left alone, a changed one is erased (unless it is
    /// blank already) and refilled; a chunk is copied only from a block that comes later (still holds the old image),
    /// written otherwise.
    pub fn in_place(old: &[u8], new: &[u8]) -> Plan {
        let mut steps = Vec::new();
        for block in 0..BLOCKS {
            let range = block * ERASE..(block + 1) * ERASE;
            if old[range.clone()] == new[range.clone()] {
                continue;
            }
            if !is_blank(&old[range.clone()]) {
                steps.push(Step::Erase {
                    block: block as u16,
                });
            }
            for dst in block * ERASE / CHUNK..(block + 1) * ERASE / CHUNK {
                let chunk = &new[dst * CHUNK..(dst + 1) * CHUNK];
                if is_blank(chunk) {
                    continue;
                }
                let hash = chunk_hash(chunk);
                let later = old
                    .chunks(CHUNK)
                    .enumerate()
                    .skip((block + 1) * ERASE / CHUNK)
                    .find(|(_, c)| *c == chunk);
                match later {
                    Some((src, _)) => steps.push(Step::Copy {
                        dst: dst as u16,
                        src: src as u16,
                        hash,
                    }),
                    None => steps.push(Step::Write {
                        dst: dst as u16,
                        hash,
                        data: chunk.to_vec(),
                    }),
                }
            }
        }
        Plan::new(steps, new)
    }

    /// Byte position of each step in the body, and the end.
    pub fn step_offsets(&self) -> Vec<usize> {
        let mut at = 0;
        let mut offsets = vec![0];
        for s in &self.steps {
            let mut b = Vec::new();
            s.encode(&mut b);
            at += b.len();
            offsets.push(at);
        }
        offsets
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_rebuild_the_image() {
        let (old, new) = (v1(), v2());
        for (plan, in_place) in [
            (Plan::ab(&old, &new), false),
            (Plan::full(&new), false),
            (Plan::in_place(&old, &new), true),
        ] {
            let header = Header::parse(&plan.header).unwrap();
            assert_eq!(header.body_len as usize, plan.body.len());
            let mut slot = if in_place {
                old.clone()
            } else {
                vec![0u8; SLOT]
            };
            let mut at = 0;
            let mut steps = vec![];
            while let Some((step, len)) = Step::decode(&plan.body[at..]).unwrap() {
                steps.push(step);
                at += len;
            }
            assert_eq!(steps, plan.steps);
            for step in steps {
                match step {
                    Step::Erase { block } => {
                        let b = block as usize * ERASE;
                        slot[b..b + ERASE].fill(0xFF)
                    }
                    Step::Copy { dst, src, hash } => {
                        let src: Vec<u8> = old[src as usize * CHUNK..][..CHUNK].to_vec();
                        assert_eq!(chunk_hash(&src), hash);
                        slot[dst as usize * CHUNK..][..CHUNK].copy_from_slice(&src);
                    }
                    Step::Write { dst, data, .. } => {
                        slot[dst as usize * CHUNK..][..CHUNK].copy_from_slice(&data)
                    }
                }
            }
            assert_eq!(slot, new);
        }
    }
}
