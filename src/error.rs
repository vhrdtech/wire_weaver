#[derive(Debug,)]
pub enum LoaderError {
    IoError(std::io::Error)
}

impl From<std::io::Error> for LoaderError {
    fn from(e: std::io::Error) -> Self {
        LoaderError::IoError(e)
    }
}

impl From<LoaderError> for Error {
    fn from(e: LoaderError) -> Self {
        Error::LoaderError(e)
    }
}

#[derive(Debug,)]
pub enum Error {
    LoaderError(LoaderError),
}

pub type Result<T> = std::result::Result<T, Error>;

