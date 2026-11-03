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

/// The build id inside an image's bytes, as a flasher holds them before writing (a raw binary, the flash contents of
/// an ELF or a UF2): the GNU build note found by its header (`namesz` 4, `type` 3, `GNU\0`). `None` when there is no
/// such note, or more than one (an image built from two notes can't be told apart). Its first 8 bytes are the
/// `ww_board_info::ImageId` the rework guard binds an allowance to (STD-101).
pub fn find_build_id(image: &[u8]) -> Option<&[u8]> {
    let mut found = None;
    for at in 0..image.len().saturating_sub(16) {
        let head = &image[at..at + 16];
        if head[0..4] == [4, 0, 0, 0]
            && head[8..12] == [3, 0, 0, 0]
            && head[12..16] == *b"GNU\0"
            && let Some(id) = parse_build_id_note(&image[at..])
        {
            if found.is_some() {
                return None;
            }
            found = Some(id);
        }
    }
    found
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
    fn found_in_an_image() {
        let mut note = vec![4, 0, 0, 0, 20, 0, 0, 0, 3, 0, 0, 0];
        note.extend_from_slice(b"GNU\0");
        note.extend(0..20u8);
        let mut image = vec![0xff; 100];
        image.extend(&note);
        image.extend([0xff; 50]);
        assert_eq!(
            find_build_id(&image),
            Some(&(0..20u8).collect::<Vec<_>>()[..])
        );
        assert_eq!(find_build_id(&image[..100]), None);
        // two notes: can't tell which one is the image's
        image.extend(&note);
        assert_eq!(find_build_id(&image), None);
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
