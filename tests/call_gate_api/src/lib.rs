//! Test API of the server's call gate (SRV-50), used by `tests/call_gate`: a global trait the gate admits while the
//! device is guarded (`ww_user_info::UserInfo`), project-local items it refuses or admits by item, a stream, a sink,
//! an observed property, a resource array and a multi call.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;

#[ww_api_root]
pub trait CallGateTest {
    ww_impl!(user: ww_user_info::UserInfo);
    ww_impl!(motor: Motor);
    ww_impl!(channel[]: Channel);
}

#[ww_trait]
pub trait Motor {
    /// Starts the motor: refused while guarded
    fn start();
    /// Stops the motor: admitted while guarded, by item
    fn stop();
    property!(rw enabled: bool);
    property!(ro+observe running: bool);
    stream!(events: MotorEvent);
    sink!(commands: MotorCommand);
}

#[ww_trait]
pub trait Channel {
    /// Pulses the channel's output once
    fn pulse();
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone, Copy), ww_repr = u4)]
pub enum MotorEvent {
    Started,
    Stopped,
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone, Copy), ww_repr = u4)]
pub enum MotorCommand {
    Start,
    Stop,
}
