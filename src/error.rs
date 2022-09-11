use parser::span::Span;
use thiserror::Error;

#[derive(Debug)]
pub struct Error {
    pub kind: ErrorKind,
    pub span: Span,
}

#[derive(Error, Debug)]
pub enum ErrorKind {
    #[error("No serial number provided for a resource")]
    NoSerial,
    #[error("Const resource cannot be rw, wo, observe or stream")]
    ConstWithMods,
}