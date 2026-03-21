use crate::ww::BankClient;
use crate::Error;
use std::sync::Arc;
use wire_weaver::ValidIndicesOwned;
use wire_weaver_client_common::promise::{Promise, PromiseState};
use wire_weaver_client_common::Attachment;
use ww_gpio::{Level, Mode, Pull, Speed};

pub struct BankPromise {
    bank: BankClient,
    valid_pin_indices: Option<Arc<Vec<u32>>>,
    valid_pin_indices_promise: Promise<ValidIndicesOwned>,
    bank_name: Option<String>,
    bank_name_promise: Promise<String>,
}

impl BankPromise {
    /// Create a promise-based Bank client from the attachment.
    ///
    /// Get the correct [Attachment] from a client that implements ww_gpio::Bank:
    /// `my_client.my_gpio_bank().attachment()`
    pub fn new(attachment: Attachment) -> Result<BankPromise, Error> {
        let bank = BankClient::new(attachment)?;
        let valid_pin_indices_promise = bank.pin_valid_indices().read_promise("");
        Ok(BankPromise {
            bank,
            valid_pin_indices: None,
            valid_pin_indices_promise,
            bank_name: None,
            bank_name_promise: Default::default(),
        })
    }

    pub fn new_hint(
        attachment: Attachment,
        valid_pin_indices: Option<Arc<Vec<u32>>>,
        bank_name: Option<String>,
    ) -> Result<BankPromise, Error> {
        let bank = BankClient::new(attachment)?;
        let valid_pin_indices_promise = if valid_pin_indices.is_some() {
            Default::default()
        } else {
            bank.pin_valid_indices().read_promise("")
        };
        Ok(BankPromise {
            bank,
            valid_pin_indices,
            valid_pin_indices_promise,
            bank_name,
            bank_name_promise: Default::default(),
        })
    }

    pub fn sync_poll(&mut self) {
        self.valid_pin_indices_promise.sync_poll();
        if let Some(valid_pin_indices) = self.valid_pin_indices_promise.take_ready() {
            self.valid_pin_indices = Some(Arc::new(valid_pin_indices.iter().collect()));
        }
        self.bank_name_promise.sync_poll();
        if let Some(bank_name) = self.bank_name_promise.take_ready() {
            self.bank_name = Some(bank_name);
        }
    }

    pub fn pin_indices(&self) -> Option<&[u32]> {
        self.valid_pin_indices.as_ref().map(|v| v.as_slice())
    }

    pub fn request_pin_indices(&mut self) {
        if self.valid_pin_indices_promise.is_waiting() {
            return;
        }
        self.valid_pin_indices_promise = self.bank.pin_valid_indices().read_promise("");
    }

    pub fn pin_ref(&self, pin_idx: u32) -> Result<FlexPromiseRef<'_>, Error> {
        self.check_idx(pin_idx)?;
        Ok(FlexPromiseRef {
            bank: &self.bank,
            pin_idx,
        })
    }

    pub fn pin(&self, pin_idx: u32) -> Result<FlexPromise, Error> {
        self.check_idx(pin_idx)?;
        Ok(FlexPromise::new(self.bank.clone(), pin_idx))
    }

    pub fn all_pins(&self) -> Vec<FlexPromise> {
        self.pin_indices()
            .unwrap_or(&[])
            .iter()
            .map(|&pin_idx| FlexPromise::new(self.bank.clone(), pin_idx))
            .collect()
    }

    pub fn bank_name(&self) -> Option<&str> {
        self.bank_name.as_deref()
    }

    pub fn request_bank_name(&mut self) {
        if self.bank_name_promise.is_waiting() {
            return;
        }
        self.bank_name_promise = self.bank.name().call_promise("");
    }

    fn check_idx(&self, pin_idx: u32) -> Result<(), Error> {
        let Some(pin_indices) = self.pin_indices() else {
            return Err(Error::Usage(
                "Cannot get a pin before valid pin indices are received".into(),
            ));
        };
        if !pin_indices.contains(&pin_idx) {
            return Err(Error::Usage(format!(
                "Pin index {} is not valid for this bank",
                pin_idx
            )));
        }
        Ok(())
    }
}

pub struct FlexPromiseRef<'i> {
    bank: &'i BankClient,
    pin_idx: u32,
}

pub struct FlexPromise {
    bank: BankClient,
    pin_idx: u32,
    mode: Promise<Mode>,
    mode_cached: Option<Mode>,
    set_mode: Promise<()>,
    output_level: Promise<Level>,
    output_level_cached: Option<Level>,
    set_output_level: Promise<()>,
    toggle: Promise<()>,
    input_level: Promise<Level>,
    input_level_cached: Option<Level>,
    speed: Promise<Speed>,
    speed_cached: Option<Speed>,
    set_speed: Promise<Result<(), ww_gpio::Error>>,
    pull: Promise<Pull>,
    pull_cached: Option<Pull>,
    set_pull: Promise<Result<(), ww_gpio::Error>>,
}

