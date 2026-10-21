//! Framers of frame based media on the host, and what is logged of their frame prefix checks.

use tracing::{debug, trace};
use ww_framer::prefix::{FrameReport, PrefixStats};
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
}

#[allow(dead_code)] // not every method is used by every transport
impl UsbRx {
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
