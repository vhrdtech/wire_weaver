//! Compressed counted streams (API-55): a counted stream's events carry their serialized item through a codec that
//! keeps history across events (LZ4 with the items before as its dictionary), negotiated when the host opens the
//! stream. Design: `docs/api/stream_compression.md`.
//!
//! A counted stream's state is anything that implements [CountedState]: a plain [Counted] sends items as they are,
//! [Compressed] puts them through an [ItemCodec] when the host offered that codec. Generated code goes through the
//! trait, so a firmware opts a stream in by what its `stream_<name>()` returns, the API doesn't change.
//!
//! The device and the host keep the same history only while every event that went through the codec reaches the
//! host. So [Compressed] starts the history over (and marks that event with [HeadExt::codec_restart]):
//!
//! - at every open and resume;
//! - after an event it couldn't send ([CountedState::unsent]): the codec saw it, the host never will;
//! - every [Compressed::restart_every] bytes of items, so a loss on the link the device can't see costs at most
//!   that much: the host drops the events it can't decode until the next restart and reports their units lost.
//!
//! Units the producer drops before they reach the stream ([Counted::lost]) never went through the codec and need no
//! restart.

use crate::tx::Counted;
use crate::{HeadExt, Opened, ResumeError, StreamCursor, StreamHead};
use wire_weaver::shrink_wrap::Error;
use ww_client_server::stream::{StreamCodec, StreamCodecs, StreamStatus};
use ww_si::Second;

/// Default of [Compressed::restart_every]: 32 KiB of items. With a few KiB of history the events right after a
/// restart compress worse; every 32 KiB that costs a few percent of the ratio, and a link loss heals within 32 KiB.
pub const DEFAULT_RESTART_EVERY: u32 = 32 * 1024;

/// A codec that compresses a stream's items one at a time with the items before as history (API-55).
pub trait ItemCodec {
    /// The codec to use out of the host's offer, `None` when the host can't decode this one.
    fn answer(&self, offer: &StreamCodecs) -> Option<StreamCodec>;

    /// Start over: the next item references nothing before it.
    fn restart(&mut self);

    /// Serialize the next item with `item` (it writes into the buffer it is given and returns how many bytes) into
    /// the codec's own buffer, and write its compressed form into `out`. Returns the length of the item and of its
    /// compressed form. The item is in the history from here on, whether the event gets sent or not.
    fn compress(
        &mut self,
        item: impl FnOnce(&mut [u8]) -> Result<usize, Error>,
        out: &mut [u8],
    ) -> Result<(usize, usize), Error>;
}

/// State of a counted stream as generated code uses it: [Counted] itself, or [Compressed] around it.
///
/// Generated `<name>_send` asks for the [CountedState::head], writes the event's data with
/// [CountedState::write_item] and calls [CountedState::unsent] when sending failed.
pub trait CountedState {
    /// The positions and sessions.
    fn counted(&mut self) -> &mut Counted;

    /// The host opened the stream offering `codecs`: a new session, and the codec taken, if any.
    fn open_offered(
        &mut self,
        session: u32,
        codecs: Option<&StreamCodecs>,
    ) -> (Opened, Option<StreamCodec>) {
        let _ = codecs;
        (self.counted().open(session), None)
    }

    /// The host resumes from `cursor` offering `codecs` (see [Counted::resume]), and the codec taken, if any.
    fn resume_offered(
        &mut self,
        cursor: StreamCursor,
        codecs: Option<&StreamCodecs>,
    ) -> Result<(Opened, Option<StreamCodec>), ResumeError<()>> {
        let _ = codecs;
        Ok((self.counted().resume(cursor)?, None))
    }

    /// The host closed the stream or the link went down, see [Counted::detach].
    fn detach(&mut self) {
        self.counted().detach();
    }

    /// State for a read on the stream.
    fn status(&mut self) -> StreamStatus {
        self.counted().status()
    }

    /// Head of the next event, see [Counted::head].
    fn head(&mut self, units: u64, t: Option<Second>) -> Option<StreamHead> {
        self.counted().head(units, t)
    }

    /// Write the event's data into `buf`: the item as `item` serializes it, or its compressed form. Returns the
    /// length written.
    fn write_item(
        &mut self,
        buf: &mut [u8],
        item: impl FnOnce(&mut [u8]) -> Result<usize, Error>,
    ) -> Result<usize, Error> {
        item(buf)
    }

    /// The event with `head` couldn't be sent, see [Counted::unsent].
    fn unsent(&mut self, head: &StreamHead) {
        self.counted().unsent(head);
    }
}

impl CountedState for Counted {
    fn counted(&mut self) -> &mut Counted {
        self
    }
}

