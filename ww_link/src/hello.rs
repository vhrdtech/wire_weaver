//! The hello that opens link setup: plain bytes at fixed positions, frozen forever.
//!
//! Everything else on a link is evolvable (shrink_wrap inside [ww_framer] frames), so a break in one of those layers
//! would make the version exchange itself unreadable. The hello is read before any of them: a magic, the sender's
//! role, one generation byte per foundation layer ([Generations]) and the setup token. Two peers whose generations
//! differ learn it from the hello and say which layer differs, instead of failing on a frame they can't decode.
//!
//! Payload layout ([Hello::to_bytes]), the same on every medium:
//!
//! | offset | size | field                                                             |
//! |--------|------|-------------------------------------------------------------------|
//! | 0      | 2    | magic `WW` (`0x57 0x57`)                                          |
//! | 2      | 1    | role: `H` (`0x48`) host, `D` (`0x44`) device                      |
//! | 3      | 1    | `n`, the number of generation bytes that follow (4)               |
//! | 4      | n    | generations: framer, link, shrink_wrap, API model                 |
//! | 4 + n  | 8    | setup token, `u64` little endian                                  |
//! | 12 + n | ..   | nothing yet; a reader ignores whatever follows                    |
//!
//! A later layer gets its generation byte appended (`n` grows), so a reader takes the first four it knows and skips
//! the rest. Generations count only breaks: additions are negotiated in [DeviceInfo](crate::DeviceInfo) and
//! [LinkSetup](crate::LinkSetup), which follow the hello once every generation matches.
//!
//! The hello costs bytes only in link setup: it is the payload of [Kind::Hello](crate::Kind::Hello), sent once each
//! way per connection, and nothing is added to any message after `LinkReady`.
//!
//! Each medium freezes the frame around the payload too, and both sides write and read it outside the framer
//! ([Framing::hello_frame](crate::Framing::hello_frame), [Framing::find_hello](crate::Framing::find_hello)): on USB
//! ([usb]) one packet, so a peer whose framer is of another generation still reads the hello and says which layer
//! differs. [udp] (a datagram with a CRC-32), [rtt] (the USB bytes anywhere in the stream) and [ws] (one WebSocket
//! message) freeze theirs the same way; `embassy-net` devices send the [udp] and [ws] frames.

/// The magic a hello starts with.
pub const MAGIC: [u8; 2] = *b"WW";
/// Number of generation bytes this build sends.
pub const GENERATION_COUNT: u8 = 4;
/// Length of a hello payload this build sends.
pub const HELLO_LEN: usize = 4 + GENERATION_COUNT as usize + 8;

/// Generation of the link layer (message kinds, their payloads, link setup), see [Generations::link].
pub const GENERATION: u8 = 1;

/// Who sent a [Hello].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Role {
    Host,
    Device,
}

impl Role {
    const fn byte(self) -> u8 {
        match self {
            Role::Host => b'H',
            Role::Device => b'D',
        }
    }
}

/// One generation byte per foundation layer of a link. Each counts only breaks of its layer's bytes: an addition
/// leaves it alone, a break bumps it, and peers of different generations refuse each other by name.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Generations {
    /// [ww_framer::GENERATION]: frame heads, prefixes, checksums.
    pub framer: u8,
    /// [GENERATION]: link message kinds and payloads.
    pub link: u8,
    /// [shrink_wrap::GENERATION]: the wire format.
    pub shrink_wrap: u8,
    /// `ww_client_server::GENERATION`: requests, events and their envelopes.
    pub api_model: u8,
}

/// `framer=1 link=1 shrink_wrap=1 api_model=1`, for display: the hello generations a device and host (or the hub,
/// per client) agreed on to get this far.
impl core::fmt::Display for Generations {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "framer={} link={} shrink_wrap={} api_model={}",
            self.framer, self.link, self.shrink_wrap, self.api_model
        )
    }
}

