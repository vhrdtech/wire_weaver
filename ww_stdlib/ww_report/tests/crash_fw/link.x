MEMORY
{
  FLASH : ORIGIN = 0x08000000, LENGTH = 64K
  RAM : ORIGIN = 0x20000000, LENGTH = 8K
}
ENTRY(Reset);
SECTIONS
{
  .text : ALIGN(4)
  {
    __stext = .;
    *(.text .text.*);
    __etext = .;
  } > FLASH
  .rodata : ALIGN(4) { *(.rodata .rodata.*); } > FLASH
  .bss (NOLOAD) : ALIGN(4) { *(.bss .bss.*); } > RAM
}
