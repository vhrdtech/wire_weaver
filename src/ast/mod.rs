use syn::{LitStr, LitInt, ExprStruct, Expr, ExprCall, ExprClosure, Path};

// #[derive(Debug)]
// pub enum Value {
//     Constant(Ty),
//     Variable(Ty),
//     Resource,
//     Expression(Box<Expression>),
// }

#[derive(Debug)]
pub struct Range {
    pub start: LitInt,
    pub end: LitInt,
}

#[derive(Debug)]
pub enum ResourceName {
    Plain(LitStr),
    //ArrayProduct(String, Array, String),
    RangeProduct(LitStr, Range, LitStr),
}

/// Represents bit field inside a Register or _.
/// Endianness is not applicable here, since bit field can't be used on it's own, see Register.
#[derive(Debug)]
pub struct BitField {
    /// Base type, one of: u8, u16, u32, u64, u128.
    pub base_ty: Path,
    /// Is writing to this bit field allowed
    pub access_mode: AccessMode,
    /// Starting from bit, inclusive.
    /// Must be >= that `to`.
    /// If from == to, only one bit is represented (flag).
    pub from: u32,
    /// Ending at bit, inclusive.
    pub to: u32,
    /// Example values for enum fields code generation, documentation and UI.
    /// The only values that are allowed, if `is_valid` == `Some(OnlyListedValues)`.
    pub values: Vec<ExprStruct>,
    /// Validity check mode
    pub is_valid: Option<ValidityCheck>
}

#[derive(Debug)]
pub enum RegisterAddress {
    Plain(u64),
    Custom(ExprStruct)
}

#[derive(Debug)]
pub enum Endianness {
    Little,
    Big
}

#[derive(Debug)]
pub enum AccessMode {
    ReadOnly,
    WriteOnly,
    ReadWrite
}

#[derive(Debug)]
pub enum ValidityCheck {
    OnlyListedValues,
    Call(ExprCall),
    Closure(ExprClosure)
}

/// Represents hardware register accessed by it's `address`.
/// `address` can be a simple number (periphery register), or it can be a structure with arbitrary
/// fields (SPI accessed register).
/// Code generators for a particular interface decide what to do with `address`.
#[derive(Debug)]
pub struct Register {
    pub address: RegisterAddress,
    pub endianness: Endianness,
    pub default: Option<Expr>,
    pub description: Option<LitStr>,
    pub values: Vec<ExprStruct>,
    pub is_valid: Option<ValidityCheck>,
    pub bits: Vec<BitField>
}

#[derive(Debug)]
pub enum Type {
    /// Hardware register
    Register(Register),

}

#[derive(Debug)]
pub enum ResourceKind {
    // set/get/subscribe(sugar on stream?), default, allowed, values
    // underlying type is required, maybe not known right away, but can be derived from bits
    Property(Type),
    //Function, // fn(args) -> value
    //Stream(Type), // value,value,value... subscribe, unsubscribe, backpressure, bandwith limit
    //User      // everything else
}

#[derive(Debug)]
pub struct Resource {
    pub id: Option<u32>, // required, if not provided, it can be auto assigned through vhl-cli
    pub name: ResourceName,
    pub children: Vec<Resource>,
    pub kind: Option<ResourceKind>,
}