impl Generations {
    /// This build's framer, link and shrink_wrap generations with the API model's, which this crate doesn't depend
    /// on: pass `ww_client_server::GENERATION`.
    pub const fn this_build(api_model: u8) -> Self {
        Generations {
            framer: ww_framer::GENERATION,
            link: GENERATION,
            shrink_wrap: shrink_wrap::GENERATION,
            api_model,
        }
    }

    /// The first layer whose generation differs, with this side's and the other's.
    pub fn mismatch(&self, other: &Generations) -> Option<Mismatch> {
        [
            (Layer::Framer, self.framer, other.framer),
            (Layer::Link, self.link, other.link),
            (Layer::ShrinkWrap, self.shrink_wrap, other.shrink_wrap),
            (Layer::ApiModel, self.api_model, other.api_model),
        ]
        .into_iter()
        .find(|(_, ours, theirs)| ours != theirs)
        .map(|(layer, ours, theirs)| Mismatch {
            layer,
            ours,
            theirs,
        })
    }
}

/// A foundation layer with a generation in the [Hello].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Layer {
    Framer,
    Link,
    ShrinkWrap,
    ApiModel,
}

impl Layer {
    /// The layer's name for people: `framer`, `link`, `shrink_wrap`, `API model`.
    pub const fn name(self) -> &'static str {
        match self {
            Layer::Framer => "framer",
            Layer::Link => "link",
            Layer::ShrinkWrap => "shrink_wrap",
            Layer::ApiModel => "API model",
        }
    }
}

/// A layer whose generation differs between two peers, see [Generations::mismatch].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Mismatch {
    pub layer: Layer,
    /// This side's generation.
    pub ours: u8,
    /// The other side's generation.
    pub theirs: u8,
}

/// The hello that opens link setup, see the [module](self) docs for its bytes.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Hello {
    pub role: Role,
    pub generations: Generations,
    /// The host's setup token, echoed by the device, see [Message::Hello](crate::Message::Hello).
    pub token: u64,
}

/// Why bytes are not a [Hello].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum HelloError {
    /// Shorter than its own generation count says.
    TooShort,
    /// Doesn't start with [MAGIC].
    BadMagic,
    /// Neither `H` nor `D`.
    BadRole(u8),
    /// Fewer generation bytes than the four every hello has.
    TooFewGenerations(u8),
}

impl Hello {
    /// This build's hello.
    pub const fn new(role: Role, api_model: u8, token: u64) -> Self {
        Hello {
            role,
            generations: Generations::this_build(api_model),
            token,
        }
    }

    /// The frozen payload bytes.
    pub fn to_bytes(&self) -> [u8; HELLO_LEN] {
        let g = &self.generations;
        let mut b = [0u8; HELLO_LEN];
        b[0..2].copy_from_slice(&MAGIC);
        b[2] = self.role.byte();
        b[3] = GENERATION_COUNT;
        b[4..8].copy_from_slice(&[g.framer, g.link, g.shrink_wrap, g.api_model]);
        b[8..16].copy_from_slice(&self.token.to_le_bytes());
        b
    }

    /// Reads a hello payload; bytes after it are ignored.
    pub fn from_bytes(b: &[u8]) -> Result<Self, HelloError> {
        if b.len() < 4 {
            return Err(if b.starts_with(&MAGIC[..b.len().min(2)]) {
                HelloError::TooShort
            } else {
                HelloError::BadMagic
            });
        }
        if b[0..2] != MAGIC {
            return Err(HelloError::BadMagic);
        }
        let role = match b[2] {
            b'H' => Role::Host,
            b'D' => Role::Device,
            other => return Err(HelloError::BadRole(other)),
        };
        let n = b[3];
        if n < GENERATION_COUNT {
            return Err(HelloError::TooFewGenerations(n));
        }
        let token_at = 4 + n as usize;
        let Some(token) = b.get(token_at..token_at + 8) else {
            return Err(HelloError::TooShort);
        };
        let mut t = [0u8; 8];
        t.copy_from_slice(token);
        Ok(Hello {
            role,
            generations: Generations {
                framer: b[4],
                link: b[5],
                shrink_wrap: b[6],
                api_model: b[7],
            },
            token: u64::from_le_bytes(t),
        })
    }
}

