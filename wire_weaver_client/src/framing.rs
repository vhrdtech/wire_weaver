//! Framers of frame based media on the host, and what is logged of their frame prefix checks.

use tracing::{debug, trace};
use wire_weaver::prelude::*;
use ww_framer::prefix::{FrameCtx, FrameReport, PrefixStats};
use ww_link::hello::{HELLO_LEN, usb as usb_hello};
use ww_link::{FramingRxOwned, FramingTxOwned, Hello, Kind, UsbFraming, UsbSeqFraming};

/// What a frame's prefix check found, logged: debug for anything but the expected frame.
pub(crate) fn log_report(report: FrameReport) {
    match report {
        FrameReport::Unchecked | FrameReport::InOrder => {}
        FrameReport::Synced(at) => trace!("rx frame prefix: synced at {at:?}"),
        other => debug!("rx frame prefix: {other:?}"),
    }
}

/// USB tx framer, picked at connect time from the interface protocol (see [ww_link::USB_INTERFACE_PROTOCOL]). The
/// hello goes outside it, in its frozen frame ([ww_link::hello::usb]), a packet of its own.
pub(crate) struct UsbTx {
    framer: UsbTxFramer,
    /// A hello written, its frame is what the next flush gives
    hello: Option<[u8; usb_hello::FRAME_LEN]>,
    /// The hello's frame was the last flush: [UsbTx::buf] is it
    flushed_hello: Option<[u8; usb_hello::FRAME_LEN]>,
}

pub(crate) enum UsbTxFramer {
    Plain(FramingTxOwned<UsbFraming>),
    Seq(FramingTxOwned<UsbSeqFraming>),
}

/// USB rx framer, see [UsbTx]. Every packet is checked for the device's hello before the framer sees it.
pub(crate) struct UsbRx {
    framer: UsbRxFramer,
    /// A hello found outside the framer, waiting to be [UsbRx::reassemble]d
    hello: Option<[u8; HELLO_LEN]>,
    /// [UsbRx::message] is that hello
    hello_ready: bool,
}

pub(crate) enum UsbRxFramer {
    Plain(FramingRxOwned<UsbFraming>),
    Seq(FramingRxOwned<UsbSeqFraming>),
}

macro_rules! each {
    (UsbTx: $self:expr, $f:ident => $e:expr) => {
        each!(UsbTxFramer: &mut $self.framer, $f => $e)
    };
    (UsbRx: mut $self:expr, $f:ident => $e:expr) => {
        each!(UsbRxFramer: &mut $self.framer, $f => $e)
    };
    (UsbRx: $self:expr, $f:ident => $e:expr) => {
        each!(UsbRxFramer: &$self.framer, $f => $e)
    };
    ($ty:ident: $self:expr, $f:ident => $e:expr) => {
        match $self {
            $ty::Plain($f) => $e,
            $ty::Seq($f) => $e,
        }
    };
}

#[allow(dead_code)] // not every method is used by every transport
impl UsbTx {
    pub fn new(seq_prefix: bool, frame_size: usize) -> Self {
        let framer = if seq_prefix {
            UsbTxFramer::Seq(FramingTxOwned::<UsbSeqFraming>::new(frame_size))
        } else {
            UsbTxFramer::Plain(FramingTxOwned::<UsbFraming>::new(frame_size))
        };
        UsbTx {
            framer,
            hello: None,
            flushed_hello: None,
        }
    }

    /// As the framer's `write`; a hello is held as its frozen frame and is the next flush alone (Ok(false) first
    /// while the framer holds anything: flush, then write it again). Packets too short for the frozen frame take
    /// the hello framed, as the device does.
    pub fn write(&mut self, user_kind: u8, message: &[u8]) -> Result<bool, ()> {
        self.flushed_hello = None;
        if user_kind == Kind::Hello as u8
            && let Ok(hello) = Hello::from_bytes(message)
            && usb_hello::FRAME_LEN <= self.frame_size()
        {
            if self.hello.is_some() || !self.framer_is_empty() {
                return Ok(false);
            }
            self.hello = Some(usb_hello::frame(&hello));
            return Ok(true);
        }
        if self.hello.is_some() {
            return Ok(false);
        }
        each!(UsbTx: self, f => f.write(user_kind, message))
    }