/// A counted stream whose items go through `codec` when the host offered it (API-55); events go as they are to a
/// host that didn't. Big (the codec's history and tables): keep it in a `static` and build it in place, see
/// [Compressed::new].
pub struct Compressed<C> {
    /// Positions and sessions.
    pub stream: Counted,
    /// The codec, with its history.
    pub codec: C,
    /// The host offered the codec: events go through it.
    on: bool,
    /// The next event starts the history over.
    restart: bool,
    /// Item bytes since the last restart.
    since_restart: u32,
    restart_every: u32,
}

impl<C: ItemCodec> Compressed<C> {
    /// A counted stream with no session yet that compresses with `codec` once a host offers it.
    pub const fn new(codec: C) -> Self {
        Compressed {
            stream: Counted::new(),
            codec,
            on: false,
            restart: true,
            since_restart: 0,
            restart_every: DEFAULT_RESTART_EVERY,
        }
    }

    /// Start the history over every `bytes` bytes of items ([DEFAULT_RESTART_EVERY] by default), so a loss on the
    /// link costs at most that much; 0 only at open, resume and after an unsent event.
    pub const fn restart_every(mut self, bytes: u32) -> Self {
        self.restart_every = bytes;
        self
    }

    /// The host decodes the codec: events go through it.
    pub fn is_compressing(&self) -> bool {
        self.on
    }

    fn take_offer(&mut self, codecs: Option<&StreamCodecs>) -> Option<StreamCodec> {
        let codec = codecs.and_then(|offer| self.codec.answer(offer));
        self.on = codec.is_some();
        self.restart = true;
        codec
    }
}

impl<C: ItemCodec> CountedState for Compressed<C> {
    fn counted(&mut self) -> &mut Counted {
        &mut self.stream
    }

    fn open_offered(
        &mut self,
        session: u32,
        codecs: Option<&StreamCodecs>,
    ) -> (Opened, Option<StreamCodec>) {
        let codec = self.take_offer(codecs);
        (self.stream.open(session), codec)
    }

    fn resume_offered(
        &mut self,
        cursor: StreamCursor,
        codecs: Option<&StreamCodecs>,
    ) -> Result<(Opened, Option<StreamCodec>), ResumeError<()>> {
        let opened = self.stream.resume(cursor)?;
        Ok((opened, self.take_offer(codecs)))
    }

    fn head(&mut self, units: u64, t: Option<Second>) -> Option<StreamHead> {
        let mut head = self.stream.head(units, t)?;
        if self.on
            && (self.restart
                || (self.restart_every > 0 && self.since_restart >= self.restart_every))
        {
            self.codec.restart();
            self.restart = false;
            self.since_restart = 0;
            head.ext.get_or_insert_with(HeadExt::default).codec_restart = true;
        }
        Some(head)
    }

    fn write_item(
        &mut self,
        buf: &mut [u8],
        item: impl FnOnce(&mut [u8]) -> Result<usize, Error>,
    ) -> Result<usize, Error> {
        if !self.on {
            return item(buf);
        }
        match self.codec.compress(item, buf) {
            Ok((item_len, len)) => {
                self.since_restart = self.since_restart.saturating_add(item_len as u32);
                Ok(len)
            }
            Err(e) => {
                // the codec may have taken the item into its history
                self.restart = true;
                Err(e)
            }
        }
    }

    fn unsent(&mut self, head: &StreamHead) {
        self.stream.unsent(head);
        // the next event starts the history over and marks it again
        self.stream.clear_codec_restart();
        self.restart = true;
    }
}

/// LZ4 with the items before as history (`ww_lz4`), answered as [StreamCodec::Lz4] to a host that offers it.
#[cfg(any(feature = "lz4", test))]
impl<const HASH: usize, const BUF: usize> ItemCodec for ww_lz4::Encoder<HASH, BUF> {
    fn answer(&self, offer: &StreamCodecs) -> Option<StreamCodec> {
        offer.lz4.then_some(StreamCodec::Lz4 {
            history: wire_weaver::shrink_wrap::UVlq32(Self::HISTORY as u32),
        })
    }

    fn restart(&mut self) {
        self.reset();
    }

    fn compress(
        &mut self,
        item: impl FnOnce(&mut [u8]) -> Result<usize, Error>,
        out: &mut [u8],
    ) -> Result<(usize, usize), Error> {
        // the longest item whose worst case block still fits `out`
        let fits = out.len().saturating_sub(16) * 255 / 256;
        let want = fits.min(self.max_chunk());
        let input = self
            .input(want)
            .map_err(|_| Error::OutOfBoundsWriteRawSlice)?;
        let len = item(&mut input[..want])?;
        let n = ww_lz4::Encoder::compress(self, len, out)
            .map_err(|_| Error::OutOfBoundsWriteRawSlice)?;
        Ok((len, n))
    }
}
