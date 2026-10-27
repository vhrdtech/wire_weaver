//! Async flavor: [Flex], [PushPullOutput], [Input], [OpenDrainOutput] and [Bank] over the async client, for code
//! in an async task. [Flex], [Input] and [OpenDrainOutput] implement `embedded_hal_async::digital::Wait`.
//!
//! # Waiting without a race
//!
//! Every wait opens the pin's `event` stream with the edges it needs first, then (for a level) reads the input
//! level, then awaits an edge: an edge that comes after the call can't be missed, one from before it doesn't count.
//! The stream is closed again when the wait returns or is dropped. Opening the stream is one round trip, reading the
//! level another; a pin already at the level returns after these two.
//!
//! [Flex::last_edge_time] keeps the device time of the edge that ended the last wait (when the device sends one),
//! to measure from the moment the line moved, not from when the host heard about it.
//!
//! A gap in the stream (the device dropped events) ends an edge wait, since the edge asked for may be among the
//! lost ones, and makes a level wait read the level again.
//!
//! `embedded_hal::digital` (`OutputPin`, `InputPin`, ...) is synchronous, so it is implemented by the
//! [blocking](crate::blocking) flavor: a blocking call inside an async task would stall it.

use crate::Error;
use crate::ww::{BankClient, GpioClient};
use embedded_hal::digital::ErrorType;
use wire_weaver::ValidIndicesOwned;
use wire_weaver_client::{Attachment, Priority, Second, StreamEvent};
use ww_gpio::{
    BankCapabilitiesOwned, BankConfig, IoPinEnabledEventsOwned, IoPinEvent, Level, Mode, Pull, Volt,
};

/// GPIO configured as Push-Pull output.
/// Async flavor.
pub struct PushPullOutput {
    flex: Flex,
}

/// GPIO configured as an Input with an optional Pull-Up or Pull-Down resistor.
/// Async flavor.
pub struct Input {
    flex: Flex,
}

/// GPIO configured as an Input or Push-Pull Low output.
/// Async flavor.
pub struct OpenDrainOutput {
    flex: Flex,
}

/// GPIO that can be reconfigured on the fly.
/// Supports all features of [PushPullOutput], [Input] and [OpenDrainOutput].
///
/// Modeled after embassy Flex.
/// Async flavor.
pub struct Flex {
    io: GpioClient,
    mode: Option<Mode>,
    index: u32,
    last_edge_time: Option<Second>,
}

/// A bank of pins.
/// Async flavor.
pub struct Bank {
    bank: BankClient,
    available_pins: ValidIndicesOwned,
    name: Option<String>,
    capabilities: Option<BankCapabilitiesOwned>,
}

/// What a wait waits for.
#[derive(Clone, Copy)]
enum Until {
    Level(Level),
    /// An edge of this kind, `None`: either.
    Edge(Option<IoPinEvent>),
}

impl Flex {
    /// Create Flex pin and send a request to get its mode right away.
    ///
    /// Get the correct [Attachment] from a client that implements ww_gpio::Bank:
    /// `my_client.my_gpio_bank().pin(pin_idx).attachment()`
    pub async fn new_get_mode(gpio_pin: Attachment) -> Result<Flex, Error> {
        let mut s = Self::new_ignore_mode(gpio_pin)?;
        s.mode().await?;
        Ok(s)
    }

    /// Create Flex pin assuming unknown mode.
    /// The intended use is to immediately call one of the into_ methods to save on one remote call.
    ///
    /// Get the correct [Attachment] from a client that implements ww_gpio::Bank:
    /// `my_client.my_gpio_bank().pin(pin_idx).attachment()`
    pub fn new_ignore_mode(gpio_pin: Attachment) -> Result<Flex, Error> {
        let (io, index) = GpioClient::from_attachment(gpio_pin)?;
        Ok(Flex {
            io,
            mode: None,
            index,
            last_edge_time: None,
        })
    }

    /// Priority of this pin's requests from now on, [Priority::NORMAL] unless the client was configured with
    /// another default. [Priority::URGENT] and above go out at once and are answered at once.
    pub fn set_priority(&mut self, priority: Priority) {
        self.io.set_priority(priority);
    }

    /// Priority of this pin's requests.
    pub fn priority(&self) -> Priority {
        self.io.priority()
    }

    /// This pin with every request urgent ([Priority::URGENT]): for latency-bound drivers.
    pub fn urgent(mut self) -> Self {
        self.set_priority(Priority::URGENT);
        self
    }

    /// Optionally set initial level and put the pin into push-pull output mode.
    pub async fn set_as_output(&mut self, initial: Option<Level>) -> Result<(), Error> {
        self.set_mode(Mode::PushPullOutput, initial).await
    }

