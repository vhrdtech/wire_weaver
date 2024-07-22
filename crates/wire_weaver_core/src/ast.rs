use strum_macros::EnumString;

use ident::Ident;
use path::Path;
use value::Value;

pub mod ident;
pub mod path;
pub mod value;

#[derive(Debug)]
pub struct Context {
    pub modules: Vec<Module>,
}

#[derive(Debug)]
pub struct Module {
    // docs
    pub source: Source,
    pub version: Version,
    pub items: Vec<Item>,
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Source {
    File {
        /// Path relative to project root.
        /// Project root itself is known to executable through other mechanism.
        path: String,
    },
    Registry {
        collection: String,
        version: Version,
    },
    Git {
        url: String,
        sha: String,
    },
}

#[derive(Debug)]
pub enum Item {
    Struct(ItemStruct),
    Enum(ItemEnum),
    // api
}

#[derive(Debug)]
pub struct ItemStruct {
    pub is_final: bool,
    pub ident: Ident,
    pub fields: Vec<Field>,
}

#[derive(Debug)]
pub struct ItemEnum {
    pub is_final: bool,
    pub repr: Repr,
    pub ident: Ident,
    pub variants: Vec<Variant>,
}

#[derive(Copy, Clone, Debug, Default, EnumString)]
pub enum Repr {
    #[strum(serialize = "u4")]
    U4,
    #[strum(serialize = "u8")]
    U8,
    #[strum(serialize = "u16")]
    U16,
    #[strum(serialize = "nib16")]
    #[default]
    Nib16,
    #[strum(serialize = "u32")]
    U32,
}

#[derive(Debug)]
pub struct Variant {
    // attrs
    pub ident: Ident,
    pub fields: Fields,
    pub discriminant: u32,
    pub since: Option<Version>,
}

#[derive(Debug)]
pub struct Field {
    pub id: u32,
    pub ident: Ident,
    pub ty: Type,
    pub since: Option<Version>,
    pub default: Option<Value>,
}

#[derive(Debug)]
pub enum Type {
    Bool,

    U4,
    U8,
    U16,
    U32,
    U64,
    U128,

    Nib16,
    ULeb32,
    ULeb64,
    ULeb128,

    I4,
    I8,
    I16,
    I32,
    I64,
    I128,

    ILeb32,
    ILeb64,
    ILeb128,

    F32,
    F64,

    Bytes,
    String,

    Array(usize, Layout),
    Tuple(Vec<Type>),
    Vec(Layout),

    /// All types going through generic read<T: DeserializeShrinkWrap>(&T) and write<T: SerializeShrinkWrap>(&T)
    User(UserLayout),

    // Only relevant for fields with type Option<T>. Vec<Option<T>> handles flags differently.
    IsSome,
    // Only relevant for fields with type Result<T, E>. Vec<Result<T, E>> handles flags differently.
    IsOk,
}

#[derive(Debug)]
pub enum Fields {
    Named(Vec<Field>),
    Unnamed(Vec<Type>),
    Unit,
}

#[derive(Debug)]
pub enum Layout {
    Builtin(Box<Type>),
    // Skip reading data if previously read flag is false.
    Option(Box<Type>),
    // Read T or E depending on previously read flag.
    Result(Box<(Type, Type)>),
    // User defined, size unknown.
    // On read: BufReader size will be limited to the one read from the back, unread bytes will be skipped.
    // On write: size will be written to the back of the buffer.
    // Type name is used only for dynamic ser/des operations.
    Unsized(Path),
    // User defined, size known and will not be read/written.
    // BufReader size will be limited to the provided number of bytes, unread bytes will be skipped.
    Sized(Path, u32),
}

#[derive(Debug)]
pub enum UserLayout {
    Unsized(Path),
    Sized(Path, u32),
}

impl UserLayout {
    pub fn path(&self) -> &Path {
        match self {
            UserLayout::Unsized(path) => path,
            UserLayout::Sized(path, _) => path,
        }
    }
}
