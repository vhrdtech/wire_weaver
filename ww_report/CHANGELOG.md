# Changelog

## Unreleased

### ⚠️ Breaking

- `report::Report::reset_symbols: Part<Vec<Located>>` is replaced by `crash: Part<CrashFrames>` (CLT-59), also in
  `ww report --format json`: read the frames from `crash.sites[].frames[]` (`function`, `file`, `short_file`,
  `line`, `column`, `inlined`, `kind`), the panic site from `crash.panic_site`.
- `symbols::Symbols` names functions without the crate disambiguator and hash (`crash_fw::ratio`).

### 🚀 Features

- `crash::Site::stale`: stack words past the entry point (`main`, `Reset`, a task's executor `poll`) are marked, and
  `CrashFrames::render` lists them after a "possibly stale" line (CLT-59).
- `rework_check` gains `scan_installed_image` (the image in flash whether it has a table or not, for `ww info` on
  an old image), `board_guard` (`ww_board_info::guard::decide` run on what `read_board` answered: the verdict a
  reboot would reach right now), `guard_line` and `rom_info_text` (TOOL-30): what `ww info` shows for a board
  already in its boot ROM.
- `crash` (CLT-59): the crash record's call chain as `file:line` frames from the ELF's DWARF, for firmware built with
  `-Zlocation-detail=none`. `Symbolizer` (`load`, `from_elf`, `symbolize`, `frames`, `call_site`), `CrashAddresses`
  (`from_record` for a `ResetRecordOwned`, `from_last_reset`), `CrashFrames` (`sites`, `panic_site`, `panic_frame`,
  `summary`, `render`, `render_lines`, `dropped`, `no_debug_info`), `Site`, `Frame`, `FrameKind`, `Source`,
  `short_path`, `short_function` (the text leaves generic arguments out, the JSON keeps them). A fault's `pc` is looked
  up exactly, `lr` and stack words at the call before them; stack words that are no return address are dropped (the
  `bedrock_crash::is_return_address` rule, read from the ELF's code); inlined frames come innermost first; the panic
  machinery is marked so the panic site stands out. `ww report` shows the frames and the panic site under "Last reset".
- `rework_check` (TOOL-29, STD-101): the check every flasher makes before writing an image, with the chip in its ROM bootloader. `RomFlash` (read, and replay `FlashOp` erases and writes) is what a flasher gives it; `ImageReworks::from_flash` (the image's table, locator and `ImageId` from its flash contents), `scan_installed` (the table of the image in flash, read through the ROM), `board_name`, `read_board` (`BoardReworks`: name, revision, applied reworks and the stored allowance, from the bench record through the ROM), `verdict` (`Verdict`: `Ok`, `Lacking`, `OtherBoard`, `NoRecord`, `BadRecord`, with `Display` texts), `set_allowance` (store or clear it through the ROM, read back), `allowance_line`. Used by `ww dfu rp flash` and `ww dfu stm32 flash`.
- `ww info` / `ww report` print the board's reworks (`ww_board_info::BoardInfo::REWORKS`, STD-24) when the device
  serves them: `info::Board::reworks: Part<Vec<Rework>>`. `Part::from_error` maps `NotImplementedByDevice` to
  `NotImplemented`. `info::Rework` follows `ww_board_info` 0.3.0 (STD-98): `id: u16`, `title: Option<String>`; a
  rework the firmware doesn't know prints as `rework 9 (unknown to this firmware): applied, allowed for this image`
  (a running firmware with one runs because an allowance for its image covers it, STD-101).
- New crate: `collect_info` (`ww info`: firmware, board, ids, label, last reset of any device by global trait path),
  `collect_report` and `render_report` (`ww report`: info, counters, the stored `defmt` log and the crash's addresses
  decoded with the ELF found by build id), `Contract` (clients of every trait on one connection), `find_trait`
  (absolute path of a trait in a device's introspection data, which the log stream needs) (CLT-25). The last reset shows `boots_since_crash` (STD-27).
- `find_all_traits`: every place a trait is implemented at, with the path's names (`diag.pins`), for `ww pins` (TOOL-15).

### 🐛 Fixes

- `crash`: the panic site or fault place is the first frame that is not `core` / `alloc` / `std`: a HardFault in an inlined `core::ptr::read_volatile` names its caller, the full chain stays (CLT-59).
- `crash` (CLT-59): the "<- panic site" marker no longer lands on a stale return address left among the machinery's words by the panic handler's own frames; the site is the first code past the last `core::panicking` / `unwrap_failed` entry, the caller of the panic machinery (`CrashFrames::panic_site`).
- Drop the unused `serde_json` dependency (P2401, cargo-machete).
