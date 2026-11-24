//! The little of the wasm binary format the host reads itself, before an interpreter sees the module: its sections,
//! the model's custom section and the `memory` export.

use crate::{MODEL_SECTION, export};

const MAGIC: &[u8; 4] = b"\0asm";
const VERSION: &[u8; 4] = &[1, 0, 0, 0];
const SECTION_CUSTOM: u8 = 0;
const SECTION_EXPORT: u8 = 7;
const EXPORT_KIND_MEMORY: u8 = 2;

/// Why the host refused a wasm binary before handing it to an interpreter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum BinaryError {
    /// No wasm magic and version 1 at the start.
    Header,
    /// A section or a name runs past the binary.
    Truncated,
    /// No `ww_model` custom section, or more than one.
    Model,
    /// No `memory` export of a memory.
    Memory,
}

struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    fn byte(&mut self) -> Result<u8, BinaryError> {
        let (first, rest) = self.bytes.split_first().ok_or(BinaryError::Truncated)?;
        self.bytes = rest;
        Ok(*first)
    }

    /// An unsigned LEB128 of at most 32 bits.
    fn u32(&mut self) -> Result<u32, BinaryError> {
        let mut value = 0u32;
        for shift in (0..35).step_by(7) {
            let byte = self.byte()?;
            value |= u32::from(byte & 0x7f)
                .checked_shl(shift)
                .ok_or(BinaryError::Truncated)?;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(BinaryError::Truncated)
    }

    fn take(&mut self, len: u32) -> Result<&'a [u8], BinaryError> {
        let len = usize::try_from(len).map_err(|_| BinaryError::Truncated)?;
        let (taken, rest) = self
            .bytes
            .split_at_checked(len)
            .ok_or(BinaryError::Truncated)?;
        self.bytes = rest;
        Ok(taken)
    }

    fn name(&mut self) -> Result<&'a [u8], BinaryError> {
        let len = self.u32()?;
        self.take(len)
    }
}

/// The sections of `wasm` as (id, contents).
fn sections(
    wasm: &[u8],
) -> Result<impl Iterator<Item = Result<(u8, &[u8]), BinaryError>>, BinaryError> {
    let rest = wasm
        .strip_prefix(MAGIC)
        .and_then(|rest| rest.strip_prefix(VERSION))
        .ok_or(BinaryError::Header)?;
    let mut reader = Reader { bytes: rest };
    Ok(core::iter::from_fn(move || {
        if reader.bytes.is_empty() {
            return None;
        }
        let section = (|| {
            let id = reader.byte()?;
            let len = reader.u32()?;
            Ok((id, reader.take(len)?))
        })();
        if section.is_err() {
            reader.bytes = &[];
        }
        Some(section)
    }))
}

/// The model of a module: its one `ww_model` custom section, borrowed from `wasm` (in flash, no copy).
///
/// # Errors
///
/// [BinaryError] when `wasm` doesn't parse or has no model, or two.
pub fn model_section(wasm: &[u8]) -> Result<&[u8], BinaryError> {
    let mut model = None;
    for section in sections(wasm)? {
        let (id, contents) = section?;
        if id != SECTION_CUSTOM {
            continue;
        }
        let mut reader = Reader { bytes: contents };
        if reader.name()? == MODEL_SECTION.as_bytes() {
            if model.is_some() {
                return Err(BinaryError::Model);
            }
            model = Some(reader.bytes);
        }
    }
    model.ok_or(BinaryError::Model)
}

/// Whether `wasm` exports a memory named `memory`: an interpreter may assume the module has one once it is
/// instantiated.
///
/// # Errors
///
/// [BinaryError] when `wasm` doesn't parse, [BinaryError::Memory] when there is no such export.
pub fn has_memory_export(wasm: &[u8]) -> Result<(), BinaryError> {
    for section in sections(wasm)? {
        let (id, contents) = section?;
        if id != SECTION_EXPORT {
            continue;
        }
        let mut reader = Reader { bytes: contents };
        for _ in 0..reader.u32()? {
            let name = reader.name()?;
            let kind = reader.byte()?;
            let _index = reader.u32()?;
            if name == export::MEMORY.as_bytes() && kind == EXPORT_KIND_MEMORY {
                return Ok(());
            }
        }
    }
    Err(BinaryError::Memory)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A module with only the given sections.
    fn module(sections: &[(u8, &[u8])]) -> Vec<u8> {
        let mut wasm = b"\0asm\x01\0\0\0".to_vec();
        for (id, contents) in sections {
            wasm.push(*id);
            wasm.push(u8::try_from(contents.len()).unwrap());
            wasm.extend_from_slice(contents);
        }
        wasm
    }

    fn custom(name: &str, data: &[u8]) -> Vec<u8> {
        let mut contents = vec![u8::try_from(name.len()).unwrap()];
        contents.extend_from_slice(name.as_bytes());
        contents.extend_from_slice(data);
        contents
    }

    #[test]
    fn finds_the_model() {
        let other = custom("name", b"xyz");
        let model = custom(MODEL_SECTION, b"model bytes");
        let wasm = module(&[(0, &other), (0, &model)]);
        assert_eq!(model_section(&wasm), Ok(&b"model bytes"[..]));
    }

    #[test]
    fn refuses_no_model_two_models_and_bad_binaries() {
        let model = custom(MODEL_SECTION, b"m");
        assert_eq!(model_section(&module(&[])), Err(BinaryError::Model));
        assert_eq!(
            model_section(&module(&[(0, &model), (0, &model)])),
            Err(BinaryError::Model)
        );
        assert_eq!(model_section(b"\0asm\x02\0\0\0"), Err(BinaryError::Header));
        let mut truncated = module(&[(0, &model)]);
        truncated.pop();
        assert_eq!(model_section(&truncated), Err(BinaryError::Truncated));
        // a LEB128 longer than 32 bits
        assert_eq!(
            model_section(b"\0asm\x01\0\0\0\0\xff\xff\xff\xff\xff\x01"),
            Err(BinaryError::Truncated)
        );
    }

    #[test]
    fn finds_the_memory_export() {
        // one export: "memory", kind memory, index 0
        let exports = [1, 6, b'm', b'e', b'm', b'o', b'r', b'y', 2, 0];
        assert_eq!(has_memory_export(&module(&[(7, &exports)])), Ok(()));
        // the same name for a function
        let function = [1, 6, b'm', b'e', b'm', b'o', b'r', b'y', 0, 0];
        assert_eq!(
            has_memory_export(&module(&[(7, &function)])),
            Err(BinaryError::Memory)
        );
        assert_eq!(has_memory_export(&module(&[])), Err(BinaryError::Memory));
    }
}
