use thiserror::Error;

#[derive(Error, Debug)]
pub enum CodegenError {
    #[error("Error originating in core module")]
    Core(#[from] vhl::error::Error)
}