    /// Consume self, optionally set the initial level and put the pin into push-pull output mode, return
    /// [PushPullOutput].
    pub async fn into_output(mut self, initial: Option<Level>) -> Result<PushPullOutput, Error> {
        self.set_as_output(initial).await?;
        Ok(PushPullOutput { flex: self })
    }

    /// Put the pin into input mode.
    ///
    /// The internal pull-up or pull-down resistor can optionally be enabled according to pull.
    pub async fn set_as_input(&mut self, pull: Pull) -> Result<(), Error> {
        self.set_pull(pull).await?;
        self.set_mode(Mode::Input, None).await
    }

    /// Consume self, put the pin into input mode and return [Input].
    ///
    /// The internal pull-up or pull-down resistor can optionally be enabled according to pull.
    pub async fn into_input(mut self, pull: Pull) -> Result<Input, Error> {
        self.set_as_input(pull).await?;
        Ok(Input { flex: self })
    }

    /// Put the pin into input + open-drain output mode.
    ///
    /// The hardware will drive the line low if you set it to low and will leave it floating if you set it to high.
    /// When set high, input can be read to figure out whether another device is driving the line low.
    ///
    /// The internal pull-up or pull-down resistor can optionally be enabled according to pull.
    pub async fn set_as_open_drain(&mut self, pull: Pull) -> Result<(), Error> {
        self.set_pull(pull).await?;
        self.set_mode(Mode::OpenDrainOutput, None).await
    }

    /// Consume self, put the pin into input + open-drain output mode, return [OpenDrainOutput].
    ///
    /// The internal pull-up or pull-down resistor can optionally be enabled according to pull.
    pub async fn into_open_drain_output(mut self, pull: Pull) -> Result<OpenDrainOutput, Error> {
        self.set_as_open_drain(pull).await?;
        Ok(OpenDrainOutput { flex: self })
    }

    /// Change the mode of the pin, optionally setting the initial level first.
    /// This method may fail with an unsupported-mode error (`GpioErrorOwned::Common(CommonErrorOwned::Unsupported)`).
    pub async fn set_mode(&mut self, mode: Mode, initial: Option<Level>) -> Result<(), Error> {
        self.io.set_mode(mode, initial).call().await?;
        self.mode = Some(mode);
        Ok(())
    }

    /// Returns current pin mode as cached locally.
    /// None is returned if the mode is unknown (created by [Self::new_ignore_mode])
    pub fn mode_cached(&self) -> Option<Mode> {
        self.mode
    }

    /// Returns current pin mode, requested from a remote device.
    /// Locally cached mode is also updated.
    pub async fn mode(&mut self) -> Result<Mode, Error> {
        let mode = self.io.mode().call().await?;
        self.mode = Some(mode);
        Ok(mode)
    }

    /// Returns the GPIO index that this pin is using.
    pub fn index(&self) -> u32 {
        self.index
    }

    /// Set the output level.
    pub async fn set_level(&mut self, level: Level) -> Result<(), Error> {
        self.io.set_output_level(level).call().await?;
        Ok(())
    }

    /// Set the output as high.
    pub async fn set_high(&mut self) -> Result<(), Error> {
        self.set_level(Level::High).await
    }

    /// Set the output as low.
    pub async fn set_low(&mut self) -> Result<(), Error> {
        self.set_level(Level::Low).await
    }

    /// Toggle the output level.
    pub async fn toggle(&mut self) -> Result<(), Error> {
        self.io.toggle().call().await?;
        Ok(())
    }

    /// Get the output level, previously set with [set_level](Self::set_level).
    pub async fn output_level(&self) -> Result<Level, Error> {
        Ok(self.io.output_level().call().await?)
    }

    /// Returns true, if output was previously set to high.
    pub async fn is_set_high(&self) -> Result<bool, Error> {
        Ok(self.output_level().await?.is_high())
    }

    /// Returns true, if output was previously set to low.
    pub async fn is_set_low(&self) -> Result<bool, Error> {
        Ok(self.output_level().await?.is_low())
    }

    /// Get pin input level.
    pub async fn input_level(&self) -> Result<Level, Error> {
        Ok(self.io.input_level().call().await?)
    }

    /// Returns true if pin input level is high.
    pub async fn is_high(&self) -> Result<bool, Error> {
        Ok(self.input_level().await?.is_high())
    }

    /// Returns true if pin input level is low.
    pub async fn is_low(&self) -> Result<bool, Error> {
        Ok(self.input_level().await?.is_low())
    }

    /// Enable or disable pull-up or pull-down resistor.
    pub async fn set_pull(&mut self, pull: Pull) -> Result<(), Error> {
        self.io.write_pull(pull).write().await?;
        Ok(())
    }

    /// Device time of the edge that ended the last wait (`Wait::wait_for_*`), when the device sends one. `None`
    /// before the first wait, when the last one found the pin at its level already, ended on a gap in the event
    /// stream or was dropped.
    pub fn last_edge_time(&self) -> Option<Second> {
        self.last_edge_time
    }

