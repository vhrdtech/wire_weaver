# Global trait ID registry

Below is a table with common traits, with manually assigned unique IDs.
It is possible to communicate with a device, without knowing anything about it apart from the fact that it speaks
WireWeaver.
To do this, instead of sending a resource path, trait's crate name and version is sent instead. Device knows which
traits
are implemented and can process requested targeted at them.

The only downside of doing this - payloads become rather large (e.g., for `ww_log_bare_metal` it's 17 bytes only for the
name). Especially if you want to send many requests in one go in one packet.
To solve this, this registry was created, which replaces long names with a small ID.

IDs `0..=7` consume only one nibble (4 bits), `8..63` - 1 byte, `64..=511` - 1.5 bytes, `512..=4096` - 2 bytes, etc.
So small numbers should be used sparingly, only for things that might be used a lot, like GPIO control or logging for
example.

If you create a generic trait that you think is useful for bare-metal usage, create a PR and assign an ID for it.

| Crate                | Description        |                  Link                  |
|----------------------|--------------------|:--------------------------------------:|
| ww_log_bare_metal    | Logging support    | [crates.io](https://crates.io/crates/) |
| ww_counters          | Counters support   |                                        |
| ww_uid               | Device unique ID   |                                        |
| ww_board_info        | PCB information    |                                        |
| ww_dfu               | Firmware update    |                                        |
| ww_gpio              | Working with GPIOs |                                        |
| ww_can_bus           | CAN Bus API        |                                        |
| ww_uart              | UART/USART API     |                                        |
| ww_spi               | SPI API            |                                        |
| ww_i2c               | I2C API            |                                        |
| wire_weaver_usb_link | USB link layer     |                                        |

IDs are stored in `wire_weaver_gid.json` file for easy machine consumption. `lib.rs` is automatically generated in
`build.rs`.