    fn framer_is_empty(&self) -> bool {
        match &self.framer {
            UsbTxFramer::Plain(f) => f.is_empty(),
            UsbTxFramer::Seq(f) => f.is_empty(),
        }
    }

    fn frame_size(&self) -> usize {
        match &self.framer {
            UsbTxFramer::Plain(f) => f.frame_size(),
            UsbTxFramer::Seq(f) => f.frame_size(),
        }
    }

    pub fn flush(&mut self) -> usize {
        if let Some(frame) = self.hello.take() {
            self.flushed_hello = Some(frame);
            return frame.len();
        }
        self.flushed_hello = None;
        each!(UsbTx: self, f => f.flush())
    }

    pub fn buf(&self) -> &[u8] {
        if let Some(frame) = &self.flushed_hello {
            return frame;
        }
        match &self.framer {
            UsbTxFramer::Plain(f) => f.buf(),
            UsbTxFramer::Seq(f) => f.buf(),
        }
    }

    pub fn flush_to_vec(&mut self) -> Option<Vec<u8>> {
        self.flushed_hello = None;
        if let Some(frame) = self.hello.take() {
            return Some(frame.to_vec());
        }
        each!(UsbTx: self, f => f.flush_to_vec())
    }

    pub fn reset(&mut self) {
        self.hello = None;
        self.flushed_hello = None;
        each!(UsbTx: self, f => f.reset())
    }

    /// The next frame's counter and epoch, for a framer that goes on from here (CLT-44); None without a prefix.
    pub fn frame_ctx(&mut self) -> Option<FrameCtx> {
        match &mut self.framer {
            UsbTxFramer::Plain(_) => None,
            UsbTxFramer::Seq(f) => Some(f.frame_ctx()),
        }
    }

    /// Goes on from another framer's [Self::frame_ctx].
    pub fn continue_at(&mut self, frame: FrameCtx) {
        each!(UsbTx: self, f => f.continue_at(frame))
    }
}

/// What the framers of a USB link hold between frames (CLT-44): the part of a USB medium residual that the framers of
/// the medium taking the link over go on from, see [ww_framer::Carry].
#[derive_shrink_wrap(owned, derive(Clone, Debug, Default, PartialEq, Eq))]
#[cfg_attr(not(feature = "local_socket"), allow(dead_code))]
pub(crate) struct FramerCarryOwned {
    /// The tx framer's next frame, None without a frame prefix
    pub tx_frame: Option<FrameAtOwned>,
    /// A split message the rx framer had half assembled
    pub rx_partial: Option<PartialOwned>,
    pub rx_staged: Vec<u8>,
    pub rx_next_frame: Option<FrameAtOwned>,
}

#[derive_shrink_wrap(owned, derive(Copy, Clone, Debug, PartialEq, Eq))]
#[cfg_attr(not(feature = "local_socket"), allow(dead_code))]
pub(crate) struct FrameAtOwned {
    pub seq: u32,
    pub epoch: u8,
}

#[derive_shrink_wrap(owned, derive(Clone, Debug, PartialEq, Eq))]
#[cfg_attr(not(feature = "local_socket"), allow(dead_code))]
pub(crate) struct PartialOwned {
    pub user_kind: u8,
    pub total: u32,
    pub bytes: Vec<u8>,
}

impl From<FrameCtx> for FrameAtOwned {
    fn from(f: FrameCtx) -> Self {
        FrameAtOwned {
            seq: f.seq,
            epoch: f.epoch,
        }
    }
}

impl From<FrameAtOwned> for FrameCtx {
    fn from(f: FrameAtOwned) -> Self {
        FrameCtx {
            seq: f.seq,
            epoch: f.epoch,
        }
    }
}

