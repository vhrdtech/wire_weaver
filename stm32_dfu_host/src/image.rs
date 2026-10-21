//! Firmware images to write: ELF files and raw binaries, as segments of addresses and bytes.

use crate::Error;

/// A contiguous run of bytes at an address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment {
    pub addr: u32,
    pub data: Vec<u8>,
}

/// A firmware image: non-overlapping segments, sorted by address.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Image {
    pub segments: Vec<Segment>,
}

impl Image {
    /// A raw binary written at `addr` (usually `0x08000000`, the start of flash).
    pub fn from_bin(addr: u32, data: Vec<u8>) -> Result<Self, Error> {
        let mut image = Image::default();
        image.add(addr, &data)?;
        if image.is_empty() {
            return Err(Error::Image("the binary is empty".into()));
        }
        Ok(image)
    }

    /// The `PT_LOAD` segments of an ELF file, by physical address (`p_paddr`, the load address a segment is placed
    /// at — as opposed to `p_vaddr`, the address it runs from, for a segment copied to RAM at startup). Segments with
    /// no file bytes (`NOLOAD`, pure `.bss`) are skipped. Where the bytes may land is checked against the device's
    /// memory layout when writing.
    #[cfg(feature = "elf")]
    pub fn from_elf(bytes: &[u8]) -> Result<Self, Error> {
        use object::elf::PT_LOAD;
        use object::read::elf::{ElfFile32, ProgramHeader};
        use object::{Endianness, FileKind};

        let kind = FileKind::parse(bytes).map_err(|e| Error::Image(format!("ELF: {e}")))?;
        if kind != FileKind::Elf32 {
            return Err(Error::Image(format!(
                "ELF: {kind:?} isn't supported, STM32 firmware is always 32-bit"
            )));
        }
        let elf =
            ElfFile32::<Endianness>::parse(bytes).map_err(|e| Error::Image(format!("ELF: {e}")))?;
        let endian = elf.endian();
        let mut image = Image::default();
        for seg in elf.elf_program_headers() {
            if seg.p_type(endian) != PT_LOAD || seg.p_filesz(endian) == 0 {
                continue;
            }
            let paddr = seg.p_paddr(endian);
            let data = seg.data(endian, bytes).map_err(|()| {
                Error::Image(format!(
                    "ELF: the load segment at {paddr:#010x} reaches past the end of the file"
                ))
            })?;
            image.add(paddr, data)?;
        }
        if image.is_empty() {
            return Err(Error::Image(
                "ELF: no PT_LOAD segment with file bytes".into(),
            ));
        }
        Ok(image)
    }

    /// Add bytes at `addr`, merged with an adjacent segment; overlapping an existing one is an error.
    pub fn add(&mut self, addr: u32, data: &[u8]) -> Result<(), Error> {
        if data.is_empty() {
            return Ok(());
        }
        let end = addr as u64 + data.len() as u64;
        if end > u32::MAX as u64 + 1 {
            return Err(Error::Image(format!(
                "{} bytes at {addr:#010x} reach past 4 GiB",
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

    /// The same image with every segment grown to whole `align`-byte units (a flash word: 32 bytes on the H7) by
    /// filling with `0xFF`, the erased value, so a partial word is programmed whole. Segments that then touch are
    /// merged.
    pub fn aligned(&self, align: u32) -> Result<Image, Error> {
        assert!(align.is_power_of_two());
        let mut out = Image::default();
        for s in &self.segments {
            let start = s.addr & !(align - 1);
            let end = (s.addr as u64 + s.data.len() as u64).next_multiple_of(align as u64);
            let mut data = vec![0xFF; (end - start as u64) as usize];
            let off = (s.addr - start) as usize;
            data[off..off + s.data.len()].copy_from_slice(&s.data);
            // an earlier segment may already own part of the first word: its bytes win over the fill
            if let Some(prev) = out.segments.last_mut() {
                let prev_end = prev.addr as u64 + prev.data.len() as u64;
                if prev_end > start as u64 {
                    let overlap = (prev_end - start as u64) as usize;
                    let keep = &prev.data[prev.data.len() - overlap..];
                    for (i, b) in keep.iter().enumerate() {
                        if i < off {
                            data[i] = *b;
                        }
                    }
                    let cut = prev.data.len() - overlap;
                    prev.data.truncate(cut);
                    if prev.data.is_empty() {
                        out.segments.pop();
                    }
                }
            }
            out.add(start, &data)?;
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adjacent_segments_merge() {
        let mut i = Image::default();
        i.add(0x0800_0100, &[2; 16]).unwrap();
        i.add(0x0800_0000, &[1; 256]).unwrap();
        assert_eq!(i.segments.len(), 1);
        assert_eq!(i.len(), 272);
        assert!(i.add(0x0800_0010, &[0]).is_err());
    }

    #[test]
    fn aligned_fills_with_erased_value_and_merges() {
        let mut i = Image::default();
        i.add(0x0800_0004, &[1, 2, 3]).unwrap();
        i.add(0x0800_0030, &[9]).unwrap();
        let a = i.aligned(32).unwrap();
        // [0, 32) and [32, 64) touch: one segment
        assert_eq!(a.segments.len(), 1);
        assert_eq!(a.segments[0].addr, 0x0800_0000);
        assert_eq!(a.segments[0].data.len(), 64);
        assert_eq!(
            &a.segments[0].data[..8],
            &[0xFF, 0xFF, 0xFF, 0xFF, 1, 2, 3, 0xFF]
        );
        assert_eq!(a.segments[0].data[48], 9);
        // two segments in one word share it
        let mut i = Image::default();
        i.add(0x0800_0000, &[1]).unwrap();
        i.add(0x0800_0008, &[2]).unwrap();
        let a = i.aligned(32).unwrap();
        assert_eq!(a.segments.len(), 1);
        assert_eq!(
            (
                a.segments[0].data[0],
                a.segments[0].data[8],
                a.segments[0].data[1]
            ),
            (1, 2, 0xFF)
        );
    }
}