/// Largest hello frame of any medium: a buffer of this size holds [Framing::hello_frame](crate::Framing::hello_frame).
pub const HELLO_FRAME_MAX: usize = 32;

/// The hello frame on USB, frozen: one packet, never with a frame prefix (also with
/// [UsbSeqFraming](crate::UsbSeqFraming)), written and read outside the framer by both sides. It is what framer
/// generation 1 writes for a whole [Kind::Hello](crate::Kind::Hello) message, so a generation 1 framer could read it
/// too, but nothing relies on that: a peer of any framer generation finds the hello at the packet's start.
///
/// | byte | value                                                                  |
/// |------|------------------------------------------------------------------------|
/// | 0    | `0x30`                                                                 |
/// | 1    | `0x48 \| (len >> 8)`, `len` the payload length (16 today, up to 1023)  |
/// | 2    | `len & 0xff`                                                           |
/// | 3..  | the payload, see the [module](self) docs                               |
/// The head framer generation 1 writes before a whole [Kind::Hello](crate::Kind::Hello) message of `len` bytes
/// (`len` < 1024) at a byte boundary, frozen on every medium that has one: `0x30`, `0x48 | (len >> 8)`, `len & 0xff`.
pub const fn frame_head(len: usize) -> [u8; 3] {
    [0x30, 0x48 | ((len >> 8) as u8 & 0b11), len as u8]
}

/// The payload length in a [frame_head] at the start of `b`.
fn read_frame_head(b: &[u8]) -> Option<usize> {
    let [0x30, b1, b2, ..] = *b else {
        return None;
    };
    if b1 & 0xfc != 0x48 {
        return None;
    }
    Some((((b1 & 0x03) as usize) << 8) | b2 as usize)
}

pub mod usb {
    use super::*;

    /// The frame head before a 16 byte payload, as framer generation 1 writes it.
    pub const HEAD: [u8; 3] = frame_head(HELLO_LEN);
    /// Length of the whole hello frame on USB.
    pub const FRAME_LEN: usize = HEAD.len() + HELLO_LEN;

    /// The frozen frame of `hello`.
    pub fn frame(hello: &Hello) -> [u8; FRAME_LEN] {
        let mut f = [0u8; FRAME_LEN];
        f[..HEAD.len()].copy_from_slice(&HEAD);
        f[HEAD.len()..].copy_from_slice(&hello.to_bytes());
        f
    }

    /// The hello at the start of a USB packet, read without the framer: whatever generation the sender's framer is,
    /// its hello is read here. A longer payload (a later hello with more generation bytes) is read as well.
    pub fn find(packet: &[u8]) -> Option<Hello> {
        let len = read_frame_head(packet)?;
        Hello::from_bytes(packet.get(3..3 + len)?).ok()
    }
}

/// The hello datagram on UDP, frozen: one datagram, never with a frame prefix (unlike every other
/// [UdpFraming](crate::UdpFraming) datagram), written and read outside the framer by both sides. A CRC-32/ISO-HDLC
/// ([LinkCrc32](crate::LinkCrc32)) over the head and the payload follows, little endian: datagrams get corrupted,
/// and a corrupted one is not a hello.
///
/// | byte          | value                                                     |
/// |---------------|-----------------------------------------------------------|
/// | 0..3          | [frame_head] of the payload length (`30 48 10` today)     |
/// | 3..3 + len    | the payload, see the [module](self) docs                  |
/// | 3 + len..     | CRC-32 of bytes `0..3 + len`, `u32` little endian         |
pub mod udp {
    use super::*;
    use crate::LinkCrc32;
    use ww_framer::crc::CrcAlgorithm;