#[allow(dead_code)] // not every method is used by every transport
impl FramerCarryOwned {
    /// What both framers hold; take every ready message out of `rx` first.
    pub(crate) fn take(tx: &mut UsbTx, rx: &UsbRx) -> Self {
        let (rx_partial, rx_staged, rx_next_frame) = each!(UsbRx: rx, f => {
            let c = f.carry();
            (
                c.partial.map(|(user_kind, total, bytes)| PartialOwned {
                    user_kind,
                    total: total as u32,
                    bytes: bytes.to_vec(),
                }),
                c.staged.to_vec(),
                c.next_frame.map(FrameAtOwned::from),
            )
        });
        FramerCarryOwned {
            tx_frame: tx.frame_ctx().map(FrameAtOwned::from),
            rx_partial,
            rx_staged,
            rx_next_frame,
        }
    }

    /// Bounds the sizes against the medium's packets and the largest message this host takes.
    pub fn check(&self, max_packet: usize, max_message_len: usize) -> Result<(), String> {
        if let Some(p) = &self.rx_partial {
            let total = p.total as usize;
            if total > max_message_len || p.bytes.len() >= total {
                return Err(format!(
                    "partial message of {} / {} bytes",
                    p.bytes.len(),
                    p.total
                ));
            }
        }
        if self.rx_staged.len() > max_packet {
            return Err(format!("{} staged bytes", self.rx_staged.len()));
        }
        Ok(())
    }

    /// Both framers go on from here.
    pub(crate) fn apply(&self, tx: &mut UsbTx, rx: &mut UsbRx) -> Result<(), String> {
        if let Some(frame) = self.tx_frame {
            tx.continue_at(frame.into());
        }
        let c = ww_framer::Carry {
            partial: self
                .rx_partial
                .as_ref()
                .map(|p| (p.user_kind, p.total as usize, p.bytes.as_slice())),
            staged: &self.rx_staged,
            next_frame: self.rx_next_frame.map(FrameCtx::from),
        };
        each!(UsbRx: mut rx, f => f.continue_from(&c))
            .map_err(|_| "the adopted link's partial message doesn't fit".to_string())
    }
}

#[allow(dead_code)] // not every method is used by every transport
impl UsbRx {
    pub fn new(seq_prefix: bool, assembly_size: usize) -> Self {
        let framer = if seq_prefix {
            UsbRxFramer::Seq(FramingRxOwned::<UsbSeqFraming>::new(assembly_size))
        } else {
            UsbRxFramer::Plain(FramingRxOwned::<UsbFraming>::new(assembly_size))
        };
        UsbRx {
            framer,
            hello: None,
            hello_ready: false,
        }
    }

    /// As the framer's `stage`, but a packet holding a hello in its frozen frame ([ww_link::hello::usb::find]) never
    /// reaches the framer, whatever generation the device's framer is: it is the next message, and the framer
    /// drops what it held (the device started a new session). A near miss is staged as any other packet.
    pub fn stage(&mut self, frame: &[u8]) -> Result<FrameReport, ()> {
        if let Some(hello) = usb_hello::find(frame) {
            each!(UsbRx: mut self, f => f.reset());
            self.hello = Some(hello.to_bytes());
            self.hello_ready = false;
            return Ok(FrameReport::Unchecked);
        }
        each!(UsbRx: mut self, f => f.stage(frame))
    }

    pub fn reassemble(&mut self) {
        if self.hello_ready {
            self.hello = None;
            self.hello_ready = false;
        } else if self.hello.is_some() {
            self.hello_ready = true;
            return;
        }
        each!(UsbRx: mut self, f => f.reassemble())
    }

    pub fn message(&self) -> Option<(u8, &[u8])> {
        match (&self.hello, self.hello_ready) {
            (Some(hello), true) => Some((Kind::Hello as u8, hello)),
            _ => each!(UsbRx: self, f => f.message()),
        }
    }

    /// For a live handover's pause (CLT-44): done with the message a read returned last (if any), then every message
    /// complete in what is staged goes to `messages`, in order. Nothing is ready afterwards, so calling it again after
    /// staging another frame takes only what that frame completed; what is left over is [FramerCarryOwned::take].
    #[cfg(feature = "local_socket")]
    pub fn take_complete(&mut self, messages: &mut Vec<(u8, Vec<u8>)>) {
        loop {
            self.reassemble();
            match self.message() {
                Some((kind, m)) => messages.push((kind, m.to_vec())),
                None => break,
            }
        }
    }