impl FlexPromiseRef<'_> {
    pub fn mode(&self) -> Promise<Mode> {
        self.bank.pin(self.pin_idx).mode().call_promise("")
    }

    pub fn output_level(&self) -> Promise<Level> {
        self.bank.pin(self.pin_idx).output_level().call_promise("")
    }

    pub fn input_level(&self) -> Promise<Level> {
        self.bank.pin(self.pin_idx).input_level().call_promise("")
    }

    pub fn speed(&self) -> Promise<Speed> {
        self.bank.pin(self.pin_idx).read_speed().read_promise("")
    }

    pub fn pull(&self) -> Promise<Pull> {
        self.bank.pin(self.pin_idx).read_pull().read_promise("")
    }
}

impl FlexPromise {
    fn new(bank: BankClient, pin_idx: u32) -> Self {
        Self {
            bank,
            pin_idx,
            mode: Default::default(),
            mode_cached: None,
            set_mode: Default::default(),
            output_level: Default::default(),
            output_level_cached: None,
            set_output_level: Default::default(),
            toggle: Default::default(),
            input_level: Default::default(),
            input_level_cached: None,
            speed: Default::default(),
            speed_cached: None,
            set_speed: Default::default(),
            pull: Default::default(),
            pull_cached: None,
            set_pull: Default::default(),
        }
    }

    pub fn mode(&self) -> Option<Mode> {
        self.mode_cached
    }

    pub fn mode_promise(&self) -> &Promise<Mode> {
        &self.mode
    }

    pub fn request_mode(&mut self) {
        if self.mode.is_waiting() {
            return;
        }
        self.mode = self.bank.pin(self.pin_idx).mode().call_promise("");
    }

    pub fn set_mode_promise(&self) -> &Promise<()> {
        &self.set_mode
    }

    pub fn output_level(&self) -> Option<Level> {
        self.output_level_cached
    }

    pub fn output_level_promise(&self) -> &Promise<Level> {
        &self.output_level
    }

    pub fn request_output_level(&mut self) {
        if self.output_level.is_waiting() {
            return;
        }
        self.output_level = self.bank.pin(self.pin_idx).output_level().call_promise("");
    }

    pub fn set_output_level(&mut self, level: Level) {
        self.set_output_level = self
            .bank
            .pin(self.pin_idx)
            .set_output_level(level)
            .call_promise("");
    }

    pub fn toggle(&mut self) {
        self.toggle = self.bank.pin(self.pin_idx).toggle().call_promise("");
    }

    pub fn set_output_level_state(&self) -> PromiseState<'_, ()> {
        self.set_output_level.state()
    }

    pub fn input_level(&self) -> Option<Level> {
        self.input_level_cached
    }

    pub fn input_level_promise(&self) -> &Promise<Level> {
        &self.input_level
    }

    pub fn request_input_level(&mut self) {
        if self.input_level.is_waiting() {
            return;
        }
        self.input_level = self.bank.pin(self.pin_idx).input_level().call_promise("");
    }

    pub fn speed(&self) -> Option<Speed> {
        self.speed_cached
    }

    pub fn speed_promise(&self) -> &Promise<Speed> {
        &self.speed
    }

    pub fn set_speed(&mut self, speed: Speed) {
        self.set_speed = self
            .bank
            .pin(self.pin_idx)
            .write_speed(speed)
            .write_promise("");
    }

    pub fn request_speed(&mut self) {
        if self.speed.is_waiting() {
            return;
        }
        self.speed = self.bank.pin(self.pin_idx).read_speed().read_promise("");
    }

    pub fn pull(&self) -> Option<Pull> {
        self.pull_cached
    }

    pub fn pull_promise(&self) -> &Promise<Pull> {
        &self.pull
    }

    pub fn set_pull(&mut self, pull: Pull) {
        self.set_pull = self
            .bank
            .pin(self.pin_idx)
            .write_pull(pull)
            .write_promise("")
    }

    pub fn request_pull(&mut self) {
        if self.pull.is_waiting() {
            return;
        }
        self.pull = self.bank.pin(self.pin_idx).read_pull().read_promise("");
    }

    pub fn request_all(&mut self) {
        self.request_mode();
        self.request_output_level();
        self.request_input_level();
        self.request_speed();
        self.request_pull();
    }

    pub fn sync_poll(&mut self) {
        self.mode.sync_poll();
        if let Some(mode) = self.mode.peek_done() {
            self.mode_cached = Some(*mode);
        }
        self.set_mode.sync_poll();
        self.output_level.sync_poll();
        if let Some(level) = self.output_level.peek_done() {
            self.output_level_cached = Some(*level);
        }
        self.set_output_level.sync_poll();
        self.toggle.sync_poll();
        self.input_level.sync_poll();
        if let Some(level) = self.input_level.peek_done() {
            self.input_level_cached = Some(*level);
        }
        self.speed.sync_poll();
        if let Some(speed) = self.speed.peek_done() {
            self.speed_cached = Some(*speed);
        }
        self.set_speed.sync_poll();
        self.pull.sync_poll();
        if let Some(pull) = self.pull.peek_done() {
            self.pull_cached = Some(*pull);
        }
        self.set_pull.sync_poll();
    }
}
