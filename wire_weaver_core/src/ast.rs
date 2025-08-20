use path::Path;
use proc_macro2::{Ident, Span, TokenStream};
use quote::{ToTokens, quote};
use shrink_wrap::ElementSize;
use std::fmt::Debug;
use syn::LitStr;
use value::Value;

pub mod api;
pub mod path;
// pub mod subtype;
pub mod trait_macro_args;
pub mod value;

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

#[derive(Debug)]
pub enum Item {
    Struct(ItemStruct),
    Enum(ItemEnum),
    // Const(ItemConst),
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
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
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
    ILeb32,
    ILeb64,
    ILeb128,

    F32,
    F64,

    // Bytes,
    String,

    Array(usize, Box<Type>),
    Tuple(Vec<Type>),
    Vec(Box<Type>),

    // User defined type
    // TODO: use enum structs instead of tuples
    External(Path, bool),
    // User defined, size is known and fixed, or deterministic (depends on enum discriminant) and will not be read/written.
    // Sized(Path, bool),

    // is_some_flag, optional_ty
    Option(Ident, Box<Type>),
    // Only used for relocation of is_some flag in structs and enum struct variants.
    IsSome(Ident),

    // is_ok_flag, (ok_ty, err_ty)
    Result(Ident, Box<(Type, Type)>),
    // Only used for relocation of is_ok flag in structs and enum struct variants.
    IsOk(Ident),

    RefBox(Box<Type>),
}

#[derive(Clone, Debug)]
pub enum Fields {
    Named(Vec<Field>),
    Unnamed(Vec<Type>),
    Unit,
}

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
}

impl ToTokens for Docs {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        for doc in &self.docs {
            tokens.extend(quote!(#[doc = #doc]));
        }
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
        owned.ident = Ident::new(format!("{}Owned", self.ident).as_str(), self.ident.span());
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
        owned.ident = Ident::new(format!("{}Owned", self.ident).as_str(), self.ident.span());
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
            Type::String | Type::Vec(_) | Type::RefBox(_) => true,
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
            Type::Array(_, ty) => ty.potential_lifetimes(),
            Type::External(_, potential_lifetimes) => *potential_lifetimes,
            // Type::Sized(_, potential_lifetimes) => *potential_lifetimes,
            _ => false,
        }
    }

    pub fn make_owned(&mut self) {
        match self {
            Type::External(path, potential_lifetimes) => {
                // Type::Unsized(path, potential_lifetimes) | Type::Sized(path, potential_lifetimes) => {
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

    pub fn visit_external_types<F: FnMut(&Path, bool)>(&self, f: &mut F) {
        match self {
            Type::External(path, potential_lifetimes) => {
                f(path, *potential_lifetimes);
            }
            Type::Option(_, some_ty) => {
                some_ty.visit_external_types(f);
            }
            Type::Result(_, ok_err_ty) => {
                let (ok_ty, err_ty) = &**ok_err_ty;
                ok_ty.visit_external_types(f);
                err_ty.visit_external_types(f);
            }
            Type::Array(_, ty) => {
                ty.visit_external_types(f);
            }
            Type::Tuple(types) => {
                for ty in types {
                    ty.visit_external_types(f);
                }
            }
            Type::Vec(ty) => {
                ty.visit_external_types(f);
            }
            Type::RefBox(ty) => {
                ty.visit_external_types(f);
            }
            _ => {}
        }
    }
}

impl Field {
    pub fn new(id: u32, ident: &str, ty: Type) -> Self {
        Self {
            docs: Docs::empty(),
            id,
            ident: Ident::new(ident, Span::call_site()),
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
