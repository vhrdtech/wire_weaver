//! The register map both slaves answer from, and the host updates waiting for the end of a transaction.

/// Bytes a staged update takes in front of its data: the offset (`u32`) and the length (`u16`).
const STAGED_HEAD: usize = 6;

/// A register map of `N` bytes, with room for `P` bytes of host updates that wait for the end of the master's
/// transaction (each takes 6 bytes more than its data).
pub(crate) struct RegisterMap<const N: usize, const P: usize> {
    pub(crate) bytes: [u8; N],
    staged: [u8; P],
    staged_len: usize,
}

/// Why a host update or read of the map didn't go through.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum MapError {
    /// Past the end of the map (`BufferTooBig` on the API), or an update larger than the staging room.
    OutOfRange,
    /// A transaction is in progress and the staging room is full: try again after it ended (the slave is idle,
    /// `is_idle()`).
    Busy,
}

impl<const N: usize, const P: usize> RegisterMap<N, P> {
    pub(crate) const fn new() -> Self {
        RegisterMap {
            bytes: [0; N],
            staged: [0; P],
            staged_len: 0,
        }
    }

    /// Writes now when `idle`, else stages the update until [apply_staged](Self::apply_staged).
    pub(crate) fn write(&mut self, offset: u32, data: &[u8], idle: bool) -> Result<(), MapError> {
        let start = offset as usize;
        if start > N || data.len() > N - start {
            return Err(MapError::OutOfRange);
        }
        if idle && self.staged_len == 0 {
            self.bytes[start..start + data.len()].copy_from_slice(data);
            return Ok(());
        }
        // staged: also when idle with older updates still staged, so they are applied in order
        if STAGED_HEAD + data.len() > P {
            return Err(MapError::OutOfRange);
        }
        if self.staged_len + STAGED_HEAD + data.len() > P {
            return Err(MapError::Busy);
        }
        let at = self.staged_len;
        self.staged[at..at + 4].copy_from_slice(&offset.to_le_bytes());
        self.staged[at + 4..at + 6].copy_from_slice(&(data.len() as u16).to_le_bytes());
        self.staged[at + 6..at + 6 + data.len()].copy_from_slice(data);
        self.staged_len = at + STAGED_HEAD + data.len();
        if idle {
            self.apply_staged();
        }
        Ok(())
    }

    pub(crate) fn read(&self, offset: u32, out: &mut [u8]) -> Result<(), MapError> {
        let start = offset as usize;
        if start > N || out.len() > N - start {
            return Err(MapError::OutOfRange);
        }
        out.copy_from_slice(&self.bytes[start..start + out.len()]);
        Ok(())
    }

    /// Applies the staged updates in the order they came, at the end of a transaction.
    #[inline]
    pub(crate) fn apply_staged(&mut self) {
        if self.staged_len != 0 {
            self.apply_staged_slow();
        }
    }

    #[inline(never)]
    fn apply_staged_slow(&mut self) {
        let mut at = 0;
        while at < self.staged_len {
            let offset = u32::from_le_bytes([
                self.staged[at],
                self.staged[at + 1],
                self.staged[at + 2],
                self.staged[at + 3],
            ]) as usize;
            let len = u16::from_le_bytes([self.staged[at + 4], self.staged[at + 5]]) as usize;
            let data = at + STAGED_HEAD;
            self.bytes[offset..offset + len].copy_from_slice(&self.staged[data..data + len]);
            at = data + len;
        }
        self.staged_len = 0;
    }
}
