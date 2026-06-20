#![no_std]

mod gid;

pub use gid::GlobalTypeId;

pub const WW_LOG_BARE_METAL: GlobalTypeId = GlobalTypeId::new(0);
pub const WIRE_WEAVER_USB_LINK: GlobalTypeId = GlobalTypeId::new(512);
pub const WW_CLIENT_SERVER: GlobalTypeId = GlobalTypeId::new(513);
