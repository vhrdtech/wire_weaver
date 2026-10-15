# Handoff: ecc-safe-flash (SRV-19, TEST-21)

## Done (commit 5f8db196)
- `mcu/ww_ecc_flash`: EccSafeFlash, G0 (`on_nmi`, `nmi-handler`) and H7 (FAULTMASK + BFHFNMIGN). check-mcu + CI
  check it with family features on thumb targets (plain workspace check excludes it: compile_error without a family).
- `examples_mcu/ecc_flash_g0b1`: store reset loop (3000 boots PASS on B153A via J-Link 1366:0105) and `reset_cut`.
- Finding: G0 system reset waits for the flash op (73 erases cut 1-20 ms all ended fully erased, programs stop
  between double words, 0 ECC errors). ECC path needs a real power cut.
- Docs `docs/api/ecc_flash.md` (not built: ask before `just build-docs`), FEATURES SRV-19 🧪, TEST-21 🚧.

## Next
- Real power cut: B129A (G0B1) powered via Donguru plug VBUS (Q101/Q102). Asked donguru-api how to switch it from
  the PC (fw API or SWD pin poke). If nothing works: needs Roman (say it in words, tpm ask doesn't exist yet).
  Then: flash ecc_flash_g0b1 onto the B129A (via Donguru CMSIS-DAP), main.rs without SysTick reset, PC cuts power at
  random times, expect `words read as zeros (ECC)` > 0 and the store still loading.
- Board etiquette: B153A is shared (api-tree, device-contract). Ask before every flash, say when done.

## Gotchas
- probe-rs run needs `--no-catch-reset` (in .cargo/config.toml) or it stops at the first reset.
- Fast reset loops garble RTT; reset_cut waits 50 ms per boot. Counters also sit in `.uninit` RAM (STATS/STATE).
