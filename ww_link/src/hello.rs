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
//! differs.

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
pub mod usb {
    use super::*;

    /// The frame head before a 16 byte payload, as framer generation 1 writes it.
    pub const HEAD: [u8; 3] = [0x30, 0x48, 0x10];
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
        let [0x30, b1, b2, ..] = *packet else {
            return None;
        };
        if b1 & 0xfc != 0x48 {
            return None;
        }
        let len = (((b1 & 0x03) as usize) << 8) | b2 as usize;
        Hello::from_bytes(packet.get(3..3 + len)?).ok()
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use crate::{FramingTx, Kind, UsbFraming};

    const HOST: Hello = Hello::new(Role::Host, 1, 0x0102_0304_0506_0708);

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
