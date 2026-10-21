//! The memory layout string of a DfuSe alternate setting (AN3156 §"DFU interface string descriptor"):
//! `@Internal Flash  /0x08000000/16*128Kg`, several runs of sectors separated by commas, several areas by `/`.

use crate::Error;

/// One run of equal sectors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Region {
    pub start: u32,
    pub sector_size: u32,
    pub sectors: u32,
    pub readable: bool,
    pub erasable: bool,
    pub writable: bool,
}

impl Region {
    /// First address after the region.
    pub fn end(&self) -> u64 {
        self.start as u64 + self.sector_size as u64 * self.sectors as u64
    }

    pub fn contains(&self, addr: u32) -> bool {
        addr >= self.start && (addr as u64) < self.end()
    }
}

/// What the alt setting's string descriptor says about the memory behind it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryLayout {
    /// `Internal Flash`, `Option Bytes`, ...
    pub name: String,
    pub regions: Vec<Region>,
}

impl MemoryLayout {
    /// Parse `@name /0x08000000/04*016Kg,01*064Kg,07*128Kg`.
    pub fn parse(s: &str) -> Result<Self, Error> {
        let bad = |why: &str| Error::Layout(format!("{s:?}: {why}"));
        let s = s.trim().trim_end_matches('\0');
        let s = s.strip_prefix('@').ok_or_else(|| bad("no leading '@'"))?;
        let mut parts = s.split('/');
        let name = parts.next().unwrap_or_default().trim().to_string();
        let mut regions = vec![];
        while let Some(addr) = parts.next() {
            let addr = addr.trim();
            let start = addr
                .strip_prefix("0x")
                .or_else(|| addr.strip_prefix("0X"))
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .ok_or_else(|| bad("start address is not 0x<hex>"))?;
            let runs = parts
                .next()
                .ok_or_else(|| bad("no sectors after the address"))?;
            let mut at = start;
            for run in runs.split(',') {
                let run = run.trim();
                let (count, rest) = run
                    .split_once('*')
                    .ok_or_else(|| bad("no '*' in a sector run"))?;
                let sectors: u32 = count.trim().parse().map_err(|_| bad("bad sector count"))?;
                // <size><unit: B, K or M><type letter>
                let rest = rest.trim();
                let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
                let size: u32 = rest[..digits].parse().map_err(|_| bad("bad sector size"))?;
                // the unit letter is optional, a space (or nothing) between size and type means bytes
                // (`@Option Bytes /0x5200201C/01*88 e`)
                let tail = rest[digits..].trim_end();
                let (unit, kind) = match tail.chars().collect::<Vec<_>>()[..] {
                    [u, k] => (u, k),
                    [k] => (' ', k),
                    _ => return Err(bad("expected <size>[B|K|M]<a-g>")),
                };
                let mult = match unit.to_ascii_uppercase() {
                    'B' | ' ' => 1,
                    'K' => 1024,
                    'M' => 1024 * 1024,
                    _ => return Err(bad("unit is not B, K or M")),
                };
                let (readable, erasable, writable) = match kind.to_ascii_lowercase() {
                    'a' => (true, false, false),
                    'b' => (false, true, false),
                    'c' => (true, true, false),
                    'd' => (false, false, true),
                    'e' => (true, false, true),
                    'f' => (false, true, true),
                    'g' => (true, true, true),
                    _ => return Err(bad("type is not a..g")),
                };
                let sector_size = size
                    .checked_mul(mult)
                    .filter(|s| *s > 0)
                    .ok_or_else(|| bad("bad sector size"))?;
                let region = Region {
                    start: at,
                    sector_size,
                    sectors,
                    readable,
                    erasable,
                    writable,
                };
                if region.end() > u32::MAX as u64 + 1 {
                    return Err(bad("region reaches past 4 GiB"));
                }
                at = region.end() as u32;
                regions.push(region);
            }
        }
        if regions.is_empty() {
            return Err(bad("no sectors"));
        }
        Ok(MemoryLayout { name, regions })
    }

    pub fn region_at(&self, addr: u32) -> Option<&Region> {
        self.regions.iter().find(|r| r.contains(addr))
    }

    /// The sector containing `addr`: its start and size, and the region it is in.
    pub fn sector_at(&self, addr: u32) -> Option<(u32, u32, &Region)> {
        let r = self.region_at(addr)?;
        let base = r.start + (addr - r.start) / r.sector_size * r.sector_size;
        Some((base, r.sector_size, r))
    }

    /// Whether the whole range lies in memory that is writable.
    pub fn writable(&self, addr: u32, len: usize) -> bool {
        self.all(addr, len, |r| r.writable)
    }

    pub fn readable(&self, addr: u32, len: usize) -> bool {
        self.all(addr, len, |r| r.readable)
    }

    fn all(&self, addr: u32, len: usize, ok: impl Fn(&Region) -> bool) -> bool {
        let end = addr as u64 + len as u64;
        let mut at = addr as u64;
        while at < end {
            match self.region_at(at as u32) {
                Some(r) if ok(r) => at = r.end(),
                _ => return false,
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h7_flash() {
        let l = MemoryLayout::parse("@Internal Flash  /0x08000000/08*128Kg").unwrap();
        assert_eq!(l.name, "Internal Flash");
        assert_eq!(l.regions.len(), 1);
        let r = &l.regions[0];
        assert_eq!(
            (r.start, r.sector_size, r.sectors),
            (0x0800_0000, 128 * 1024, 8)
        );
        assert!(r.readable && r.erasable && r.writable);
        assert_eq!(l.sector_at(0x0802_0001), Some((0x0802_0000, 128 * 1024, r)));
        assert!(l.writable(0x0800_0000, 8 * 128 * 1024));
        assert!(!l.writable(0x0800_0000, 8 * 128 * 1024 + 1));
    }

    #[test]
    fn f4_mixed_sectors_and_areas() {
        let l = MemoryLayout::parse(
            "@Internal Flash  /0x08000000/04*016Kg,01*064Kg,07*128Kg/0x1FFF0000/01*016Ka",
        )
        .unwrap();
        assert_eq!(l.regions.len(), 4);
        assert_eq!(l.regions[1].start, 0x0801_0000);
        assert_eq!(l.regions[2].start, 0x0802_0000);
        assert_eq!(l.regions[3].start, 0x1FFF_0000);
        assert!(l.readable(0x1FFF_0000, 16));
        assert!(!l.writable(0x1FFF_0000, 16));
    }

    #[test]
    fn size_without_unit_is_bytes() {
        let l = MemoryLayout::parse("@Option Bytes   /0x5200201C/01*88 e").unwrap();
        let r = &l.regions[0];
        assert_eq!((r.start, r.sector_size, r.sectors), (0x5200_201C, 88, 1));
        assert!(r.readable && !r.erasable && r.writable);
        let l = MemoryLayout::parse("@x/0x0/02*16e").unwrap();
        assert_eq!(l.regions[0].sector_size, 16);
    }

    #[test]
    fn bad_strings() {
        for s in [
            "Internal Flash/0x08000000/8*128Kg",
            "@x/08000000/8*128Kg",
            "@x/0x0/8*128Kz",
            "@x/0x0/8*Kg",
            "@x",
        ] {
            assert!(MemoryLayout::parse(s).is_err(), "{s}");
        }
    }
}
