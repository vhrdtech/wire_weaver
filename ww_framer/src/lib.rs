#![no_std]

pub mod framed;
pub mod rx;
pub mod traits;
pub mod tx;

pub use rx::Rx;
pub use tx::Tx;

#[cfg(all(feature = "very_large", not(feature = "large")))]
compile_error!("Doesn't make sense having 'very_large' feature enabled, but not 'large'");
