use ident::Ident;
use path::Path;
use proc_macro2::{Span, TokenStream};
use quote::quote;
use shrink_wrap::ElementSize;
use std::fmt::{Debug, Formatter};
use syn::LitStr;
use value::Value;

use crate::ast::api::ApiLevel;

pub mod api;
pub mod ident;
pub mod path;
// pub mod subtype;
pub mod value;

#[derive(Debug)]
pub struct Context {
    pub modules: Vec<Module>,
}

#[derive(Debug)]
pub struct Module {
    pub docs: Docs,
    pub source: Source,
    pub version: Version,
    pub items: Vec<Item>,
    pub api_levels: Vec<ApiLevel>,
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub enum Source {
    ShrinkWrapDerive,
    File {
        /// Path relative to project root.
        /// Project root itself is known to executable through other mechanism.
        path: String,
    },
    String(String),
    Registry {
        collection: String,
        version: Version,
    },
    Git {
        url: String,
        sha: String,
    },
}

impl Debug for Source {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Source::ShrinkWrapDerive => write!(f, "ShrinkWrapDerive"),
            Source::File { path } => write!(f, "Source::File(path={})", path),
            Source::String(src) => write!(f, "Source::String({src})"),
            Source::Registry { .. } => unimplemented!(),
            Source::Git { .. } => unimplemented!(),
        }
    }
}

#[derive(Debug)]
pub enum Item {
    Struct(ItemStruct),
    Enum(ItemEnum),
    Const(ItemConst),
}

#[derive(Clone, Debug)]
pub struct ItemStruct {
    pub docs: Docs,
    pub derive: Vec<Path>,
    pub size_assumption: Option<ElementSize>,
    pub ident: Ident,
    pub fields: Vec<Field>,
    pub cfg: Option<LitStr>,
}

#[derive(Clone, Debug)]
pub struct ItemEnum {
    pub docs: Docs,
    pub derive: Vec<Path>,
    pub size_assumption: Option<ElementSize>,
    pub repr: Repr,
    pub explicit_ww_repr: bool,
    pub ident: Ident,
    pub variants: Vec<Variant>,
    pub cfg: Option<LitStr>,
}

#[derive(Debug)]
pub struct ItemConst {
    pub docs: Docs,
    pub ident: Ident,
    pub ty: Type,
    pub value: syn::Expr,
}

#[derive(Copy, Clone, Debug, Default)]
pub enum Repr {
    U(u8),
    #[default]
    UNib32,
}

#[derive(Clone, Debug)]
pub struct Variant {
    pub docs: Docs,
    pub ident: Ident,
    pub fields: Fields,
    pub discriminant: u32,
    pub since: Option<Version>,
}

#[derive(Clone, Debug)]
pub struct Field {
    pub docs: Docs,
    pub id: u32,
    pub ident: Ident,
    pub ty: Type,
    pub since: Option<Version>,
    pub default: Option<Value>,
}

// TODO: Convert to struct and add span
#[derive(Clone, Debug)]
pub enum Type {
    Bool,

    U4,
    U8,
    U16,
    U32,
    U64,
    U128,

    UNib32,
    ULeb32,
    ULeb64,
    ULeb128,

    I4,
    I8,
    I16,
    I32,
    I64,
    I128,
    // TODO: U2, I2, ... as separate variants
    // TODO: DateTime, Version as separate variants
    ILeb32,
    ILeb64,
    ILeb128,

    F32,
    F64,

    // Bytes,
    String,

    Array(usize, Layout),
    Tuple(Vec<Type>),
    Vec(Layout),

    // User defined, size unknown.
    // On read: BufReader size will be limited to the one read from the back, unread bytes will be skipped.
    // TODO: use enum structs instead of tuples
    Unsized(Path, bool),
    // User defined, size is known and fixed, or deterministic (depends on enum discriminant) and will not be read/written.
    Sized(Path, bool),

    // is_some_flag, optional_ty
    Option(Ident, Box<Type>),
    // Only relevant for fields with type Option<T>. Vec<Option<T>> handles flags differently.
    IsSome(Ident),

    // is_ok_flag, (ok_ty, err_ty)
    Result(Ident, Box<(Type, Type)>),
    // Only relevant for fields with type Result<T, E>. Vec<Result<T, E>> handles flags differently.
    IsOk(Ident),
}

#[derive(Clone, Debug)]
pub enum Fields {
    Named(Vec<Field>),
    Unnamed(Vec<Type>),
    Unit,
}

#[derive(Clone, Debug)]
pub enum Layout {
    Builtin(Box<Type>),
    // Skip reading data if previously read flag is false.
    Option(Box<Type>),
    // Read T or E depending on previously read flag.
    Result(Box<(Type, Type)>),
}

// #[derive(Debug)]
// pub enum UserLayout {
//     Unsized(Path),
//     Sized(Path, u32),
// }
//
// impl UserLayout {
//     pub fn path(&self) -> &Path {
//         match self {
//             UserLayout::Unsized(path) => path,
//             UserLayout::Sized(path, _) => path,
//         }
//     }
// }

#[derive(Clone, Debug)]
pub struct Docs {
    docs: Vec<LitStr>,
}

impl Docs {
    pub fn empty() -> Docs {
        Docs { docs: Vec::new() }
    }

    pub fn push(&mut self, s: LitStr) {
        self.docs.push(s);
    }

    pub fn push_str(&mut self, s: impl AsRef<str>) {
        self.docs.push(LitStr::new(s.as_ref(), Span::call_site()));
    }