    /// Length of this build's hello datagram.
    pub const FRAME_LEN: usize = usb::HEAD.len() + HELLO_LEN + 4;

    /// The frozen datagram of `hello`.
    pub fn frame(hello: &Hello) -> [u8; FRAME_LEN] {
        let mut f = [0u8; FRAME_LEN];
        f[..3].copy_from_slice(&usb::HEAD);
        f[3..3 + HELLO_LEN].copy_from_slice(&hello.to_bytes());
        let crc = LinkCrc32::checksum(&f[..3], &f[3..3 + HELLO_LEN]);
        f[3 + HELLO_LEN..].copy_from_slice(&crc.to_le_bytes());
        f
    }

    /// The hello at the start of a datagram, read without the framer, its CRC checked. A longer payload (a later
    /// hello with more generation bytes) is read as well; bytes after the CRC are ignored.
    pub fn find(datagram: &[u8]) -> Option<Hello> {
        let len = read_frame_head(datagram)?;
        let payload = datagram.get(3..3 + len)?;
        let crc = datagram.get(3 + len..7 + len)?;
        if LinkCrc32::checksum(&datagram[..3], payload).to_le_bytes() != *crc {
            return None;
        }
        Hello::from_bytes(payload).ok()
    }
}

/// The hello on RTT, frozen: the bytes of [usb::frame] in the byte stream, written and read outside the framer. RTT
/// has no prefix, checksum or tail, and the stream may hold leftovers of a previous session, so [rtt::find] looks for the
/// head, magic and role anywhere in what it is given.
pub mod rtt {
    use super::*;

    /// Length of this build's hello frame on RTT.
    pub const FRAME_LEN: usize = usb::FRAME_LEN;

    /// The frozen frame of `hello`.
    pub fn frame(hello: &Hello) -> [u8; FRAME_LEN] {
        usb::frame(hello)
    }

    /// The first whole hello of `role` in `stream`, with the offsets where it starts and ends. A hello cut short at
    /// the end of `stream` is not found: read more and look again.
    pub fn find(stream: &[u8], role: Role) -> Option<(Hello, core::ops::Range<usize>)> {
        let role = role.byte();
        let mut from = 0;
        while let Some(at) = next_head(stream, from) {
            if let Some((hello, len)) = whole_at(stream, at, role) {
                return Some((hello, at..at + len));
            }
            from = at + 1;
        }
        None
    }

    /// The next position from `from` on where a hello frame may start: a `0x30` followed by a byte with the head's
    /// second byte's bits (`0x48 | len >> 8`), or by the end of `b`. Four positions at a time: a quick test skips a
    /// word without a `0x30`; in a word with one, the word xor `30 30 30 30`, or-ed with the word of the four bytes
    /// after each masked and xor `48 48 48 48`, has a zero byte exactly where both match, and the exact zero byte test
    /// finds the first. A run of `0x30` (ASCII zeros) costs a few instructions a byte more, never a loop per byte.
    #[inline]
    fn next_head(b: &[u8], from: usize) -> Option<usize> {
        const SEVEN: u32 = 0x7f7f_7f7f;
        let rest = b.get(from..)?;
        let (words, remainder) = rest.as_chunks::<4>();
        let mut at = from;
        for w in words {
            let raw = u32::from_le_bytes(*w);
            let first = raw ^ 0x3030_3030;
            // no `0x30` at all, the usual word: one quick test (it may see one that isn't there, never miss one)
            if first.wrapping_sub(0x0101_0101) & !first & 0x8080_8080 == 0 {
                at += 4;
                continue;
            }
            // after the last byte, the end of `b`: a head may start there
            let after = b.get(at + 4).copied().unwrap_or(0x48);
            let second = (((raw >> 8) | (after as u32) << 24) & 0xfcfc_fcfc) ^ 0x4848_4848;
            let v = first | second;
            let zero = !(((v & SEVEN).wrapping_add(SEVEN)) | v) & !SEVEN;
            if zero != 0 {
                return Some(at + (zero.trailing_zeros() / 8) as usize);
            }
            at += 4;
        }
        let second_ok = |at: usize| b.get(at + 1).is_none_or(|&x| x & 0xfc == 0x48);
        for (k, &byte) in remainder.iter().enumerate() {
            if byte == 0x30 && second_ok(at + k) {
                return Some(at + k);
            }
        }
        None
    }

