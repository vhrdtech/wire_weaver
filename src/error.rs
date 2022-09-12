use parser::span::Span;
use thiserror::Error;

#[derive(Debug)]
pub struct Error {
    pub kind: ErrorKind,
    pub span: Span,
}

impl Error {
    pub fn new(kind: ErrorKind, span: Span) -> Self {
        Error { kind, span }
    }
}

#[derive(Error, Debug)]
pub enum ErrorKind {
    #[error("No serial number provided for a resource")]
    NoSerial,
    #[error("Const resource cannot be rw, wo, observe or stream")]
    ConstWithMods,
    #[error("Method resource cannot be const, ro, rw, wo, observe or stream")]
    FnWithMods,
    #[error("Cell holding const or ro resource is redundant")]
    CellWithConstRo,
    #[error("Write only resource cannot be observable")]
    WoObserve,
    #[error("Cell holding ro+stream is redundant, multiple nodes can subscribe to the same screen")]
    CellWithRoStream,
}