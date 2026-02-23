use crate::ww::{BankClient, GpioClient};
use wire_weaver_client_common::Attachment;
use ww_gpio::{AvailablePinsOwned, BankCapabilitiesOwned, Level, Mode, Pull};

/// GPIO configured as Push-Pull output.
/// Blocking flavor.
pub struct PushPullOutputBlocking {
    flex: FlexBlocking,
}

/// GPIO configured as an Input with an optional Pull-Up or Pull-Down resistor.
/// Blocking flavor.
pub struct InputBlocking {
    flex: FlexBlocking,
}

/// GPIO configured as an Input or Push-Pull Low output.
/// Blocking flavor.
pub struct OpenDrainOutputBlocking {
    flex: FlexBlocking,
}

/// GPIO that can be reconfigured on the fly.
/// Supports all features of [PushPullOutput](PushPullOutputBlocking), [Input](InputBlocking) and [OpenDrainOutput](OpenDrainOutputBlocking).
///
/// Modeled after embassy Flex.
/// Blocking flavor.
pub struct FlexBlocking {
    io: GpioClient,
    mode: Option<Mode>,
    index: u32,
}

pub struct BankBlocking {
    bank: BankClient,
    available_pins: AvailablePinsOwned,
    name: Option<String>,
    capabilities: Option<BankCapabilitiesOwned>,
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("client error: '{:?}'", .0)]
    Client(#[from] wire_weaver_client_common::Error),
    #[error("ww_gpio error: '{:?}'", .0)]
    Gpio(ww_gpio::Error),
    #[error("expected ww_gpio::Gpio trait, got: '{}'", .0)]
    IncompatibleTrait(String),
    #[error("internal: '{}'", .0)]
    Internal(String),
}

impl From<ww_gpio::Error> for Error {
    fn from(e: ww_gpio::Error) -> Self {
        Error::Gpio(e)
    }
}

impl FlexBlocking {
    /// Create Flex pin and send a request to get its mode right away.
    ///
    /// Get the correct [Attachment] from a client that implements ww_gpio::GpioBank:
    /// `my_client.my_gpio_bank().pins(pin_idx).attachment()`
    pub fn new_get_mode(gpio_pin: Attachment) -> Result<FlexBlocking, Error> {
        let mut s = Self::new_ignore_mode(gpio_pin)?;
        s.mode()?;
        Ok(s)
    }

    /// Create Flex pin assuming unknown mode.
    /// Intended use is to immediately call one of the into_ methods, to save on one remote call.
    ///
    /// Get the correct [Attachment] from a client that implements ww_gpio::GpioBank:
    /// `my_client.my_gpio_bank().pins(pin_idx).attachment()`
    pub fn new_ignore_mode(gpio_pin: Attachment) -> Result<FlexBlocking, Error> {
        if (gpio_pin.trait_name() != "Gpio") || (gpio_pin.source_crate().crate_id != "ww_gpio") {
            return Err(Error::IncompatibleTrait(format!(
                "{}::{}",
                gpio_pin.source_crate().crate_id,
                gpio_pin.trait_name()
            )));
        }
        let cmd_tx = gpio_pin.cmd_tx_take();
        let index = if let Some(base_path) = cmd_tx.base_path()
            && let Some(last) = base_path.last()
        {
            last.0
        } else {
            return Err(Error::Internal("empty base path".into()));
        };
        Ok(FlexBlocking {
            io: GpioClient::new(cmd_tx),
            mode: None,
            index,
        })
    }

    /// Optionally set initial level and put the pin into push-pull output mode.
    pub fn set_as_output(&mut self, initial: Option<Level>) -> Result<(), Error> {
        self.io
            .set_mode(Mode::PushPullOutput, initial)
            .blocking_call()??;
        self.mode = Some(Mode::PushPullOutput);
        Ok(())
    }

    /// Consume self, optionally set initial level and put the pin into push-pull output mode, return [PushPullOutputBlocking].
    pub fn into_output(mut self, initial: Option<Level>) -> Result<PushPullOutputBlocking, Error> {
        self.set_as_output(initial)?;
        Ok(PushPullOutputBlocking { flex: self })
    }

    /// Put the pin into input mode.
    ///
    /// The internal pull-up or pull-down resistor can optionally be enabled according to pull.
    pub fn set_as_input(&mut self, pull: Pull) -> Result<(), Error> {
        self.io.write_pull(pull).blocking_write()?;
        self.io.set_mode(Mode::Input, None).blocking_call()??;
        self.mode = Some(Mode::Input);
        Ok(())
    }