    /// The bytes of `b` from `at` (found by [next_head]) are, as far as they go, the start of a hello frame of `role`:
    /// the magic, the role and the generation count.
    #[inline(always)]
    fn could_start(b: &[u8], at: usize, role: u8) -> bool {
        let is = |i: usize, x: u8| b.get(at + i).is_none_or(|&y| y == x);
        is(3, MAGIC[0])
            && is(4, MAGIC[1])
            && is(5, role)
            && b.get(at + 6).is_none_or(|&n| n >= GENERATION_COUNT)
    }

    /// The hello of `role` whose frame starts at `at` in `b`, with the frame's length.
    #[inline(always)]
    fn whole_at(b: &[u8], at: usize, role: u8) -> Option<(Hello, usize)> {
        if b.len() < at + 3 + HELLO_LEN || !could_start(b, at, role) {
            return None;
        }
        let rest = &b[at..];
        let len = read_frame_head(rest)?;
        let hello = Hello::from_bytes(rest.get(3..3 + len)?).ok()?;
        Some((hello, 3 + len))
    }

    /// Finds the peer's hello in a stream read in chunks, before the framer sees them: a hello split across two
    /// reads is found too. Every byte of a read is compared with the head's first byte only (a word at a time), a hit
    /// then with the next bytes of a hello; of a read's end only bytes that may still start one are kept for the next
    /// read, usually none. A hello of up to [HELLO_FRAME_MAX] bytes is found across reads.
    ///
    /// What it costs on a Cortex-M3 is measured by `examples_mcu/mcu_qemu` `qemu-rtt-scan`, see EVO-23 in
    /// FEATURES.md.
    ///
    /// The stream has no checksum, so bytes inside a message that happen to be a whole hello frame of the peer's
    /// role (head, magic, role, generation count and token) are taken for one: only a payload that carries a
    /// WireWeaver hello frame itself does that.
    pub struct Scanner {
        role: u8,
        /// The end of the last bytes scanned from where a hello may still start, `0x30` first
        tail: [u8; HELLO_FRAME_MAX - 1],
        tail_len: u8,
    }

    impl Scanner {
        /// Looks for hellos of `role`: [Role::Host] on a device, [Role::Device] on a host.
        pub const fn new(role: Role) -> Self {
            Scanner {
                role: role.byte(),
                tail: [0; HELLO_FRAME_MAX - 1],
                tail_len: 0,
            }
        }

        /// Forget the bytes kept, e.g. when the medium went down.
        pub fn reset(&mut self) {
            self.tail_len = 0;
        }

        /// The first hello in the bytes kept and `bytes`, and how many of `bytes` it ends after: the caller drops
        /// what came before it (the framer too) and scans what follows with the next call. None: no hello, `bytes`
        /// go to the framer.
        ///
        /// Never inlined: on a device it runs once per read, and one copy of it is all the flash it takes.
        #[inline(never)]
        pub fn scan(&mut self, bytes: &[u8]) -> Option<(Hello, usize)> {
            let tail_len = self.tail_len as usize;
            if tail_len == 0 {
                return self.search(bytes, 0);
            }
            self.joined(bytes, tail_len)
        }

