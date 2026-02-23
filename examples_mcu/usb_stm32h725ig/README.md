# STM32H725IG examples (USB ULPI / 480Mbps)

> This is an example project with several binaries for the STM32H725IG MCU with ULPI. This project uses `embassy` and
> `embassy-usb` so it can easily be adapted to other STM32H7 boards or different families.
> vhrd.tech boards: B125A, B135A, B135B

## MCU Info

* Family: STM32H7
* Line: STM32H743/753
* Die: DIE450
* Device ID: 1104
* Packages:
    * STM32H743ZITx: LQFP144 (144 pins)

## Documentation

* [STM32H742, STM32H743/753 and STM32H750 Value line advanced Arm-based 32-bit MCUs (reference_manual / DM00314099)](http://www.st.com/resource/en/reference_manual/DM00314099.pdf)
* [STM32F7 Series and STM32H7 Series Cortex-M7 processor programming manual (programming_manual / DM00237416)](http://www.st.com/resource/en/programming_manual/DM00237416.pdf)
* [32-bit Arm Cortex-M7 480MHz MCUs, up to 2MB Flash, up to 1MB RAM, 46 com. and analog interfaces (datahseet / DM00387108)](http://www.st.com/resource/en/datasheet/DM00387108.pdf)
* [STM32H742xI/G and STM32H743xI/G device limitations (errata_sheet / DM00368411)](http://www.st.com/resource/en/errata_sheet/DM00368411.pdf)
* [STM32 microcontroller system memory boot mode (application_note / CD00167594)](http://www.st.com/resource/en/application_note/CD00167594.pdf)
* [How to get the best ADC accuracy in STM32 microcontrollers (application_note / CD00211314)](http://www.st.com/resource/en/application_note/CD00211314.pdf)
* [Audio and waveform generation using the DAC in STM32 products (application_note / CD00259245)](http://www.st.com/resource/en/application_note/CD00259245.pdf)
* [USART protocol used in the STM32 bootloader (application_note / CD00264342)](http://www.st.com/resource/en/application_note/CD00264342.pdf)
* [USB DFU protocol used in the STM32 bootloader (application_note / CD00264379)](http://www.st.com/resource/en/application_note/CD00264379.pdf)
* [STM32 cross-series timer overview (application_note / DM00042534)](http://www.st.com/resource/en/application_note/DM00042534.pdf)
* [I2C protocol used in the STM32 bootloader (application_note / DM00072315)](http://www.st.com/resource/en/application_note/DM00072315.pdf)
* [How to implement a vocoder solution using STM32 microcontrollers (application_note / DM00073742)](http://www.st.com/resource/en/application_note/DM00073742.pdf)
* [STM32 microcontroller random number generation validation using the NIST statistical test suite (application_note / DM00073853)](http://www.st.com/resource/en/application_note/DM00073853.pdf)
* [SPI protocol used in the STM32 bootloader (application_note / DM00081379)](http://www.st.com/resource/en/application_note/DM00081379.pdf)
* [HRTIM cookbook (application_note / DM00121475)](http://www.st.com/resource/en/application_note/DM00121475.pdf)
* [Extending the DAC performance of STM32 microcontrollers (application_note / DM00129215)](http://www.st.com/resource/en/application_note/DM00129215.pdf)
* [Minimization of power consumption using LPUART for STM32 microcontrollers (application_note / DM00151811)](http://www.st.com/resource/en/application_note/DM00151811.pdf)
* [Virtually increasing the number of serial communication peripherals in STM32 applications (application_note / DM00160482)](http://www.st.com/resource/en/application_note/DM00160482.pdf)
* [Handling of soft errors in STM32 applications (application_note / DM00220769)](http://www.st.com/resource/en/application_note/DM00220769.pdf)
* [Using the hardware real-time clock (RTC) and the tamper management unit (TAMP) with STM32 microcontrollers (application_note / DM00226326)](http://www.st.com/resource/en/application_note/DM00226326.pdf)
* [Quad-SPI interface on STM32 microcontrollers and microprocessors (application_note / DM00227538)](http://www.st.com/resource/en/application_note/DM00227538.pdf)
* [General-purpose timer cookbook for STM32 microcontrollers (application_note / DM00236305)](http://www.st.com/resource/en/application_note/DM00236305.pdf)
* [High-speed SI simulations using IBIS and board-level simulations using HyperLynx SI on STM32 MCUs and MPUs (application_note / DM00257177)](http://www.st.com/resource/en/application_note/DM00257177.pdf)
* [Managing memory protection unit in STM32 MCUs (application_note / DM00272912)](http://www.st.com/resource/en/application_note/DM00272912.pdf)
* [Level 1 cache on STM32F7 Series and STM32H7 Series (application_note / DM00272913)](http://www.st.com/resource/en/application_note/DM00272913.pdf)
* [LCD-TFT display controller (LTDC) on STM32 MCUs (application_note / DM00287603)](http://www.st.com/resource/en/application_note/DM00287603.pdf)
* [USB hardware and PCB guidelines using STM32 MCUs (application_note / DM00296349)](http://www.st.com/resource/en/application_note/DM00296349.pdf)
* [STM32 GPIO configuration for hardware settings and low-power consumption (application_note / DM00315319)](http://www.st.com/resource/en/application_note/DM00315319.pdf)
* [STM32 USART automatic baud rate detection (application_note / DM00327191)](http://www.st.com/resource/en/application_note/DM00327191.pdf)
* [Migration of microcontroller applications from STM32F7 Series to STM32H743/753 line (application_note / DM00337702)](http://www.st.com/resource/en/application_note/DM00337702.pdf)
* [Getting started with STM32H74xI/G and STM32H75xI/G hardware development (application_note / DM00337873)](http://www.st.com/resource/en/application_note/DM00337873.pdf)
* [STM32 microcontroller debug toolbox (application_note / DM00354244)](http://www.st.com/resource/en/application_note/DM00354244.pdf)
* [Getting started with sigma-delta digital interface on applicable STM32 microcontrollers (application_note / DM00354333)](http://www.st.com/resource/en/application_note/DM00354333.pdf)
* [Hardware JPEG codec peripheral in STM32F76/77xxx and STM32H743/53/45/55/47/57/50/A3/B3/B0xx microcontrollers (application_note / DM00356635)](http://www.st.com/resource/en/application_note/DM00356635.pdf)
* [Digital camera interface (DCMI) on STM32 MCUs (application_note / DM00373474)](http://www.st.com/resource/en/application_note/DM00373474.pdf)
* [Interfacing PDM digital microphones using STM32 MCUs and MPUs (application_note / DM00380469)](http://www.st.com/resource/en/application_note/DM00380469.pdf)
* [STM32Cube MCU Package examples for STM32H7 Series (application_note / DM00393275)](http://www.st.com/resource/en/application_note/DM00393275.pdf)
* [Thermal management guidelines for STM32 applications (application_note / DM00395696)](http://www.st.com/resource/en/application_note/DM00395696.pdf)
* [Receiving S/PDIF audio stream with the STM32F4/F7/H7 Series (application_note / DM00431633)](http://www.st.com/resource/en/application_note/DM00431633.pdf)
* [Introduction to STM32 microcontrollers security (application_note / DM00493651)](http://www.st.com/resource/en/application_note/DM00493651.pdf)
* [Getting started with STM32H7 Series SDMMC host controller (application_note / DM00525510)](http://www.st.com/resource/en/application_note/DM00525510.pdf)
* [STM32 DMAMUX: the DMA request router (application_note / DM00535045)](http://www.st.com/resource/en/application_note/DM00535045.pdf)
* [USB Type-C Power Delivery using STM32 MCUs and MPUs (application_note / DM00536349)](http://www.st.com/resource/en/application_note/DM00536349.pdf)
* [Migration guide from STM32F7 Series and STM32H743/753 line, to STM32H7A3/7B3 and STM32H7B0 Value line devices (application_note / DM00600614)](http://www.st.com/resource/en/application_note/DM00600614.pdf)
* [Migration from RevY to RevV for STM32H743/753 and STM32H750 Value line microcontrollers (application_note / DM00609692)](http://www.st.com/resource/en/application_note/DM00609692.pdf)
* [STM32H7 Series lifetime estimates (application_note / DM00622045)](http://www.st.com/resource/en/application_note/DM00622045.pdf)
* [Error correction code (ECC) management for internal memories protection on STM32H7 Series (application_note / DM00623136)](http://www.st.com/resource/en/application_note/DM00623136.pdf)
* [FDCAN peripheral on STM32 devices (application_note / DM00625700)](http://www.st.com/resource/en/application_note/DM00625700.pdf)
* [Getting started with the STM32H7 Series MCU 16-bit ADC (application_note / DM00628458)](http://www.st.com/resource/en/application_note/DM00628458.pdf)
* [FDCAN protocol used in the STM32 bootloader (application_note / DM00660346)](http://www.st.com/resource/en/application_note/DM00660346.pdf)
* [Enhanced methods to handle SPI communication on STM32 devices (application_note / DM00725181)](http://www.st.com/resource/en/application_note/DM00725181.pdf)
