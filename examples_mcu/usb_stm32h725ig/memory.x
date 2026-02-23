MEMORY
{
    /* STM32H725IGK - 8 sectors of 128K each + ECC */
    FLASH    : ORIGIN = 0x08000000, LENGTH = 1024K
    /* FLASH1 : ORIGIN = 0x08100000, LENGTH = 512K */

    /* DTCM  */
    DTCM    : ORIGIN = 0x20000000, LENGTH = 128K

    /* AXISRAM */
    /* Can be reconfigured to be 128, 192, 256 or 320KB vs ITCM */
    /* See TCM_AXI_SHARED option bits in ref. manual */
    RAM : ORIGIN = 0x24000000, LENGTH = 320K

    /* SRAM D2 Domain, disabled after reset and must be enabled via RCC_AHB2ENR */
    SRAM1 : ORIGIN = 0x30000000, LENGTH = 16K
    SRAM2 : ORIGIN = 0x30004000, LENGTH = 16K

    /* SRAM D3 Domain */
    SRAM4 : ORIGIN = 0x38000000, LENGTH = 16K

    /* Backup SRAM */
    BSRAM : ORIGIN = 0x38800000, LENGTH = 4K

    /* Instruction TCM */
    /* Can be reconfigured to be 64, 128, 192 or 256KB vs AXISRAM */
    ITCM  : ORIGIN = 0x00000000, LENGTH = 64K
}

SECTIONS {
    /*.axisram (NOLOAD) : ALIGN(8) {
        *(.axisram .axisram.*);
        . = ALIGN(8);
    } > AXISRAM */
    .sram1 (NOLOAD) : ALIGN(4) {
        *(.sram1 .sram1.*);
        . = ALIGN(4);
    } > SRAM1
    .sram2 (NOLOAD) : ALIGN(4) {
        *(.sram2 .sram2.*);
        . = ALIGN(4);
    } > SRAM2
    .sram4 (NOLOAD) : ALIGN(4) {
        *(.sram4 .sram4.*);
        . = ALIGN(4);
    } > SRAM4
    .bsram (NOLOAD) : ALIGN(4) {
        *(.bsram .bsram.*);
        . = ALIGN(4);
    } > BSRAM
};
