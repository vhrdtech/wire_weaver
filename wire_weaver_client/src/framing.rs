//! Framers of frame based media on the host, and what is logged of their frame prefix checks.

use tracing::{debug, trace};
use ww_framer::prefix::{FrameCtx, FrameReport, PrefixStats};
use ww_link::{FramingRxOwned, FramingTxOwned, UsbFraming, UsbSeqFraming};

/// What a frame's prefix check found, logged: debug for anything but the expected frame.
pub(crate) fn log_report(report: FrameReport) {
    match report {
        FrameReport::Unchecked | FrameReport::InOrder => {}
        FrameReport::Synced(at) => trace!("rx frame prefix: synced at {at:?}"),
        other => debug!("rx frame prefix: {other:?}"),
    }
}

/// USB tx framer, picked at connect time from the interface protocol (see [ww_link::USB_INTERFACE_PROTOCOL]).
pub(crate) enum UsbTx {
    Plain(FramingTxOwned<UsbFraming>),
    Seq(FramingTxOwned<UsbSeqFraming>),
}

/// USB rx framer, see [UsbTx].
pub(crate) enum UsbRx {
    Plain(FramingRxOwned<UsbFraming>),
    Seq(FramingRxOwned<UsbSeqFraming>),
}

macro_rules! each {
    ($self:expr, $f:ident => $e:expr) => {
        match $self {
            Self::Plain($f) => $e,
            Self::Seq($f) => $e,
        }
    };
}

#[allow(dead_code)] // not every method is used by every transport
impl UsbTx {
    pub fn new(seq_prefix: bool, frame_size: usize) -> Self {
        if seq_prefix {
            UsbTx::Seq(FramingTxOwned::<UsbSeqFraming>::new(frame_size))
        } else {
            UsbTx::Plain(FramingTxOwned::<UsbFraming>::new(frame_size))
        }
    }

    pub fn write(&mut self, user_kind: u8, message: &[u8]) -> Result<bool, ()> {
        each!(self, f => f.write(user_kind, message))
    }

    pub fn flush(&mut self) -> usize {
        each!(self, f => f.flush())
    }

    pub fn buf(&self) -> &[u8] {
        each!(self, f => f.buf())
    }

    pub fn flush_to_vec(&mut self) -> Option<Vec<u8>> {
        each!(self, f => f.flush_to_vec())
    }

    pub fn reset(&mut self) {
        each!(self, f => f.reset())
    }

    /// The next frame's counter and epoch, for a framer that goes on from here (CLT-44); None without a prefix.
    pub fn frame_ctx(&mut self) -> Option<FrameCtx> {
        match self {
            UsbTx::Plain(_) => None,
            UsbTx::Seq(f) => Some(f.frame_ctx()),
        }
    }

    /// Goes on from another framer's [Self::frame_ctx].
    pub fn continue_at(&mut self, frame: FrameCtx) {
        each!(self, f => f.continue_at(frame))
    }
}

/// What a [UsbRx] holds between frames, owned (CLT-44), see [ww_framer::Carry].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RxCarry {
    pub partial: Option<(u8, usize, Vec<u8>)>,
    pub staged: Vec<u8>,
    pub next_frame: Option<FrameCtx>,
}

#[allow(dead_code)] // not every method is used by every transport
impl UsbRx {
    /// What another framer needs to go on from here; take every ready message out first.
    pub fn carry(&self) -> RxCarry {
        each!(self, f => {
            let c = f.carry();
            RxCarry {
                partial: c.partial.map(|(k, total, bytes)| (k, total, bytes.to_vec())),
                staged: c.staged.to_vec(),
                next_frame: c.next_frame,
            }
        })
    }

    /// Goes on from another framer's [Self::carry].
    pub fn continue_from(&mut self, carry: &RxCarry) -> Result<(), ()> {
        let c = ww_framer::Carry {
            partial: carry
                .partial
                .as_ref()
                .map(|(k, total, bytes)| (*k, *total, bytes.as_slice())),
            staged: &carry.staged,
            next_frame: carry.next_frame,
        };
        each!(self, f => f.continue_from(&c))
    }

    pub fn new(seq_prefix: bool, assembly_size: usize) -> Self {
        if seq_prefix {
            UsbRx::Seq(FramingRxOwned::<UsbSeqFraming>::new(assembly_size))
        } else {
            UsbRx::Plain(FramingRxOwned::<UsbFraming>::new(assembly_size))
        }
    }

    pub fn stage(&mut self, frame: &[u8]) -> Result<FrameReport, ()> {
        each!(self, f => f.stage(frame))
    }

    pub fn reassemble(&mut self) {
        each!(self, f => f.reassemble())
    }

    pub fn message(&self) -> Option<(u8, &[u8])> {
        each!(self, f => f.message())
    }

    /// For a live handover's pause (CLT-44): done with the message a read returned last (if any), then every message
    /// complete in what is staged goes to `messages`, in order. Nothing is ready afterwards, so calling it again after
    /// staging another frame takes only what that frame completed; what is left over is [Self::carry].
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
        each!(self, f => f.reset())
    }

    pub fn prefix_stats(&mut self) -> PrefixStats {
        each!(self, f => f.prefix_stats())
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
}
