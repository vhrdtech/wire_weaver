MEMORY
{
  /* FLASH and RAM are mandatory memory regions */

  /* SRAM (ram) */
  /*   SRAM : ORIGIN = 0x20000000, LENGTH = 144K */
  RAM : ORIGIN = 0x20000000, LENGTH = 144K

  /* FLASH (flash) */
  FLASH : ORIGIN = 0x8000000, LENGTH = 512K
}

/* Helper constants to access FLASH regions */
__flash_start = ORIGIN(FLASH);
__flash_end = ORIGIN(FLASH) + LENGTH(FLASH);