    async fn wait(&mut self, until: Until) -> Result<(), Error> {
        use IoPinEvent::{FallingEdge, RisingEdge};
        self.last_edge_time = None;
        let (rising, falling) = match until {
            Until::Level(Level::High) | Until::Edge(Some(RisingEdge)) => (true, false),
            Until::Level(Level::Low) | Until::Edge(Some(FallingEdge)) => (false, true),
            Until::Edge(_) => (true, true),
        };
        // Edges are reported from here on, before the level is read: none can fall between the two. Dropping the
        // stream at the end (or when this future is dropped) closes it.
        let ((), mut events) = self
            .io
            .event(IoPinEnabledEventsOwned {
                rising,
                falling,
                custom: vec![],
            })
            .call()
            .await??;
        if let Until::Level(level) = until
            && self.input_level().await? == level
        {
            return Ok(());
        }
        loop {
            match events.recv_event().await? {
                // only the edges asked for are enabled
                StreamEvent::Item {
                    t,
                    value: RisingEdge | FallingEdge,
                    ..
                } => {
                    self.last_edge_time = t;
                    return Ok(());
                }
                StreamEvent::Gap { .. } => match until {
                    Until::Edge(_) => return Ok(()),
                    Until::Level(level) => {
                        if self.input_level().await? == level {
                            return Ok(());
                        }
                    }
                },
                _ => {}
            }
        }
    }
}

impl ErrorType for Flex {
    type Error = Error;
}

impl embedded_hal_async::digital::Wait for Flex {
    async fn wait_for_high(&mut self) -> Result<(), Self::Error> {
        self.wait(Until::Level(Level::High)).await
    }

    async fn wait_for_low(&mut self) -> Result<(), Self::Error> {
        self.wait(Until::Level(Level::Low)).await
    }

    async fn wait_for_rising_edge(&mut self) -> Result<(), Self::Error> {
        self.wait(Until::Edge(Some(IoPinEvent::RisingEdge))).await
    }

    async fn wait_for_falling_edge(&mut self) -> Result<(), Self::Error> {
        self.wait(Until::Edge(Some(IoPinEvent::FallingEdge))).await
    }

    async fn wait_for_any_edge(&mut self) -> Result<(), Self::Error> {
        self.wait(Until::Edge(None)).await
    }
}

impl PushPullOutput {
    /// Set the output as high.
    pub async fn set_high(&mut self) -> Result<(), Error> {
        self.flex.set_high().await
    }

    /// Set the output as low.
    pub async fn set_low(&mut self) -> Result<(), Error> {
        self.flex.set_low().await
    }

    /// Toggle the output level.
    pub async fn toggle(&mut self) -> Result<(), Error> {
        self.flex.toggle().await
    }

    /// Set the output level.
    pub async fn set_level(&mut self, level: Level) -> Result<(), Error> {
        self.flex.set_level(level).await
    }

    /// Get previously set output level.
    pub async fn level(&self) -> Result<Level, Error> {
        self.flex.output_level().await
    }

    /// The pin underneath, for its priority and everything else.
    pub fn flex(&mut self) -> &mut Flex {
        &mut self.flex
    }
}

impl Input {
    /// Returns true if pin input level is high.
    pub async fn is_high(&self) -> Result<bool, Error> {
        self.flex.is_high().await
    }

    /// Returns true if pin input level is low.
    pub async fn is_low(&self) -> Result<bool, Error> {
        self.flex.is_low().await
    }

    /// Enable or disable pull-up or pull-down resistor.
    pub async fn set_pull(&mut self, pull: Pull) -> Result<(), Error> {
        self.flex.set_pull(pull).await
    }

    /// See [Flex::last_edge_time].
    pub fn last_edge_time(&self) -> Option<Second> {
        self.flex.last_edge_time()
    }

    /// The pin underneath, for its priority and everything else.
    pub fn flex(&mut self) -> &mut Flex {
        &mut self.flex
    }
}

impl OpenDrainOutput {
    /// Set the pin as input, level will depend on internal or external pull-up resistor.
    pub async fn set_high_z(&mut self) -> Result<(), Error> {
        self.flex.set_high().await
    }

    /// Set the output as low.
    pub async fn set_low(&mut self) -> Result<(), Error> {
        self.flex.set_low().await
    }

    /// Toggle the output level between high-z and low.
    pub async fn toggle(&mut self) -> Result<(), Error> {
        self.flex.toggle().await
    }

    /// Set the output level.
    pub async fn set_level(&mut self, level: Level) -> Result<(), Error> {
        self.flex.set_level(level).await
    }

    /// The level of the line: low when driven low here or by another device.
    pub async fn level(&self) -> Result<Level, Error> {
        self.flex.input_level().await
    }

