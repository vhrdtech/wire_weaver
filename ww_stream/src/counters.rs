//! What happens to a stream's data, counted per stream: [TxStats] always, and typed `cnt` instance counters
//! ([StreamCnt]) with the `cnt` feature, so `cnt tui` on a firmware shows per stream what went where.
//!
//! ```ignore
//! static ADC_SAMPLES_CNT: cnt::Counters<StreamCnt> = cnt::counters!(StreamCnt, adc_samples);
//! let samples = Reliable::new(RamRing::<4096>::new()).counters(&ADC_SAMPLES_CNT);
//! ```

/// Events of a sending stream ([Counted](crate::tx::Counted), [Reliable](crate::tx::Reliable)), one counter each.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "cnt", derive(cnt::Count))]
pub enum StreamCnt {
    /// Units sent the first time.
    #[cfg_attr(feature = "cnt", count(u64, unit = "unit"))]
    Sent,
    /// Units sent again (go-back-N after a `Resend`).
    #[cfg_attr(feature = "cnt", count(u64, unit = "unit"))]
    Resent,
    /// Units the producer dropped before they reached the stream (its own ring overflowed, `lost()`).
    #[cfg_attr(feature = "cnt", count(u64, unit = "unit", warn))]
    LostProducer,
    /// Units the store had no room for (drop newest), or forgot to make room (drop oldest).
    #[cfg_attr(feature = "cnt", count(u64, unit = "unit", warn))]
    LostStoreFull,
    /// Units of a counted stream made while no host was there, or that couldn't be sent.
    #[cfg_attr(feature = "cnt", count(u64, unit = "unit", warn))]
    LostNoHost,
    /// Bytes of items acknowledged by the host.
    #[cfg_attr(feature = "cnt", count(u64, unit = "B"))]
    Acked,
    /// `Resend` requests from the host.
    #[cfg_attr(feature = "cnt", count(warn))]
    ResendRequests,
    /// Acknowledgements beyond what was sent (a host bug, or a late one from an old session), ignored.
    #[cfg_attr(feature = "cnt", count(warn))]
    Overruns,
}

/// [StreamCnt] as plain numbers, kept by every sending stream (`stats()`), with or without the `cnt` feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TxStats {
    pub sent: u64,
    pub resent: u64,
    pub lost_producer: u64,
    pub lost_store_full: u64,
    pub lost_no_host: u64,
    pub acked_bytes: u64,
    pub resend_requests: u64,
    pub overruns: u64,
}

impl TxStats {
    /// All units lost on the device, whatever the cause.
    pub fn lost(&self) -> u64 {
        self.lost_producer + self.lost_store_full + self.lost_no_host
    }
}

/// [TxStats] and, with the `cnt` feature, the instance counters given by the firmware.
#[derive(Default)]
pub(crate) struct Counting {
    pub(crate) stats: TxStats,
    #[cfg(feature = "cnt")]
    pub(crate) cnt: Option<&'static cnt::Counters<StreamCnt>>,
}

impl Counting {
    pub(crate) const fn new() -> Self {
        Counting {
            stats: TxStats {
                sent: 0,
                resent: 0,
                lost_producer: 0,
                lost_store_full: 0,
                lost_no_host: 0,
                acked_bytes: 0,
                resend_requests: 0,
                overruns: 0,
            },
            #[cfg(feature = "cnt")]
            cnt: None,
        }
    }

    pub(crate) fn add(&mut self, event: StreamCnt, n: u64) {
        if n == 0 {
            return;
        }
        let s = &mut self.stats;
        let field = match event {
            StreamCnt::Sent => &mut s.sent,
            StreamCnt::Resent => &mut s.resent,
            StreamCnt::LostProducer => &mut s.lost_producer,
            StreamCnt::LostStoreFull => &mut s.lost_store_full,
            StreamCnt::LostNoHost => &mut s.lost_no_host,
            StreamCnt::Acked => &mut s.acked_bytes,
            StreamCnt::ResendRequests => &mut s.resend_requests,
            StreamCnt::Overruns => &mut s.overruns,
        };
        *field = field.saturating_add(n);
        #[cfg(feature = "cnt")]
        if let Some(cnt) = self.cnt {
            cnt.add(event, n);
        }
    }
}
