/* mps2-an386 (Cortex-M4F): 4 MB flash (ZBT SSRAM1, QEMU aliases it at 0x0 too) + 4 MB RAM (ZBT SSRAM2/3) */
MEMORY
{
  FLASH : ORIGIN = 0x00400000, LENGTH = 4M
  RAM : ORIGIN = 0x20000000, LENGTH = 4M
}
