MEMORY
{
  /* FLASH and RAM are mandatory memory regions */

  /* ITCM (ram) */
  ITCM : ORIGIN = 0x0, LENGTH = 64K

  /* DTCM (ram) */
  DTCM : ORIGIN = 0x20000000, LENGTH = 128K

  /* AXISRAM (ram) */
  /* TODO: Check whether AXISRAM size can be reconfigured vs ITCM (TCM_AXI_SHARED in ref. manual) */
  /* Using AXISRAM as main RAM to avoid potential DMA issues (vs DTCM for example), other configurations might be viable as well */
  /*   AXISRAM : ORIGIN = 0x24000000, LENGTH = 512K */
  RAM : ORIGIN = 0x24000000, LENGTH = 512K

  /* SRAM1 (ram) */
  SRAM1 : ORIGIN = 0x30000000, LENGTH = 128K

  /* SRAM2 (ram) */
  SRAM2 : ORIGIN = 0x30020000, LENGTH = 128K

  /* SRAM3 (ram) */
  SRAM3 : ORIGIN = 0x30040000, LENGTH = 32K

  /* SRAM4 (ram) */
  SRAM4 : ORIGIN = 0x38000000, LENGTH = 64K

  /* FLASH (flash) */
  FLASH : ORIGIN = 0x8000000, LENGTH = 2048K
}

SECTIONS {
  .itcm (NOLOAD) : ALIGN(4) {
    *(.itcm .itcm.*);
    . = ALIGN(4);
  } > ITCM
  .dtcm (NOLOAD) : ALIGN(4) {
    *(.dtcm .dtcm.*);
    . = ALIGN(4);
  } > DTCM
  .sram1 (NOLOAD) : ALIGN(4) {
    *(.sram1 .sram1.*);
    . = ALIGN(4);
  } > SRAM1
  .sram2 (NOLOAD) : ALIGN(4) {
    *(.sram2 .sram2.*);
    . = ALIGN(4);
  } > SRAM2
  .sram3 (NOLOAD) : ALIGN(4) {
    *(.sram3 .sram3.*);
    . = ALIGN(4);
  } > SRAM3
  .sram4 (NOLOAD) : ALIGN(4) {
    *(.sram4 .sram4.*);
    . = ALIGN(4);
  } > SRAM4
};

/* Helper constants to init additional memory banks to zero (for example if using with StaticCell) or access FLASH regions */
/* TODO: Check whether RAM banks need to be explicitly enabled before use */
__itcm_start = ORIGIN(ITCM);
__itcm_end = ORIGIN(ITCM) + LENGTH(ITCM);

__dtcm_start = ORIGIN(DTCM);
__dtcm_end = ORIGIN(DTCM) + LENGTH(DTCM);

__sram1_start = ORIGIN(SRAM1);
__sram1_end = ORIGIN(SRAM1) + LENGTH(SRAM1);

__sram2_start = ORIGIN(SRAM2);
__sram2_end = ORIGIN(SRAM2) + LENGTH(SRAM2);

__sram3_start = ORIGIN(SRAM3);
__sram3_end = ORIGIN(SRAM3) + LENGTH(SRAM3);

__sram4_start = ORIGIN(SRAM4);
__sram4_end = ORIGIN(SRAM4) + LENGTH(SRAM4);

__flash_start = ORIGIN(FLASH);
__flash_end = ORIGIN(FLASH) + LENGTH(FLASH);

