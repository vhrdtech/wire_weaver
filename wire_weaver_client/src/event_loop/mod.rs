use std::any::Any;

pub(crate) mod command;
pub(crate) mod commander;
pub(crate) mod core;
pub(crate) mod framing;
pub(crate) mod rx_dispatcher;

pub(crate) type DeviceHandle = Box<dyn Any + Send>;
