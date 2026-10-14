//! [NorMedium]: a [LogMedium] on an `embedded-storage-async` [NorFlash] (QSPI flash, a chip's own flash).

use super::LogMedium;
use embedded_storage_async::nor_flash::NorFlash;

/// A [LogMedium] on a whole [NorFlash]; give it a partition (`embassy_embedded_hal::flash::partition`) to use a
/// part of one. The flash has to read at any offset (`READ_SIZE` 1, as QSPI NOR and most MCU flash do).
pub struct NorMedium<F: NorFlash>(pub F);

impl<F: NorFlash> LogMedium for NorMedium<F> {
    type Error = F::Error;
    const WRITE_SIZE: usize = F::WRITE_SIZE;
    const ERASE_SIZE: usize = F::ERASE_SIZE;
    const NEEDS_ERASE: bool = true;

    fn capacity(&self) -> u64 {
        const {
            assert!(
                F::READ_SIZE == 1,
                "NorMedium needs a flash that reads at any offset"
            )
        };
        self.0.capacity() as u64
    }

    async fn read(&mut self, offset: u64, buf: &mut [u8]) -> Result<(), F::Error> {
        self.0.read(offset as u32, buf).await
    }

    async fn write(&mut self, offset: u64, data: &[u8]) -> Result<(), F::Error> {
        self.0.write(offset as u32, data).await
    }

    async fn erase(&mut self, from: u64, to: u64) -> Result<(), F::Error> {
        self.0.erase(from as u32, to as u32).await
    }
}
