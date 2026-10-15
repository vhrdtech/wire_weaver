//! Function symbols of a firmware ELF, to put names to the addresses a crash record holds.

use anyhow::{Context, Result};
use object::{Object, ObjectSymbol, SymbolKind};
use serde::Serialize;
use std::path::Path;

/// Functions of an ELF by address.
pub struct Symbols {
    /// (start, size, name), sorted by start
    functions: Vec<(u64, u64, String)>,
}

/// An address and the function it falls into.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Located {
    pub address: u32,
    /// Demangled, `None` when no function of the ELF contains the address
    pub function: Option<String>,
    /// Offset into the function
    pub offset: u32,
}

impl Symbols {
    pub fn load(path: &Path) -> Result<Self> {
        let bytes =
            std::fs::read(path).with_context(|| format!("Failed to read {}", path.display()))?;
        Self::from_elf(&bytes).with_context(|| path.display().to_string())
    }

    pub fn from_elf(bytes: &[u8]) -> Result<Self> {
        let file = object::File::parse(bytes)?;
        let mut functions: Vec<_> = file
            .symbols()
            .filter(|s| s.kind() == SymbolKind::Text && s.size() > 0)
            .filter_map(|s| {
                let name = s.name().ok()?;
                // thumb functions carry the mode in bit 0 of the symbol value
                let start = s.address() & !1;
                Some((start, s.size(), rustc_demangle::demangle(name).to_string()))
            })
            .collect();
        functions.sort_by_key(|f| f.0);
        Ok(Self { functions })
    }

    /// The function containing `address`. Cortex-M return addresses have bit 0 set (thumb), it is ignored.
    pub fn locate(&self, address: u32) -> Located {
        let a = u64::from(address & !1);
        let i = self.functions.partition_point(|f| f.0 <= a);
        let found = i
            .checked_sub(1)
            .map(|i| &self.functions[i])
            .filter(|(start, size, _)| a < start + size);
        match found {
            Some((start, _, name)) => Located {
                address,
                function: Some(name.clone()),
                offset: (a - start) as u32,
            },
            None => Located {
                address,
                function: None,
                offset: 0,
            },
        }
    }

    pub fn is_empty(&self) -> bool {
        self.functions.is_empty()
    }
}