        /// The first hello in `b` starting at `skip` or later, and where it ends counted from `skip`; none: from
        /// the first position near the end that may still start one, `b` is kept.
        fn search(&mut self, b: &[u8], skip: usize) -> Option<(Hello, usize)> {
            let cap = self.tail.len();
            let mut keep_at = None;
            let mut from = 0;
            while let Some(at) = next_head(b, from) {
                if let Some((hello, len)) = whole_at(b, at, self.role) {
                    self.tail_len = 0;
                    return Some((hello, at + len - skip));
                }
                if keep_at.is_none() && b.len() - at <= cap && could_start(b, at, self.role) {
                    keep_at = Some(at);
                }
                from = at + 1;
            }
            self.tail_len = 0;
            if let Some(at) = keep_at {
                self.tail[..b.len() - at].copy_from_slice(&b[at..]);
                self.tail_len = (b.len() - at) as u8;
            }
            None
        }

        /// The kept bytes looked like the start of a hello, rare: they are searched joined with the start of
        /// `bytes`, then `bytes` alone unless all of them were in the join.
        #[cold]
        #[inline(never)]
        fn joined(&mut self, bytes: &[u8], tail_len: usize) -> Option<(Hello, usize)> {
            let mut joined = [0u8; 2 * HELLO_FRAME_MAX];
            let head = bytes.len().min(HELLO_FRAME_MAX);
            joined[..tail_len].copy_from_slice(&self.tail[..tail_len]);
            joined[tail_len..tail_len + head].copy_from_slice(&bytes[..head]);
            if let Some(found) = self.search(&joined[..tail_len + head], tail_len) {
                return Some(found);
            }
            if head == bytes.len() {
                // what to keep was taken from the join, which holds all of `bytes`
                return None;
            }
            self.search(bytes, 0)
        }
    }
}

/// The hello on WebSocket, frozen: one binary WebSocket message, the kind byte 4 ([Kind::Hello](crate::Kind::Hello))
/// and the payload, the way `ww_device::ws` and the `ws` transport of `wire_weaver_client` carry every link message
/// (WebSocket frames the stream, `ww_framer` isn't used).
pub mod ws {
    use super::*;

    /// The kind byte before the payload.
    pub const KIND: u8 = 4;
    /// Length of this build's hello message.
    pub const FRAME_LEN: usize = 1 + HELLO_LEN;

    /// The frozen message of `hello`.
    pub fn frame(hello: &Hello) -> [u8; FRAME_LEN] {
        let mut f = [0u8; FRAME_LEN];
        f[0] = KIND;
        f[1..].copy_from_slice(&hello.to_bytes());
        f
    }

