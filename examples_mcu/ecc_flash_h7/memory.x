MEMORY
{
  /* STM32H725IG, 1 MiB in 8 sectors of 128 KiB: the last sector (0x080E0000) is the scratch sector of the test */
  FLASH : ORIGIN = 0x08000000, LENGTH = 896K
  RAM : ORIGIN = 0x24000000, LENGTH = 320K
}
