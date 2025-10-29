use crate::WireWeaverClass;
use embassy_usb::driver::Driver;
use embassy_usb::msos::windows_version;
use embassy_usb::{Builder, Config, UsbDevice};

pub struct UsbContext<'d, D: Driver<'d>> {
    pub(crate) usb: UsbDevice<'d, D>,
    pub(crate) ww: WireWeaverClass<'d, D>,
}

pub struct UsbInitBuffers {
    config_descriptor: [u8; 256],
    bos_descriptor: [u8; 256],
    msos_descriptor: [u8; 256],
    control: [u8; 128],
}

impl Default for UsbInitBuffers {
    fn default() -> Self {
        UsbInitBuffers {
            config_descriptor: [0u8; 256],
            bos_descriptor: [0u8; 256],
            msos_descriptor: [0u8; 256],
            control: [0u8; 128],
        }
    }
}

/// Initializes USB stack with default configuration and a single interface with WireWeaver class.
/// Device should work without drivers in Linux, macOS and Windows.
///
/// This functions is a convenient way to initialize a minimum working device, if you need more advanced setup,
/// you can do the same steps directly and extend accordingly.
///
/// It is recommended to adjust USB config in the config_mut closure, in particular:
/// * Set vid, pid (default is 0xc0de:0xcafe)
/// * Set manufacturer and product (default is "Vhrd.Tech" "WireWeaver Generic")
/// * Set serial_number (default is None, use e.g., embassy_stm32::uid::uid_hex())
/// * max_power (default is 100mA)
/// * self_powered (default is false)
pub fn usb_init<'d, 'b: 'd, D: Driver<'d>, C: FnOnce(&mut Config)>(
    driver: D,
    buffers: &'b mut UsbInitBuffers,
    config_mut: C,
) -> UsbContext<'d, D> {
    let mut config = Config::new(0xc0de, 0xcafe);
    config.manufacturer = Some("Vhrd.Tech");
    config.product = Some("WireWeaver Generic");

    // Required for windows compatibility.
    // https://developer.nordicsemi.com/nRF_Connect_SDK/doc/1.9.1/kconfig/CONFIG_CDC_ACM_IAD.html#help
    config.device_class = 0xEF;
    config.device_sub_class = 0x02;
    config.device_protocol = 0x01;
    config.composite_with_iads = true;

    config.max_power = 100;
    config.self_powered = false;
    config_mut(&mut config);

    let mut builder = Builder::new(
        driver,
        config,
        &mut buffers.config_descriptor,
        &mut buffers.bos_descriptor,
        &mut buffers.msos_descriptor,
        &mut buffers.control,
    );

    builder.msos_descriptor(windows_version::WIN8_1, 2);

    // Create class on the builder.
    let ww = WireWeaverClass::new(&mut builder, 64);

    // Build the builder.
    let usb = builder.build();
    defmt::info!("USB builder built");

    UsbContext { usb, ww }
}
