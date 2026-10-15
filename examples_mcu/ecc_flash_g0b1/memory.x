MEMORY
{
  /* bank 1 only: the test store is in bank 2 (0x08070000..0x08072000), programmed and erased while code runs */
  FLASH : ORIGIN = 0x08000000, LENGTH = 256K
  RAM : ORIGIN = 0x20000000, LENGTH = 144K
}
