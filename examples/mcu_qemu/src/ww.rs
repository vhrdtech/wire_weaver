use wire_weaver::prelude::*;
use ww_client_server::{StreamSidebandCommand, StreamSidebandEvent};
use ww_gpio::{AvailablePins, BankCapabilities, Error, IoPinEnabledEvents, Level, Mode, Pull, Speed, Volt};

#[ww_trait]
pub trait McuGpio {
    ww_impl!(bank_a: "../../ww_gpio/src/lib.rs" as ww_gpio::GpioBank);
}

pub struct ServerState {
}

impl ServerState {
    fn bank_a_available(&mut self, ) -> AvailablePins<'_> { todo!() }
    fn bank_a_capabilities(&mut self, ) -> BankCapabilities<'_> { todo!() }
    fn bank_a_reference_voltage(&mut self, ) -> Volt { todo!() }
    fn bank_a_set_reference_voltage(&mut self, _quantity: Volt) -> Result<(), Error> { todo!() }
    fn bank_a_set_mode(&mut self, _mode: Mode, _initial: Option<Level>) -> Result<(), Error> { todo!() }
    fn bank_a_mode(&mut self) -> Result<Mode, Error> { todo!() }
    fn bank_a_set_speed(&mut self, _pull: Speed) -> Result<(), Error> { todo!() }
    fn bank_a_speed(&mut self) -> Result<Speed, Error> { todo!() }
    fn bank_a_name(&mut self) -> &'_ str { todo!() }

    fn bank_a_pin_set_high(&mut self, _index: [UNib32; 1]) {

    }
    fn bank_a_pin_set_low(&mut self, _index: [UNib32; 1]) {

    }
    fn bank_a_pin_set_output_level(&mut self, _index: [UNib32; 1], _level: Level) {}
    fn bank_a_pin_output_level(&mut self, _index: [UNib32; 1]) -> Level { todo!() }
    fn bank_a_pin_toggle(&mut self, _index: [UNib32; 1]) { }
    fn bank_a_pin_input_level(&mut self, _index: [UNib32; 1]) -> Level { todo!() }
    fn event_sideband(
        &mut self,
        _index: [UNib32; 1],
        _cmd: StreamSidebandCommand,
    ) -> Option<StreamSidebandEvent> {
        None
    }
    fn bank_a_pin_voltage(&mut self, _index: [UNib32; 1]) -> Option<Volt> { None }
    fn bank_a_pin_set_mode(&mut self, _index: [UNib32; 1], _mode: Mode, _initial: Option<Level>) -> Result<(), Error> { todo!() }
    fn bank_a_pin_mode(&mut self, _index: [UNib32; 1]) -> Mode { todo!() }
    fn set_bank_a_pin_pull(&mut self, _index: [UNib32; 1], _pull: Pull) -> Result<(), Error> { todo!() }
    fn get_bank_a_pin_pull(&mut self, _index: [UNib32; 1]) -> Pull { todo!() }
    fn set_bank_a_pin_speed(&mut self, _index: [UNib32; 1], _pull: Speed) -> Result<(), Error> { todo!() }
    fn get_bank_a_pin_speed(&mut self, _index: [UNib32; 1]) -> Speed { todo!() }
    fn bank_a_pin_configure_events(&mut self, _index: [UNib32; 1], _enabled: IoPinEnabledEvents) -> Result<(), Error> { todo!() }
}

pub mod api_server {
    use super::*;

    ww_api!(
        "src/ww.rs" as mcu_gpio::McuGpio for ServerState,
        server = true, no_alloc = true, use_async = false,
        method_model = "_=immediate",
        property_model = "_=get_set",
        debug_to_file = "./target/generated_mcu_gpio_server.rs"
    );
}
