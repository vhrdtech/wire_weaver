/* One block of RAM from 0 for code, data and stack, as on the QERV softcore of fpga_tools' busgen. The images
   are only measured, never run, so the size is generous: a case that doesn't fit 8K must still link. */
MEMORY { RAM : ORIGIN = 0, LENGTH = 256K }
ENTRY(_start)
SECTIONS {
  .text : { KEEP(*(.text.start)) *(.text .text.*) } > RAM
  .rodata : { *(.rodata .rodata.* .srodata .srodata.*) } > RAM
  .data : { *(.data .data.* .sdata .sdata.*) } > RAM
  .bss (NOLOAD) : ALIGN(4) { *(.bss .bss.* .sbss .sbss.* COMMON) } > RAM
  /DISCARD/ : { *(.eh_frame*) *(.comment) *(.ARM.exidx*) *(.ARM.attributes) *(.riscv.attributes) }
}
