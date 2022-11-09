pub mod error;
mod passes;
pub mod project;
pub mod transform;
pub mod user_error;
pub mod warning;

pub use error::Error;
pub use user_error::{UserError, UserErrorKind};
pub use warning::{Warning, WarningKind};
// pub fn process(file: &mut ast::File) {
//     transform::transform(file);
// }