    pub fn ts(&self) -> TokenStream {
        let mut ts = TokenStream::new();
        for doc in &self.docs {
            ts.extend(quote!(#[doc = #doc]));
        }
        ts
    }
}

impl ItemStruct {
    pub fn potential_lifetimes(&self) -> bool {
        for field in &self.fields {
            if field.ty.potential_lifetimes() {
                return true;
            }
        }
        false
    }

    pub fn to_owned(&self, feature: LitStr) -> Self {
        let mut owned = self.clone();
        owned.ident = Ident::new(format!("{}Owned", self.ident.sym));
        owned.cfg = Some(feature);
        for f in &mut owned.fields {
            f.ty.make_owned();
        }
        owned
    }

    pub fn cfg(&self) -> TokenStream {
        if let Some(feature) = &self.cfg {
            quote! { #[cfg(feature = #feature)] }
        } else {
            quote! {}
        }
    }
}

impl ItemEnum {
    pub fn potential_lifetimes(&self) -> bool {
        for variant in &self.variants {
            if variant.potential_lifetimes() {
                return true;
            }
        }
        false
    }

    pub fn to_owned(&self, feature: LitStr) -> Self {
        let mut owned = self.clone();
        owned.ident = Ident::new(format!("{}Owned", self.ident.sym));
        owned.cfg = Some(feature);
        for v in &mut owned.variants {
            match &mut v.fields {
                Fields::Named(named) => {
                    for f in named {
                        f.ty.make_owned();
                    }
                }
                Fields::Unnamed(unnamed) => {
                    for f in unnamed {
                        f.make_owned();
                    }
                }
                Fields::Unit => {}
            }
        }
        owned
    }

    pub fn cfg(&self) -> TokenStream {
        if let Some(feature) = &self.cfg {
            quote! { #[cfg(feature = #feature)] }
        } else {
            quote! {}
        }
    }
}

impl Variant {
    pub fn potential_lifetimes(&self) -> bool {
        match &self.fields {
            Fields::Named(fields) => {
                for field in fields {
                    if field.ty.potential_lifetimes() {
                        return true;
                    }
                }
            }
            Fields::Unnamed(types) => {
                for ty in types {
                    if ty.potential_lifetimes() {
                        return true;
                    }
                }
            }
            Fields::Unit => {}
        }
        false
    }
}

impl Type {
    pub fn potential_lifetimes(&self) -> bool {
        match self {
            Type::String | Type::Vec(_) => true,
            Type::Result(_, ok_err_ty) => {
                ok_err_ty.0.potential_lifetimes() || ok_err_ty.1.potential_lifetimes()
            }
            Type::Option(_, some_ty) => some_ty.potential_lifetimes(),
            Type::Tuple(types) => {
                for ty in types {
                    if ty.potential_lifetimes() {
                        return true;
                    }
                }
                false
            }
            Type::Array(_, layout) => match layout {
                Layout::Builtin(ty) => ty.potential_lifetimes(),
                Layout::Option(ty) => ty.potential_lifetimes(),
                Layout::Result(ok_err_ty) => {
                    ok_err_ty.0.potential_lifetimes() || ok_err_ty.1.potential_lifetimes()
                }
            },
            Type::Unsized(_, potential_lifetimes) => *potential_lifetimes,
            Type::Sized(_, potential_lifetimes) => *potential_lifetimes,
            _ => false,
        }
    }

    pub fn make_owned(&mut self) {
        match self {
            Type::Unsized(path, potential_lifetimes) | Type::Sized(path, potential_lifetimes) => {
                if *potential_lifetimes {
                    path.make_owned();
                    *potential_lifetimes = false;
                }
            }
            Type::Option(_, some_ty) => some_ty.make_owned(),
            Type::Result(_, ok_err_ty) => {
                ok_err_ty.0.make_owned();
                ok_err_ty.1.make_owned();
            }
            Type::Array(_, layout) => {
                layout.make_owned();
            }
            Type::Tuple(types) => {
                for ty in types {
                    ty.make_owned();
                }
            }
            Type::Vec(layout) => {
                layout.make_owned();
            }
            _ => {}
        }
    }
}

impl Layout {
    pub fn make_owned(&mut self) {
        match self {
            Layout::Builtin(ty) => ty.make_owned(),
            Layout::Option(ty) => ty.make_owned(),
            Layout::Result(ok_err_ty) => {
                ok_err_ty.0.make_owned();
                ok_err_ty.1.make_owned();
            }
        }
    }
}

impl Field {
    pub fn new(id: u32, ident: &str, ty: Type) -> Self {
        Self {
            docs: Docs::empty(),
            id,
            ident: Ident::new(ident),
            ty,
            since: None,
            default: None,
        }
    }
}

impl Repr {
    pub fn parse_str(s: &str) -> Option<Self> {
        if s == "unib32" {
            return Some(Repr::UNib32);
        }
        let s = s.strip_prefix("u")?;
        let bits: u8 = s.parse().ok()?;
        Some(Repr::U(bits))
    }

    pub fn max_discriminant(&self) -> u32 {
        match self {
            Repr::U(bits) => {
                if *bits == 32 {
                    u32::MAX
                } else {
                    2u32.pow(*bits as u32) - 1
                }
            }
            Repr::UNib32 => u32::MAX,
        }
    }

    pub fn required_bits(&self) -> u8 {
        match self {
            Repr::U(bits) => *bits,
            Repr::UNib32 => 32,
        }
    }

    pub fn std_bits(&self) -> u8 {
        match &self {
            Repr::U(4) => 8,
            Repr::U(8) => 8,
            Repr::U(16) => 16,
            Repr::U(32) => 32,
            Repr::UNib32 => 32,
            Repr::U(bits) if *bits < 8 => 8,
            Repr::U(bits) if *bits < 16 => 16,
            Repr::U(bits) if *bits < 32 => 32,
            u => unimplemented!("discriminant_type {:?}", u),
        }
    }
}
