use std::any::Any;

pub(crate) mod command;
pub(crate) mod commander;
pub(crate) mod event_loop_state;
pub(crate) mod rx_dispatcher;

pub(crate) type DeviceHandle = Box<dyn Any + Send>;
