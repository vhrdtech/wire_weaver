//! Firmware images to write: UF2 files and raw binaries, as segments of flash addresses and bytes.

use crate::Error;
use crate::proto::{FLASH_BASE, FLASH_PAGE_SIZE, FLASH_SECTOR_SIZE};

const UF2_BLOCK_SIZE: usize = 512;
const UF2_MAGIC_START0: u32 = 0x0A32_4655;
const UF2_MAGIC_START1: u32 = 0x9E5D_5157;
const UF2_MAGIC_END: u32 = 0x0AB1_6F30;
const UF2_FLAG_NOT_MAIN_FLASH: u32 = 0x0000_0001;
const UF2_FLAG_FAMILY_ID_PRESENT: u32 = 0x0000_2000;

/// UF2 family ids of Raspberry Pi chips.
pub mod family {
    pub const RP2040: u32 = 0xe48b_ff56;
    /// Not a chip: blocks placed at their absolute address whatever the partition table says.
    pub const ABSOLUTE: u32 = 0xe48b_ff57;
    /// Not a chip: data blocks.
    pub const DATA: u32 = 0xe48b_ff58;
    pub const RP2350_ARM_S: u32 = 0xe48b_ff59;
    pub const RP2350_RISCV: u32 = 0xe48b_ff5a;
    pub const RP2350_ARM_NS: u32 = 0xe48b_ff5b;
}

/// Address of the dummy block picotool adds to RP2350 UF2 files as the RP2350-E10 errata workaround; it is not part
/// of the image and is skipped.
const RP2350_E10_DUMMY_ADDR: u32 = 0x10ff_ff00;

/// A contiguous run of bytes at a flash address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment {
    pub addr: u32,
    pub data: Vec<u8>,
}

/// A firmware image: non-overlapping segments in flash, sorted by address.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Image {
    pub segments: Vec<Segment>,
}

impl Image {
    /// A raw binary written at `addr` (usually [FLASH_BASE]).
    pub fn from_bin(addr: u32, data: Vec<u8>) -> Result<Self, Error> {
        let mut image = Image::default();
        image.add(addr, &data)?;
        Ok(image)
    }

    /// The flash blocks of a UF2 file. Blocks not meant for main flash and the RP2350-E10 dummy block are skipped.
    /// `families`: the family ids to take, or all when empty; a file with blocks of none of them is an error.
    pub fn from_uf2(bytes: &[u8], families: &[u32]) -> Result<Self, Error> {
        if bytes.is_empty() || !bytes.len().is_multiple_of(UF2_BLOCK_SIZE) {
            return Err(Error::Image(format!(
                "UF2 file size {} is not a multiple of {UF2_BLOCK_SIZE}",
                bytes.len()
            )));
        }
        let mut image = Image::default();
        let mut seen_families = vec![];
        for (i, block) in bytes.as_chunks::<UF2_BLOCK_SIZE>().0.iter().enumerate() {
            let word = |at: usize| u32::from_le_bytes(block[at..at + 4].try_into().unwrap());
            if word(0) != UF2_MAGIC_START0
                || word(4) != UF2_MAGIC_START1
                || word(UF2_BLOCK_SIZE - 4) != UF2_MAGIC_END
            {
                return Err(Error::Image(format!("UF2 block {i}: bad magic")));
            }
            let flags = word(8);
            let addr = word(12);
            let len = word(16) as usize;
            if flags & UF2_FLAG_NOT_MAIN_FLASH != 0 {
                continue;
            }
            let family = (flags & UF2_FLAG_FAMILY_ID_PRESENT != 0).then(|| word(28));
            if family == Some(family::ABSOLUTE) && addr == RP2350_E10_DUMMY_ADDR {
                continue;
            }
            if let Some(f) = family
                && !seen_families.contains(&f)
            {
                seen_families.push(f);
            }
            if !families.is_empty() && !family.is_some_and(|f| families.contains(&f)) {
                continue;
            }
            if len > 476 {
                return Err(Error::Image(format!(
                    "UF2 block {i}: payload size {len} > 476"
                )));
            }
            image.add(addr, &block[32..32 + len])?;
        }
        if image.segments.is_empty() {
            return Err(Error::Image(format!(
                "UF2 file has no flash blocks for families {families:x?} (found {seen_families:x?})"
            )));
        }
        Ok(image)
    }

    /// Add bytes at `addr`, merged with an adjacent segment; overlapping an existing one is an error.
    pub fn add(&mut self, addr: u32, data: &[u8]) -> Result<(), Error> {
        if data.is_empty() {
            return Ok(());
        }
        let end = addr as u64 + data.len() as u64;
        if addr < FLASH_BASE || end > u32::MAX as u64 {
            return Err(Error::Image(format!(
                "{} bytes at {addr:#010x} are outside flash",
                data.len()
            )));
        }
        let pos = self.segments.partition_point(|s| s.addr < addr);
        if let Some(next) = self.segments.get(pos)
            && end > next.addr as u64
        {
            return Err(Error::Image(format!(
                "data at {addr:#010x} overlaps data at {:#010x}",
                next.addr
            )));
        }
        if pos > 0 {
            let prev = &mut self.segments[pos - 1];
            let prev_end = prev.addr as u64 + prev.data.len() as u64;
            if prev_end > addr as u64 {
                return Err(Error::Image(format!(
                    "data at {addr:#010x} overlaps data at {:#010x}",
                    prev.addr
                )));
            }
            if prev_end == addr as u64 {
                prev.data.extend_from_slice(data);
                if self
                    .segments
                    .get(pos)
                    .is_some_and(|next| next.addr as u64 == end)
                {
                    let next = self.segments.remove(pos);
                    self.segments[pos - 1].data.extend(next.data);
                }
                return Ok(());
            }
        }
        if let Some(next) = self.segments.get_mut(pos)
            && next.addr as u64 == end
        {
            let mut joined = data.to_vec();
            joined.append(&mut next.data);
            next.addr = addr;
            next.data = joined;
            return Ok(());
        }
        self.segments.insert(
            pos,
            Segment {
                addr,
                data: data.to_vec(),
            },
        );
        Ok(())
    }

