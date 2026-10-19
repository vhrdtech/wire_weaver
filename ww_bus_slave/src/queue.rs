//! The answer queue: `Q` answers of up to `A` bytes each, filled by the host (`slave_answers`), taken one per master
//! read (I2C) or frame (SPI).

/// Why an answer wasn't queued.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum QueueError {
    /// Every slot is taken; the credit sink never sends more answers than [free slots](crate::I2cSlave::free_answer_slots),
    /// so this is a host that ignores its credits.
    Full,
    /// Longer than an answer slot.
    TooLong,
}

/// A ring of answer slots. The slot an answer is sent from stays taken while it is sent (and, for SPI with
/// `repeat_last`, until the next answer replaces it), so the host never writes over bytes going out.
pub(crate) struct AnswerQueue<const Q: usize, const A: usize> {
    slots: [[u8; A]; Q],
    lens: [u16; Q],
    /// Oldest taken slot: the one being sent when `current`, else the next to send.
    head: usize,
    /// Slots taken, `current` included.
    held: usize,
    /// The slot at `head` is the answer being sent (or kept to repeat).
    current: bool,
}

impl<const Q: usize, const A: usize> AnswerQueue<Q, A> {
    pub(crate) const fn new() -> Self {
        AnswerQueue {
            slots: [[0; A]; Q],
            lens: [0; Q],
            head: 0,
            held: 0,
            current: false,
        }
    }

    pub(crate) fn push(&mut self, answer: &[u8]) -> Result<(), QueueError> {
        if answer.len() > A {
            return Err(QueueError::TooLong);
        }
        if self.held == Q {
            return Err(QueueError::Full);
        }
        let mut slot = self.head + self.held;
        if slot >= Q {
            slot -= Q;
        }
        self.slots[slot][..answer.len()].copy_from_slice(answer);
        self.lens[slot] = answer.len() as u16;
        self.held += 1;
        Ok(())
    }

    /// Answers waiting, not counting the one being sent.
    #[inline(always)]
    pub(crate) fn waiting(&self) -> usize {
        self.held - self.current as usize
    }

    pub(crate) fn free(&self) -> usize {
        Q - self.held
    }

    /// Makes the next waiting answer the current one (releasing the previous current one); false if none waits.
    #[inline(always)]
    pub(crate) fn take(&mut self) -> bool {
        if self.waiting() == 0 {
            return false;
        }
        if self.current {
            self.release_head();
        }
        self.current = true;
        true
    }

    /// Puts the current answer back in front of the waiting ones, as if it wasn't taken.
    pub(crate) fn untake(&mut self) {
        self.current = false;
    }

    /// Releases the current answer's slot.
    #[inline(always)]
    pub(crate) fn finish(&mut self) {
        if self.current {
            self.release_head();
            self.current = false;
        }
    }

    #[inline(always)]
    fn release_head(&mut self) {
        self.head += 1;
        if self.head == Q {
            self.head = 0;
        }
        self.held -= 1;
    }

    /// The current answer's bytes; empty without one.
    #[inline(always)]
    pub(crate) fn current(&self) -> &[u8] {
        if self.current {
            &self.slots[self.head][..self.lens[self.head] as usize]
        } else {
            &[]
        }
    }

    #[inline(always)]
    pub(crate) fn has_current(&self) -> bool {
        self.current
    }

    /// Drops every waiting answer (not the current one) and returns how many.
    pub(crate) fn clear(&mut self) -> u32 {
        let dropped = self.waiting();
        self.held = self.current as usize;
        dropped as u32
    }

    /// Drops everything, the current answer included.
    pub(crate) fn reset(&mut self) {
        self.head = 0;
        self.held = 0;
        self.current = false;
    }
}
