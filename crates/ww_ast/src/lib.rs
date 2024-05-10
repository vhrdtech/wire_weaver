pub mod item;
pub mod data;
pub mod ty;
pub mod file;
pub mod version;

pub use file::File;

// pub(crate) struct ConversionResult<T> {
//     pub(crate) warnings: Vec<>,
//     pub(crate) result: Result<T, Vec<>>
// }