    /// Consume self, put the pin into input mode and return [Input].
    ///
    /// The internal pull-up or pull-down resistor can optionally be enabled according to pull.
    pub fn into_input(mut self, pull: Pull) -> Result<InputBlocking, Error> {
        self.set_as_input(pull)?;
        Ok(InputBlocking { flex: self })
    }

    /// Put the pin into input + open-drain output mode.
    ///
    /// The hardware will drive the line low if you set it to low, and will leave it floating if you set it to high.
    /// When set high, input can be read to figure out whether another device is driving the line low.
    ///
    /// The internal pull-up or pull-down resistor can optionally be enabled according to pull.
    pub fn set_as_open_drain(&mut self, pull: Pull) -> Result<(), Error> {
        self.set_pull(pull)?;
        self.set_mode(Mode::OpenDrainOutput, None)?;
        self.mode = Some(Mode::OpenDrainOutput);
        Ok(())
    }

    /// Consume self, put the pin into input + open-drain output mode, return [OpenDrainOutput].
    ///
    /// The hardware will drive the line low if you set it to low, and will leave it floating if you set it to high.
    /// When set high, input can be read to figure out whether another device is driving the line low.
    ///
    /// The internal pull-up or pull-down resistor can optionally be enabled according to pull.
    pub fn into_open_drain_output(mut self, pull: Pull) -> Result<OpenDrainOutputBlocking, Error> {
        self.set_as_open_drain(pull)?;
        Ok(OpenDrainOutputBlocking { flex: self })
    }

    /// Change mode of the pin.
    /// Optionally set the initial level before changing pin mode.
    /// This method may fail with [UnsupportedMode](ww_gpio::Error::UnsupportedMode) error.
    fn set_mode(&mut self, mode: Mode, initial: Option<Level>) -> Result<(), Error> {
        self.io.set_mode(mode, initial).blocking_call()??;
        Ok(())
    }

    /// Returns current pin mode as cached locally.
    /// None is returned if mode is unknown (created by [Self::new_ignore_mode])
    pub fn mode_cached(&self) -> Option<Mode> {
        self.mode
    }

    /// Returns current pin mode, requested from remote device.
    /// Locally cached mode is also updated.
    pub fn mode(&mut self) -> Result<Mode, Error> {
        let mode = self.io.mode().blocking_call()?;
        self.mode = Some(mode);
        Ok(mode)
    }

    /// Returns GPIO index that this output is using.
    pub fn index(&self) -> u32 {
        self.index
    }

    /// Set the output level.
    pub fn set_level(&mut self, level: Level) -> Result<(), Error> {
        self.io.set_output_level(level).blocking_call()?;
        Ok(())
    }

    /// Set the output as high.
    /// If the pin is configured as open-drain, then it will be reconfigured to input.
    pub fn set_high(&mut self) -> Result<(), Error> {
        self.set_level(Level::High)
    }

    /// Set the output as low.
    pub fn set_low(&mut self) -> Result<(), Error> {
        self.set_level(Level::Low)
    }

    /// Toggle the output level.
    pub fn toggle(&mut self) -> Result<(), Error> {
        self.io.toggle().blocking_call()?;
        Ok(())
    }

    /// Get output level, previously set with [set_level](Self::set_level).
    pub fn output_level(&self) -> Result<Level, Error> {
        Ok(self.io.output_level().blocking_call()?)
    }

    /// Returns true, if output was previously set to high with [set_level](Self::set_level).
    pub fn is_set_high(&self) -> Result<bool, Error> {
        Ok(self.io.output_level().blocking_call()?.is_high())
    }

    /// Returns true, if output was previously set to low with [set_level](Self::set_level).
    pub fn is_set_low(&self) -> Result<bool, Error> {
        Ok(self.io.output_level().blocking_call()?.is_low())
    }

    /// Get pin input level.
    pub fn input_level(&self) -> Result<Level, Error> {
        Ok(self.io.input_level().blocking_call()?)
    }

    /// Returns true if pin input level is high.
    pub fn is_high(&self) -> Result<bool, Error> {
        Ok(self.io.input_level().blocking_call()?.is_high())
    }

