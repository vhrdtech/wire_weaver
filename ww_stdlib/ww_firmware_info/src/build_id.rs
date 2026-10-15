//! Reading the GNU build id note of the running image.

/// The build id inside the contents of a `.note.gnu.build-id` section (`namesz`, `descsz`, `type`, name, desc;
/// little-endian words), `None` when the note is not a GNU build id or doesn't fit the slice.
pub fn parse_build_id_note(note: &[u8]) -> Option<&[u8]> {
    let word = |at: usize| -> Option<usize> {
        Some(u32::from_le_bytes(note.get(at..at + 4)?.try_into().ok()?) as usize)
    };
    let (namesz, descsz, kind) = (word(0)?, word(4)?, word(8)?);
    const NT_GNU_BUILD_ID: usize = 3;
    if kind != NT_GNU_BUILD_ID {
        return None;
    }
    let name_end = 12 + namesz.checked_add(3)? / 4 * 4;
    if note.get(12..12 + namesz)? != b"GNU\0" {
        return None;
    }
    note.get(name_end..name_end.checked_add(descsz)?)
}

/// Build id of the running image: the bytes of the GNU build note the linker wrote, empty when there is none.
///
/// Needs the `__build_id_start` / `__build_id_end` symbols from the firmware's linker script, see the
/// [crate] documentation.
#[cfg(all(feature = "build-id", not(feature = "std")))]
pub fn build_id() -> &'static [u8] {
    unsafe extern "C" {
        static __build_id_start: u8;
        static __build_id_end: u8;
    }
    // SAFETY: the linker script places the note between the two symbols, in flash for the program's lifetime
    let note = unsafe {
        let start = core::ptr::addr_of!(__build_id_start);
        let end = core::ptr::addr_of!(__build_id_end);
        core::slice::from_raw_parts(start, end as usize - start as usize)
    };
    parse_build_id_note(note).unwrap_or(&[])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_with_sha1() {
        let mut note = vec![4, 0, 0, 0, 20, 0, 0, 0, 3, 0, 0, 0];
        note.extend_from_slice(b"GNU\0");
        note.extend(0..20u8);
        assert_eq!(
            parse_build_id_note(&note),
            Some(&(0..20u8).collect::<Vec<_>>()[..])
        );
    }

    #[test]
    fn not_a_build_id() {
        let mut note = vec![4, 0, 0, 0, 2, 0, 0, 0, 1, 0, 0, 0];
        note.extend_from_slice(b"GNU\0");
        note.extend([0, 0]);
        assert_eq!(parse_build_id_note(&note), None);
        assert_eq!(parse_build_id_note(&[]), None);
        // truncated desc
        let mut note = vec![4, 0, 0, 0, 20, 0, 0, 0, 3, 0, 0, 0];
        note.extend_from_slice(b"GNU\0");
        note.extend([1, 2, 3]);
        assert_eq!(parse_build_id_note(&note), None);
    }
}
