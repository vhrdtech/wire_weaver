use wire_weaver::prelude::*;
use ww_numeric::{NumericAnyType, NumericBaseType};
use ww_version::{CompactVersion, FullVersion};

#[derive_shrink_wrap]
#[ww_repr(unib32)]
#[derive(Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Type<'i> {
    /// 1-bit, alignment of one-bit, same as `UB(UBits(1))` but serialized with only 1 nibble because bool is used very often.
    Bool,

    NumericBase(NumericBaseType),
    NumericAny(NumericAnyType<'i>),

    // Reserved Type with discriminant length of 1 nibble
    // Reserved6,
    // Reserved Type with discriminant length of 1 nibble
    // Reserved7,
    /// Variable length Unicode string
    String,
    /// Variable length `Vec<T>`
    Vec(RefBox<'i, Type<'i>>),
    /// Fixed size array `[T; len]`
    Array {
        len: u32,
        ty: RefBox<'i, Type<'i>>,
    },
    /// Variable length tuple `(T1, T2, ...)`
    Tuple(RefVec<'i, Type<'i>>),
    /// User defined struct
    Struct(ItemStruct<'i>),
    /// User defined enum
    Enum,
    /// Flag followed by Optional `T` if true and nothing otherwise.
    Option {
        /// If true then flag is popped from the stack; otherwise it is read from the buffer
        is_flag_on_stack: bool,
        some_ty: RefBox<'i, Type<'i>>,
    },
    /// Flag followed by `T` if flag is true and `E` otherwise.
    Result {
        /// If true then flag is popped from the stack; otherwise it is read from the buffer
        is_flag_on_stack: bool,
        ok_ty: RefBox<'i, Type<'i>>,
        err_ty: RefBox<'i, Type<'i>>,
    },
    /// Read bool and put it onto "flag stack".
    /// When serializing: must do the reverse operation for all Options and Results that have is_flag_on_stack set to true.
    Flag,

    OutOfLineGlobalTextual {
        full_version: FullVersion<'i>,
    },
    OutOfLineGlobalNumeric {
        global_version: Option<CompactVersion>,
    },
}

#[derive_shrink_wrap]
#[ww_repr(unib32)]
#[derive(Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Value<'i> {
    Bool(bool),
    Numeric(NumericBaseType),
    String(&'i str),
    // TODO: the rest
}

#[derive_shrink_wrap]
#[derive(Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct ItemStruct<'i> {
    pub size: ElementSize,
    pub ident: &'i str,
    pub fields: RefVec<'i, Field<'i>>,
}

#[derive_shrink_wrap]
#[derive(Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Field<'i> {
    pub ident: &'i str,
    pub ty: RefBox<'i, Type<'i>>,
    pub default: Option<Value<'i>>,
}

pub struct Api {}
