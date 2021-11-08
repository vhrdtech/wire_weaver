use std::collections::{HashMap, HashSet};
use crate::lexer::token::IdentKind;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum IntTy {
    I8,
    I16,
    I32,
    I64,
    I128,
    U8,
    U16,
    U32,
    U64,
    U128,
    In(u8),
    Un(u8),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FloatTy {
    F16,
    F32,
    F64,
    Q(u8, u8), // "Q" notation
    UQ(u8, u8),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StrTy {
    C,
    UTF8,
    UTF16,
    UTF32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StrStyle {
    Cooked,
    Raw(u16)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CharTy {
    C,
    Unicode,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SeqTy {
    Tuple(Vec<Ty>),
    Array(Box<Ty>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum UserTy {
    Struct(HashMap<IdentKind, Ty>),
    Enum(HashSet<IdentKind>),
    Union(HashMap<IdentKind, Ty>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Ty {
    Int(IntTy),
    Float(FloatTy),
    Byte,
    ByteStr,
    Str(StrTy),
    Bool,
    Char(CharTy),
    Seq(SeqTy),
    User(UserTy),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Lit {
    Int(u128, IntTy),
    Float(u64, FloatTy),
    Byte(u8),
    ByteStr(Vec<u8>),
    Str(String, StrTy, StrStyle),
    Bool(bool),
    Char(char, CharTy),
    Err
}

/// Literal, that can be used in ranges
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RangeLit {
    Int(u128, IntTy),
    Char(char, CharTy),
}

pub type StrLit = (String, StrTy, StrStyle);