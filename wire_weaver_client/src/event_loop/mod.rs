use std::any::Any;

pub(crate) mod command;
pub(crate) mod commander;
pub(crate) mod core;
pub(crate) mod rx_dispatcher;
#[cfg_attr(not(feature = "rtt"), allow(dead_code))]
pub(crate) mod stream;
pub(crate) mod transport;

#[cfg(test)]
mod device_e2e_tests;

pub(crate) type DeviceHandle = Box<dyn Any + Send>;