    /// See [Flex::last_edge_time].
    pub fn last_edge_time(&self) -> Option<Second> {
        self.flex.last_edge_time()
    }

    /// The pin underneath, for its priority and everything else.
    pub fn flex(&mut self) -> &mut Flex {
        &mut self.flex
    }
}

macro_rules! wait_through_flex {
    ($($pin:ty),*) => {$(
        impl ErrorType for $pin {
            type Error = Error;
        }

        impl embedded_hal_async::digital::Wait for $pin {
            async fn wait_for_high(&mut self) -> Result<(), Self::Error> {
                self.flex.wait_for_high().await
            }

            async fn wait_for_low(&mut self) -> Result<(), Self::Error> {
                self.flex.wait_for_low().await
            }

            async fn wait_for_rising_edge(&mut self) -> Result<(), Self::Error> {
                self.flex.wait_for_rising_edge().await
            }

            async fn wait_for_falling_edge(&mut self) -> Result<(), Self::Error> {
                self.flex.wait_for_falling_edge().await
            }

            async fn wait_for_any_edge(&mut self) -> Result<(), Self::Error> {
                self.flex.wait_for_any_edge().await
            }
        }
    )*};
}

wait_through_flex!(Input, OpenDrainOutput);

impl Bank {
    /// Create a Bank client and send a request to get available pins right away.
    ///
    /// Get the correct [Attachment] from a client that implements ww_gpio::Bank:
    /// `my_client.my_gpio_bank().attachment()`
    pub async fn new(attachment: Attachment) -> Result<Bank, Error> {
        let bank = BankClient::new(attachment)?;
        let available_pins = bank.pin_valid_indices().read().await?;
        Ok(Bank {
            bank,
            available_pins,
            name: None,
            capabilities: None,
        })
    }

    /// Returns available pins on this bank.
    pub fn available_pins(&self) -> &ValidIndicesOwned {
        &self.available_pins
    }

    /// Returns cached bank name if it was requested before, otherwise a request is made first.
    pub async fn name(&mut self) -> Result<String, Error> {
        if let Some(name) = &self.name {
            return Ok(name.clone());
        }
        let name = self.bank.name().call().await?;
        self.name = Some(name.clone());
        Ok(name)
    }

    /// Returns cached bank capabilities if they were requested before, otherwise a request is made first.
    pub async fn capabilities(&mut self) -> Result<BankCapabilitiesOwned, Error> {
        if let Some(cap) = &self.capabilities {
            return Ok(cap.clone());
        }
        let capabilities = self.bank.capabilities().call().await?;
        self.capabilities = Some(capabilities.clone());
        Ok(capabilities)
    }

    /// Sets the IO level of every pin of the bank, if supported by hardware, and returns the level in effect
    /// (`None` on a bank without level control).
    pub async fn set_io_level(&mut self, level: Volt) -> Result<Option<Volt>, Error> {
        let actual = self
            .bank
            .configure(BankConfig {
                io_level: Some(level),
            })
            .call()
            .await?;
        Ok(actual.io_level)
    }

    /// IO level of the bank now (`None` on a bank without level control): a `configure` that changes nothing.
    pub async fn io_level(&mut self) -> Result<Option<Volt>, Error> {
        let actual = self
            .bank
            .configure(BankConfig { io_level: None })
            .call()
            .await?;
        Ok(actual.io_level)
    }

    /// Get pin at `pin_idx` as a [Flex], assuming unknown mode.
    /// The intended use is to immediately call one of the into_ methods to save on one remote call.
    ///
    /// Returns [Error::Usage] if `pin_idx` is not one of [Self::available_pins].
    pub fn pin(&self, pin_idx: u32) -> Result<Flex, Error> {
        self.check_idx(pin_idx)?;
        Flex::new_ignore_mode(self.bank.pin(pin_idx).attachment())
    }

    /// Get pin at `pin_idx` as a [Flex] and send a request to get its mode right away.
    ///
    /// Returns [Error::Usage] if `pin_idx` is not one of [Self::available_pins].
    pub async fn pin_get_mode(&self, pin_idx: u32) -> Result<Flex, Error> {
        self.check_idx(pin_idx)?;
        Flex::new_get_mode(self.bank.pin(pin_idx).attachment()).await
    }

    /// Get all available pins on this bank as [Flex], assuming unknown mode.
    pub fn all_pins(&self) -> Result<Vec<Flex>, Error> {
        self.available_pins
            .iter()
            .map(|pin_idx| Flex::new_ignore_mode(self.bank.pin(pin_idx).attachment()))
            .collect()
    }

    fn check_idx(&self, pin_idx: u32) -> Result<(), Error> {
        if !self.available_pins.contains(pin_idx) {
            return Err(Error::Usage(format!(
                "Pin index {pin_idx} is not valid for this bank"
            )));
        }
        Ok(())
    }
}
