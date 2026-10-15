MEMORY
{
  /* FLASH and RAM are mandatory memory regions */

  /* SRAM (ram) */
  /*   SRAM : ORIGIN = 0x20000000, LENGTH = 144K */
  RAM : ORIGIN = 0x20000000, LENGTH = 144K

  /* FLASH (flash): the last 4 pages (2K each) of bank 2 are reserved for the `settings` bin's ww_settings::Store,
     see src/bin/settings.rs; code never links there. */
  FLASH : ORIGIN = 0x8000000, LENGTH = 504K
}

__settings_start = ORIGIN(FLASH) + LENGTH(FLASH);
__settings_end = __settings_start + 8K;

/* Helper constants to access FLASH regions */
__flash_start = ORIGIN(FLASH);
__flash_end = ORIGIN(FLASH) + LENGTH(FLASH);

