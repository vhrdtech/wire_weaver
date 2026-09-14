#![no_std]

#[cfg(feature = "std")]
extern crate alloc;

pub mod crc;
pub mod framed;
pub mod framed_rx;
#[cfg(feature = "std")]
pub mod owned;
mod tests;
pub mod traits;
pub mod tx;

pub use framed_rx::FramedRx;
#[cfg(feature = "std")]
pub use owned::{FramedRxOwned, TxOwned};
pub use tx::Tx;

#[cfg(all(feature = "very_large", not(feature = "large")))]
compile_error!("Doesn't make sense having 'very_large' feature enabled, but not 'large'");
