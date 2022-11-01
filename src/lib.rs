pub mod error;
pub mod user_error;
pub mod warning;
mod passes;
pub mod transform;
pub mod project;

pub use error::Error;
pub use user_error::{UserError, UserErrorKind};
pub use warning::{Warning, WarningKind};
// pub fn process(file: &mut ast::File) {
//     transform::transform(file);
// }