    /// Returns true if pin input level is low.
    pub fn is_low(&self) -> Result<bool, Error> {
        Ok(self.io.input_level().blocking_call()?.is_low())
    }

    /// Enable or disable pull-up or pull-down resistor.
    pub fn set_pull(&mut self, pull: Pull) -> Result<(), Error> {
        self.io.write_pull(pull).blocking_write()?;
        Ok(())
    }
}

impl PushPullOutputBlocking {
    /// Set the output as high.
    pub fn set_high(&mut self) -> Result<(), Error> {
        self.flex.set_high()
    }

    /// Set the output as low.
    pub fn set_low(&mut self) -> Result<(), Error> {
        self.flex.set_low()
    }

    /// Toggle the output level.
    pub fn toggle(&mut self) -> Result<(), Error> {
        self.flex.toggle()
    }

    /// Set the output level.
    pub fn set_level(&mut self, level: Level) -> Result<(), Error> {
        self.flex.set_level(level)
    }

    /// Get previously set output level.
    pub fn level(&self) -> Result<Level, Error> {
        self.flex.output_level()
    }

    /// Returns GPIO index that this output is using.
    pub fn index(&self) -> u32 {
        self.flex.index()
    }
}

impl InputBlocking {
    /// Returns true if pin input level is high.
    pub fn is_high(&self) -> Result<bool, Error> {
        self.flex.is_high()
    }

    /// Returns true if pin input level is low.
    pub fn is_low(&self) -> Result<bool, Error> {
        self.flex.is_low()
    }

    /// Returns GPIO index that this output is using.
    pub fn index(&self) -> u32 {
        self.flex.index()
    }

    /// Enable or disable pull-up or pull-down resistor.
    pub fn set_pull(&mut self, pull: Pull) -> Result<(), Error> {
        self.flex.set_pull(pull)
    }
}

impl OpenDrainOutputBlocking {
    /// Set the pin as input, level will depend on internal or external pull-up resistor.
    pub fn set_high_z(&mut self) -> Result<(), Error> {
        self.flex.set_high()
    }

    /// Set the output as low.
    pub fn set_low(&mut self) -> Result<(), Error> {
        self.flex.set_low()
    }

    /// Toggle the output level between high-z and low.
    pub fn toggle(&mut self) -> Result<(), Error> {
        self.flex.toggle()
    }

    /// Set the output level.
    pub fn set_level(&mut self, level: Level) -> Result<(), Error> {
        self.flex.set_level(level)
    }

    /// If previously set as low, return Level::Low, otherwise read input level and return it.
    pub fn level(&self) -> Result<Level, Error> {
        self.flex.input_level()
    }

    /// Returns GPIO index that this output is using.
    pub fn index(&self) -> u32 {
        self.flex.index()
    }
}

impl BankBlocking {
    /// Create blocking Bank client and send a request to get available pins right away.
    ///
    /// Get the correct [Attachment] from a client that implements ww_gpio::GpioBank:
    /// `my_client.my_gpio_bank().attachment()`
    pub fn new(bank: Attachment) -> Result<BankBlocking, Error> {
        if (bank.trait_name() != "GpioBank") || (bank.source_crate().crate_id != "ww_gpio") {
            return Err(Error::IncompatibleTrait(format!(
                "{}::{}",
                bank.source_crate().crate_id,
                bank.trait_name()
            )));
        }
        let cmd_tx = bank.cmd_tx_take();
        let bank = BankClient::new(cmd_tx);
        let available_pins = bank.available().blocking_call()?;
        Ok(BankBlocking {
            bank,
            available_pins,
            name: None,
            capabilities: None,
        })
    }

    /// Returns available pins on this bank.
    pub fn available_pins(&self) -> &AvailablePinsOwned {
        &self.available_pins
    }

    /// Returns cached bank name if it was requested before, otherwise a request is made first.
    pub fn bank_name(&mut self) -> Result<String, Error> {
        if let Some(name) = self.name.clone() {
            Ok(name)
        } else {
            let name = self.bank.name().blocking_call()?;
            self.name = Some(name.clone());
            Ok(name)
        }
    }

    pub fn capabilities(&mut self) -> Result<BankCapabilitiesOwned, Error> {
        if let Some(cap) = self.capabilities.clone() {
            Ok(cap)
        } else {
            let capabilities = self.bank.capabilities().blocking_call()?;
            self.capabilities = Some(capabilities.clone());
            Ok(capabilities)
        }
    }
}
