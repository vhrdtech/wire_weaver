#![no_std]

pub mod framed;
pub mod framed_rx;
pub mod traits;
pub mod tx;

pub use framed_rx::FramedRx;
pub use tx::Tx;

#[cfg(all(feature = "very_large", not(feature = "large")))]
compile_error!("Doesn't make sense having 'very_large' feature enabled, but not 'large'");
