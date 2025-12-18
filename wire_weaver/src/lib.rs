#![cfg_attr(not(feature = "std"), no_std)]

mod test;

pub use shrink_wrap;
use shrink_wrap::prelude::ShrinkWrapError;
pub use wire_weaver_derive::{full_version, ww_api, ww_trait};
pub use ww_version;
use ww_version::FullVersion;

pub mod prelude {
    pub use shrink_wrap;
    pub use shrink_wrap::prelude::*;
    pub use wire_weaver_derive::{full_version, ww_api, ww_trait};
    pub use ww_version;
    pub use ww_version::FullVersion;
}

pub trait WireWeaverAsyncApiBackend {
    /// Deserialize request and process it.
    fn process_bytes<'a>(
        &mut self,
        data: &[u8],
        scratch_args: &'a mut [u8],
        scratch_event: &'a mut [u8],
        scratch_err: &'a mut [u8],
    ) -> impl Future<Output = Result<&'a [u8], ShrinkWrapError>>;

    fn send_updates(
        &mut self,
        sink: &mut impl MessageSink,
        scratch_value: &mut [u8],
        scratch_event: &mut [u8],
    ) -> impl Future<Output = ()> {
        let _ = sink;
        let (_, _) = (scratch_value, scratch_event);
        core::future::ready(())
    }

    /// Implemented version of an API. Return `<your_ww_api_crate>::DEVICE_API_ROOT_FULL_GID` from this method.
    fn version(&self) -> FullVersion<'_>;
}

pub trait MessageSink {
    fn send(&mut self, message: &[u8]) -> impl Future<Output = Result<(), ()>>;
}
