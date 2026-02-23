# STM32G0B1CETxN examples

> This is an example project with several binaries for the STM32G0B1CETxN MCUs. This project uses `embassy` and
> `embassy-usb` so it can easily be adapted to other STM32G0 boards or different families.

## Pinout

* PA11/PA12 => USB
* PB14 => LED

## MCU Info

* Family: STM32G0
* Line: STM32G0x1
* Die: DIE467
* Device ID: 1127
* Packages:
    * STM32G0B1CETx: LQFP48 (48 pins)
    * STM32G0B1CETxN: LQFP48 (48 pins)
    * STM32G0B1CEUx: UFQFPN48 (48 pins)
    * STM32G0B1CEUxN: UFQFPN48 (48 pins)

## Documentation

* [STM32G0x1 advanced Arm-based 32-bit MCUs (reference_manual / DM00371828)](http://www.st.com/resource/en/reference_manual/DM00371828.pdf)
* [Cortex-M0+ programming manual for STM32L0, STM32G0, STM32WL and STM32WB Series (programming_manual / DM00104451)](http://www.st.com/resource/en/programming_manual/DM00104451.pdf)
* [Arm® Cortex®-M0+ 32-bit MCU, up to 512KB Flash, 144KB RAM, 6x USART, timers, ADC, DAC, comm. I/Fs, 1.7-3.6V (datahseet / DM00748675)](https://www.st.com/resource/en/datasheet/dm00748675.pdf)
* [STM32G0B1xB/xC/xE device errata (errata_sheet / DM00760234)](https://www.st.com/resource/en/errata_sheet/dm00760234-stm32g0b1xbxcxe-device-errata-stmicroelectronics.pdf)
* [Electrostatic discharge sensitivity measurement (application_note / CD00004125)](https://www.st.com/resource/en/application_note/cd00004125-electrostatic-discharge-sensitivity-measurement-stmicroelectronics.pdf)
* [EMC design guide for STM8, STM32 and Legacy MCUs (application_note / CD00004479)](https://www.st.com/resource/en/application_note/cd00004479-emc-design-guide-for-stm8-stm32-and-legacy-mcus-stmicroelectronics.pdf)
* [Using the STM32F0/F1/F3/Gx/Lx Series DMA controller (application_note / CD00160362)](http://www.st.com/resource/en/application_note/CD00160362.pdf)
* [STM32 microcontroller system memory boot mode (application_note / CD00167594)](http://www.st.com/resource/en/application_note/CD00167594.pdf)
* [Soldering recommendations and package information for Lead-free ECOPACK MCUs and MPUs (application_note / CD00173820)](https://www.st.com/resource/en/application_note/cd00173820-soldering-recommendations-and-package-information-for-leadfree-ecopack-mcus-and-mpus-stmicroelectronics.pdf)
* [How to get the best ADC accuracy in STM32 microcontrollers (application_note / CD00211314)](http://www.st.com/resource/en/application_note/CD00211314.pdf)
* [Audio and waveform generation using the DAC in STM32 products (application_note / CD00259245)](http://www.st.com/resource/en/application_note/CD00259245.pdf)
* [USART protocol used in the STM32 bootloader (application_note / CD00264342)](http://www.st.com/resource/en/application_note/CD00264342.pdf)
* [USB DFU protocol used in the STM32 bootloader (application_note / CD00264379)](http://www.st.com/resource/en/application_note/CD00264379.pdf)
* [STM32 cross-series timer overview (application_note / DM00042534)](http://www.st.com/resource/en/application_note/DM00042534.pdf)
* [I2C protocol used in the STM32 bootloader (application_note / DM00072315)](http://www.st.com/resource/en/application_note/DM00072315.pdf)
* [How to implement a vocoder solution using STM32 microcontrollers (application_note / DM00073742)](http://www.st.com/resource/en/application_note/DM00073742.pdf)
* [SPI protocol used in the STM32 bootloader (application_note / DM00081379)](http://www.st.com/resource/en/application_note/DM00081379.pdf)
* [STM32 SMBus/PMBus™ embedded software expansion for STM32Cube™ (application_note / DM00118362)](https://www.st.com/resource/en/application_note/dm00118362-stm32-smbuspmbus-embedded-software-expansion-for-stm32cube-stmicroelectronics.pdf)
* [Extending the DAC performance of STM32 microcontrollers (application_note / DM00129215)](http://www.st.com/resource/en/application_note/DM00129215.pdf)
* [Minimization of power consumption using LPUART for STM32 microcontrollers (application_note / DM00151811)](http://www.st.com/resource/en/application_note/DM00151811.pdf)
* [Virtually increasing the number of serial communication peripherals in STM32 applications (application_note / DM00160482)](http://www.st.com/resource/en/application_note/DM00160482.pdf)
* [STM32 in-application programming (IAP) using the USART (application_note / DM00161366)](https://www.st.com/resource/en/application_note/dm00161366-stm32-inapplication-programming-iap-using-the-usart-stmicroelectronics.pdf)
* [Handling of soft errors in STM32 applications (application_note / DM00220769)](http://www.st.com/resource/en/application_note/DM00220769.pdf)
* [Using the hardware real-time clock (RTC) and the tamper management unit (TAMP) with STM32 microcontrollers (application_note / DM00226326)](http://www.st.com/resource/en/application_note/DM00226326.pdf)
* [Using the hardware real-time clock (RTC) and the tamper management unit (TAMP) with STM32 microcontrollers (application_note / DM00226326)](http://www.st.com/resource/en/application_note/DM00226326.pdf)
* [General-purpose timer cookbook for STM32 microcontrollers (application_note / DM00236305)](http://www.st.com/resource/en/application_note/DM00236305.pdf)
* [High-speed SI simulations using IBIS and board-level simulations using HyperLynx SI on STM32 MCUs and MPUs (application_note / DM00257177)](http://www.st.com/resource/en/application_note/DM00257177.pdf)
* [Managing memory protection unit in STM32 MCUs (application_note / DM00272912)](http://www.st.com/resource/en/application_note/DM00272912.pdf)
* [Digital signal processing for STM32 microcontrollers using CMSIS (application_note / DM00273990)](https://www.st.com/resource/en/application_note/dm00273990-digital-signal-processing-for-stm32-microcontrollers-using-cmsis-stmicroelectronics.pdf)
* [Low-power timer (LPTIM) applicative use cases on STM32 microcontrollers (application_note / DM00290631)](https://www.st.com/resource/en/application_note/dm00290631-lowpower-timer-lptim-applicative-use-cases-on-stm32-microcontrollers-stmicroelectronics.pdf)
* [EEPROM emulation techniques and software for STM32 microcontrollers (application_note / DM00311483)](http://www.st.com/resource/en/application_note/DM00311483.pdf)
* [STM32 GPIO configuration for hardware settings and low-power consumption (application_note / DM00315319)](http://www.st.com/resource/en/application_note/DM00315319.pdf)
* [STM32 microcontroller debug toolbox (application_note / DM00354244)](http://www.st.com/resource/en/application_note/DM00354244.pdf)
* [How to wake up an STM32xx Series microcontroller from low-power mode with the USART or the LPUART (application_note / DM00355687)](http://www.st.com/resource/en/application_note/DM00355687.pdf)
* [Interfacing PDM digital microphones using STM32 MCUs and MPUs (application_note / DM00380469)](http://www.st.com/resource/en/application_note/DM00380469.pdf)
* [Thermal management guidelines for STM32 applications (application_note / DM00395696)](http://www.st.com/resource/en/application_note/DM00395696.pdf)
* [Secure programming using STM32CubeProgrammer (application_note / DM00413494)](https://www.st.com/resource/en/application_note/dm00413494-secure-programming-using-stm32cubeprogrammer-stmicroelectronics.pdf)
* [Integration guide for the X-CUBE-SBSFU STM32Cube Expansion Package (application_note / DM00414677)](https://www.st.com/resource/en/application_note/dm00414677-integration-guide-for-the-xcubesbsfu-stm32cube-expansion-package-stmicroelectronics.pdf)
* [Getting started with STM32G0 Series hardware development (application_note / DM00443870)](http://www.st.com/resource/en/application_note/DM00443870.pdf)
* [STM32Cube firmware examples for STM32G0 Series (application_note / DM00449912)](http://www.st.com/resource/en/application_note/DM00449912.pdf)
* [STM32Cube firmware examples for STM32G0 Series (application_note / DM00449912)](http://www.st.com/resource/en/application_note/DM00449912.pdf)
* [Migration of applications from STM32F0 Series to STM32G0 Series (application_note / DM00483659)](http://www.st.com/resource/en/application_note/DM00483659.pdf)
* [Introduction to STM32 microcontrollers security (application_note / DM00493651)](http://www.st.com/resource/en/application_note/DM00493651.pdf)
* [STM32 DMAMUX: the DMA request router (application_note / DM00535045)](http://www.st.com/resource/en/application_note/DM00535045.pdf)
* [USB Type-C Power Delivery using STM32 MCUs and MPUs (application_note / DM00536349)](http://www.st.com/resource/en/application_note/DM00536349.pdf)
* [FDCAN peripheral on STM32 devices (application_note / DM00625700)](http://www.st.com/resource/en/application_note/DM00625700.pdf)
* [Getting started with projects based on the STM32MP1 Series in STM32CubeIDE (application_note / DM00629854)](https://www.st.com/resource/en/application_note/dm00629854-getting-started-with-projects-based-on-the-stm32mp1-series-in-stm32cubeide-stmicroelectronics.pdf)
* [Getting started with projects based on dual-core STM32H7 microcontrollers in STM32CubeIDE (application_note / DM00629855)](https://www.st.com/resource/en/application_note/dm00629855-getting-started-with-projects-based-on-dualcore-stm32h7-microcontrollers-in-stm32cubeide-stmicroelectronics.pdf)
* [Getting started with projects based on the STM32L5 Series in STM32CubeIDE (application_note / DM00652038)](https://www.st.com/resource/en/application_note/dm00652038-getting-started-with-projects-based-on-the-stm32l5-series-in-stm32cubeide-stmicroelectronics.pdf)
* [How to build a simple USB-PD sink application with STM32CubeMX (application_note / DM00663511)](https://www.st.com/resource/en/application_note/dm00663511-how-to-build-a-simple-usbpd-sink-application-with-stm32cubemx-stmicroelectronics.pdf)
* [Migrating graphics middleware projects from STM32CubeMX 5.4.0 to STM32CubeMX 5.5.0 (application_note / DM00670808)](https://www.st.com/resource/en/application_note/dm00670808-migrating-graphics-middleware-projects-from-stm32cubemx-540-to-stm32cubemx-550-stmicroelectronics.pdf)
* [Enhanced methods to handle SPI communication on STM32 devices (application_note / DM00725181)](http://www.st.com/resource/en/application_note/DM00725181.pdf)
* [Getting started with projects based on dual-core STM32WL microcontrollers in STM32CubeIDE (application_note / DM00736854)](https://www.st.com/resource/en/application_note/dm00736854-getting-started-with-projects-based-on-dualcore-stm32wl-microcontrollers-in-stm32cubeide-stmicroelectronics.pdf)
* [STM32CubeMX Installation in TrueSTUDIO (application_note / STM32CubeMX_Installation_in_TruesSTUDIO)](https://www.st.com/resource/en/application_note/stm32cubemx_installation_in_truestudio-stm32cubemx-installation-in-truestudio-stmicroelectronics.pdf)
* [TrueSTUDIO for ARM® Migration Guide: IAR Embedded Workbench to TrueSTUDIO (application_note / TrueSTUDIO_for_ARM_Migration_Guide)](https://www.st.com/resource/en/application_note/iar_to_atollic_truestudio_migration_guide-truestudio-for-arm-migration-guide-iar-embedded-workbench-to-truestudio-stmicroelectronics.pdf)