    /// The hello in a binary WebSocket message.
    pub fn find(message: &[u8]) -> Option<Hello> {
        match message {
            [KIND, payload @ ..] => Hello::from_bytes(payload).ok(),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use crate::{FramingTx, Kind, RttChecksum, RttHead, RttTail, UsbFraming};

    const HOST: Hello = Hello::new(Role::Host, 1, 0x0102_0304_0506_0708);

    #[test]
    fn generations_display() {
        let g = Generations {
            framer: 1,
            link: 1,
            shrink_wrap: 1,
            api_model: 2,
        };
        assert_eq!(
            std::format!("{g}"),
            "framer=1 link=1 shrink_wrap=1 api_model=2"
        );
    }

    /// The frozen bytes: these never change, whatever generation any layer goes to.
    #[test]
    fn frozen_payload_bytes() {
        assert_eq!(
            HOST.to_bytes(),
            [
                b'W', b'W', b'H', 4, 1, 1, 1, 1, 0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01
            ]
        );
        let device = Hello {
            role: Role::Device,
            ..HOST
        };
        assert_eq!(device.to_bytes()[2], b'D');
        assert_eq!(Hello::from_bytes(&device.to_bytes()), Ok(device));
    }

    /// The USB head is what framer generation 1 writes for a hello: [usb::HEAD] stays as it is when the framer
    /// changes, and this test tells when the framer stops writing it.
    #[test]
    fn usb_head_is_what_framer_gen_1_writes() {
        let mut packet = [0u8; 64];
        let mut tx = FramingTx::<UsbFraming>::new(&mut packet);
        tx.write(Kind::Hello as u8, &HOST.to_bytes()).unwrap();
        let len = tx.flush();
        assert_eq!(&packet[..len], &usb::frame(&HOST)[..]);
        assert_eq!(usb::find(&packet[..len]), Some(HOST));
        assert_eq!(usb::find(&packet[1..len]), None);
        assert_eq!(usb::find(&packet[..len - 1]), None, "cut short");
        // a later hello with a fifth generation byte: 17 bytes of payload, still found
        let mut longer = std::vec![0x30, 0x48, 17, b'W', b'W', b'D', 5, 2, 1, 1, 1, 9];
        longer.extend_from_slice(&7u64.to_le_bytes());
        let found = usb::find(&longer).unwrap();
        assert_eq!((found.generations.framer, found.token), (2, 7));
    }

    /// The frozen UDP datagram, as bytes: no prefix, the CRC over head and payload (zlib's `crc32` of the first 19
    /// bytes gives the same).
    #[test]
    fn udp_frame_bytes() {
        assert_eq!(udp::frame(&HOST), UDP_GOLDEN);
        assert_eq!(udp::find(&UDP_GOLDEN), Some(HOST));
        let mut more = UDP_GOLDEN.to_vec();
        more.extend_from_slice(&[0x01, 0x02]);
        assert_eq!(udp::find(&more), Some(HOST), "bytes after it are ignored");
        let mut flipped = UDP_GOLDEN;
        flipped[10] ^= 0x10;
        assert_eq!(udp::find(&flipped), None, "a corrupted datagram");
        assert_eq!(
            udp::find(&UDP_GOLDEN[..UDP_GOLDEN.len() - 1]),
            None,
            "cut short"
        );
        // with a frame prefix, as every other datagram has: not a hello
        let mut prefixed = std::vec![0x00];
        prefixed.extend_from_slice(&UDP_GOLDEN);
        assert_eq!(udp::find(&prefixed), None);
    }

    const UDP_GOLDEN: [u8; udp::FRAME_LEN] = [
        0x30, 0x48, 0x10, b'W', b'W', b'H', 4, 1, 1, 1, 1, 0x08, 0x07, 0x06, 0x05, 0x04, 0x03,
        0x02, 0x01, 0x96, 0x45, 0x5E, 0xA5,
    ];

    /// A later, longer UDP hello is found too.
    #[test]
    fn udp_longer_hello() {
        use crate::LinkCrc32;
        use ww_framer::crc::CrcAlgorithm;
        let mut d = std::vec![0x30, 0x48, 17, b'W', b'W', b'D', 5, 2, 1, 1, 1, 9];
        d.extend_from_slice(&7u64.to_le_bytes());
        let crc = LinkCrc32::checksum(&d[..3], &d[3..]);
        d.extend_from_slice(&crc.to_le_bytes());
        let found = udp::find(&d).unwrap();
        assert_eq!((found.generations.framer, found.token), (2, 7));
    }

    /// The RTT hello is what framer generation 1 writes in stream mode, and is found anywhere in the stream.
    #[test]
    fn rtt_frame_is_what_framer_gen_1_writes() {
        let mut chunk = [0u8; 64];
        let mut tx = ww_framer::Tx::<RttHead, RttChecksum, RttTail>::new(&mut chunk);
        tx.write_full(Kind::Nop as u8, &[]).unwrap();
        tx.write_full(Kind::Hello as u8, &HOST.to_bytes()).unwrap();
        let len = tx.flush();
        // a Nop is two bytes: `30 30`
        assert_eq!(&chunk[..2], [0x30, 0x30]);
        assert_eq!(&chunk[2..len], &rtt::frame(&HOST)[..]);
        assert_eq!(rtt::find(&chunk[..len], Role::Host), Some((HOST, 2..len)));
        assert_eq!(rtt::find(&chunk[..len], Role::Device), None);
        assert_eq!(rtt::find(&chunk[..len - 1], Role::Host), None, "cut short");
        // a near miss of a previous session before it
        let mut stream = std::vec![0x30, 0x48, 0x10, b'W', 0xFF];
        stream.extend_from_slice(&chunk[..len]);
        assert_eq!(
            rtt::find(&stream, Role::Host),
            Some((HOST, 7..stream.len()))
        );
    }

    /// A hello split across reads is found, at any cut; one wholly in a chunk too, with what follows it left for
    /// the next call; a host hello is not a device's.
    #[test]
    fn rtt_scanner_across_reads() {
        let device = Hello {
            role: Role::Device,
            ..HOST
        };
        let mut stream = std::vec![0x30, 0x30, 0xAA, 0x30, 0x48];
        let at = stream.len();
        stream.extend_from_slice(&rtt::frame(&device));
        stream.extend_from_slice(&[0x30, 0x30]);
        let end = at + rtt::FRAME_LEN;
        for cut in 0..stream.len() {
            let mut s = rtt::Scanner::new(Role::Device);
            let (a, b) = stream.split_at(cut);
            let found = match s.scan(a) {
                Some((h, e)) => Some((h, e)),
                None => s.scan(b).map(|(h, e)| (h, cut + e)),
            };
            assert_eq!(found, Some((device, end)), "cut at {cut}");
        }
        // three bytes at a time
        let mut s = rtt::Scanner::new(Role::Device);
        let mut found = None;
        for (i, c) in stream.chunks(3).enumerate() {
            if let Some((h, e)) = s.scan(c) {
                found = Some((h, i * 3 + e));
                break;
            }
        }
        assert_eq!(found, Some((device, end)));
        assert_eq!(rtt::Scanner::new(Role::Host).scan(&stream), None);
        // what a device keeps for it, in its `StreamRx`
        assert_eq!(core::mem::size_of::<rtt::Scanner>(), 33);
    }

    #[test]
    fn ws_frame_bytes() {
        assert_eq!(ws::KIND, Kind::Hello as u8);
        let f = ws::frame(&HOST);
        assert_eq!(f[0], 4);
        assert_eq!(f[1..], HOST.to_bytes());
        assert_eq!(ws::find(&f), Some(HOST));
        assert_eq!(ws::find(&f[1..]), None);
        assert_eq!(ws::find(&f[..f.len() - 1]), None, "cut short");
    }

    #[test]
    fn later_generations_and_extra_bytes_are_read() {
        // a later peer with a fifth layer and a longer payload: the four known generations and the token are read
        let mut b = std::vec![b'W', b'W', b'D', 5, 2, 1, 3, 1, 9];
        b.extend_from_slice(&7u64.to_le_bytes());
        b.extend_from_slice(&[0xAA, 0xBB]);
        let hello = Hello::from_bytes(&b).unwrap();
        assert_eq!(hello.role, Role::Device);
        assert_eq!(hello.token, 7);
        let mismatch = Generations::this_build(1)
            .mismatch(&hello.generations)
            .unwrap();
        assert_eq!(
            mismatch,
            Mismatch {
                layer: Layer::Framer,
                ours: 1,
                theirs: 2
            }
        );
    }

    #[test]
    fn not_a_hello() {
        assert_eq!(Hello::from_bytes(&[]), Err(HelloError::TooShort));
        assert_eq!(
            Hello::from_bytes(&[0x01, 0x02, 0, 0, 0, 0, 0, 0]),
            Err(HelloError::BadMagic)
        );
        assert_eq!(Hello::from_bytes(b"WWX4"), Err(HelloError::BadRole(b'X')));
        assert_eq!(
            Hello::from_bytes(&[b'W', b'W', b'H', 3, 1, 1, 1]),
            Err(HelloError::TooFewGenerations(3))
        );
        assert_eq!(
            Hello::from_bytes(&HOST.to_bytes()[..15]),
            Err(HelloError::TooShort)
        );
    }
}