    /// Image bytes in total.
    pub fn len(&self) -> usize {
        self.segments.iter().map(|s| s.data.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    /// The flash sectors the image touches, each as a full sector: erased bytes (`0xFF`) where the image has none.
    pub fn sectors(&self) -> Vec<Segment> {
        let sector = FLASH_SECTOR_SIZE as u32;
        let mut sectors: Vec<Segment> = vec![];
        for s in &self.segments {
            let mut addr = s.addr;
            let mut data = &s.data[..];
            while !data.is_empty() {
                let base = addr - addr % sector;
                if sectors.last().is_none_or(|last| last.addr != base) {
                    sectors.push(Segment {
                        addr: base,
                        data: vec![0xFF; FLASH_SECTOR_SIZE],
                    });
                }
                let offset = (addr - base) as usize;
                let n = data.len().min(FLASH_SECTOR_SIZE - offset);
                let last = sectors.last_mut().unwrap();
                last.data[offset..offset + n].copy_from_slice(&data[..n]);
                addr += n as u32;
                data = &data[n..];
            }
        }
        sectors
    }
}

// a sector is a whole number of pages, the writer relies on it
const _: () = assert!(FLASH_SECTOR_SIZE.is_multiple_of(FLASH_PAGE_SIZE));

#[cfg(test)]
mod tests {
    use super::*;

    /// One UF2 block, as picotool writes them.
    pub(crate) fn uf2_block(flags: u32, addr: u32, payload: &[u8], family: u32) -> Vec<u8> {
        let mut b = vec![0u8; UF2_BLOCK_SIZE];
        let mut put = |at: usize, v: u32| b[at..at + 4].copy_from_slice(&v.to_le_bytes());
        put(0, UF2_MAGIC_START0);
        put(4, UF2_MAGIC_START1);
        put(8, flags);
        put(12, addr);
        put(16, payload.len() as u32);
        put(20, 0);
        put(24, 1);
        put(28, family);
        put(UF2_BLOCK_SIZE - 4, UF2_MAGIC_END);
        b[32..32 + payload.len()].copy_from_slice(payload);
        b
    }

    #[test]
    fn uf2_blocks_merge_and_skip_e10_dummy() {
        let mut file = uf2_block(
            UF2_FLAG_FAMILY_ID_PRESENT,
            RP2350_E10_DUMMY_ADDR,
            &[0xEF; 256],
            family::ABSOLUTE,
        );
        file.extend(uf2_block(
            UF2_FLAG_FAMILY_ID_PRESENT,
            FLASH_BASE + 256,
            &[2; 256],
            family::RP2350_ARM_S,
        ));
        file.extend(uf2_block(
            UF2_FLAG_FAMILY_ID_PRESENT,
            FLASH_BASE,
            &[1; 256],
            family::RP2350_ARM_S,
        ));
        file.extend(uf2_block(
            UF2_FLAG_NOT_MAIN_FLASH,
            0x2000_0000,
            &[3; 256],
            0,
        ));
        let image = Image::from_uf2(&file, &[family::RP2350_ARM_S]).unwrap();
        assert_eq!(image.segments.len(), 1);
        assert_eq!(image.segments[0].addr, FLASH_BASE);
        assert_eq!(&image.segments[0].data[..256], &[1; 256]);
        assert_eq!(&image.segments[0].data[256..], &[2; 256]);

        let err = Image::from_uf2(&file, &[family::RP2040]).unwrap_err();
        assert!(err.to_string().contains("e48bff59"), "{err}");
    }

    #[test]
    fn overlap_is_an_error() {
        let mut image = Image::from_bin(FLASH_BASE, vec![0; 100]).unwrap();
        assert!(image.add(FLASH_BASE + 50, &[1; 10]).is_err());
        image.add(FLASH_BASE + 200, &[1; 10]).unwrap();
        image.add(FLASH_BASE + 100, &[2; 100]).unwrap();
        assert_eq!(image.segments.len(), 1);
        assert_eq!(image.len(), 210);
    }

    #[test]
    fn sectors_cover_the_image() {
        let mut image = Image::from_bin(FLASH_BASE + 4000, vec![7; 200]).unwrap();
        image.add(FLASH_BASE + 3 * 4096, &[9; 4]).unwrap();
        let sectors = image.sectors();
        let addrs: Vec<u32> = sectors.iter().map(|s| s.addr - FLASH_BASE).collect();
        assert_eq!(addrs, [0, 4096, 3 * 4096]);
        assert_eq!(sectors[0].data[3999], 0xFF);
        assert_eq!(sectors[0].data[4000], 7);
        assert_eq!(sectors[1].data[103], 7);
        assert_eq!(sectors[1].data[104], 0xFF);
        assert_eq!(&sectors[2].data[..5], &[9, 9, 9, 9, 0xFF]);
    }
}