    pub fn reset(&mut self) {
        self.hello = None;
        self.hello_ready = false;
        each!(UsbRx: mut self, f => f.reset())
    }

    pub fn prefix_stats(&mut self) -> PrefixStats {
        each!(UsbRx: mut self, f => f.prefix_stats())
    }
}

/// Tap description of a USB framing, see [UsbTx].
pub(crate) fn usb_tap_framer(seq_prefix: bool) -> ww_tap::Framer {
    if seq_prefix {
        crate::tap::FRAMED_USB_SEQ
    } else {
        crate::tap::FRAMED_USB
    }
}

#[cfg(all(test, feature = "local_socket"))]
mod tests {
    use super::*;

    /// A read returned the first message of a frame holding three: the pause takes the other two, then what each
    /// frame reaped after completes, nothing twice and nothing lost.
    #[test]
    fn take_complete_after_a_read() {
        let mut tx = UsbTx::new(true, 64);
        let mut rx = UsbRx::new(true, 1024 + 64);
        for m in [&[1u8][..], &[2, 2], &[3, 3, 3]] {
            assert_eq!(tx.write(0, m), Ok(true));
        }
        let frame = tx.flush_to_vec().unwrap();
        rx.stage(&frame).unwrap();
        rx.reassemble();
        assert_eq!(
            rx.message(),
            Some((0, &[1u8][..])),
            "what the read returned"
        );
        let mut messages = vec![];
        rx.take_complete(&mut messages);
        assert_eq!(messages, [(0, vec![2, 2]), (0, vec![3, 3, 3])]);
        // a frame reaped after: only what it completes
        assert_eq!(tx.write(0, &[4]), Ok(true));
        rx.stage(&tx.flush_to_vec().unwrap()).unwrap();
        messages.clear();
        rx.take_complete(&mut messages);
        assert_eq!(messages, [(0, vec![4])]);
        rx.take_complete(&mut messages);
        assert_eq!(messages, [(0, vec![4])], "nothing twice");
    }

    /// A message split across frames, half received when the link is handed over: the framers of the medium that
    /// takes the link go on from the carry, as bytes, and complete it; the tx side goes on with the frame counter.
    #[test]
    fn a_carry_goes_on_in_other_framers() {
        use wire_weaver::shrink_wrap::{DeserializeShrinkWrapOwned, SerializeShrinkWrapOwned};
        let mut dev_tx = UsbTx::new(true, 64);
        let mut rx = UsbRx::new(true, 1024 + 64);
        let mut tx = UsbTx::new(true, 64);
        let long: Vec<u8> = (0..100).collect();
        assert_eq!(dev_tx.write(0, &long), Ok(false), "split");
        let first = dev_tx.flush_to_vec().unwrap();
        rx.stage(&first).unwrap();
        let mut messages = vec![];
        rx.take_complete(&mut messages);
        assert!(messages.is_empty());
        assert_eq!(tx.write(0, &[1]), Ok(true));
        tx.flush_to_vec().unwrap();
        let carry = FramerCarryOwned::take(&mut tx, &rx);
        assert!(carry.rx_partial.is_some());
        carry.check(64, crate::DEFAULT_MAX_MESSAGE_LEN).unwrap();
        let bytes = carry.to_ww_bytes_owned().unwrap();
        let carry = FramerCarryOwned::from_ww_bytes_owned(&bytes).unwrap();

        let (mut tx2, mut rx2) = (UsbTx::new(true, 64), UsbRx::new(true, 1024 + 64));
        carry.apply(&mut tx2, &mut rx2).unwrap();
        assert_eq!(tx2.frame_ctx(), carry.tx_frame.map(FrameCtx::from));
        assert_eq!(dev_tx.write(0, &long), Ok(true), "the rest");
        rx2.stage(&dev_tx.flush_to_vec().unwrap()).unwrap();
        rx2.take_complete(&mut messages);
        assert_eq!(messages, [(0, long)]);
        let mut bad = carry.clone();
        bad.rx_staged = vec![0; 65];
        assert!(bad.check(64, crate::DEFAULT_MAX_MESSAGE_LEN).is_err());
    }
}
