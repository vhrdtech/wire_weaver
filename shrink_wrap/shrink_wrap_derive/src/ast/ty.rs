use crate::ast::object_size::ObjectSize;
use crate::ast::path::Path;
use proc_macro2::{Ident, Span, TokenStream};
use quote::{ToTokens, quote};
use syn::{Expr, LitInt};

/// Length of a `[T; N]` field: an integer literal the macro can do math with, or any other const expression
/// (`MAX_SERIES`, `N * 2`, `{ .. }`) emitted as written.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ArrayLen {
    Lit(usize),
    Expr(Expr),
}

impl ToTokens for ArrayLen {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match self {
            ArrayLen::Lit(len) => {
                LitInt::new(format!("{len}").as_str(), Span::call_site()).to_tokens(tokens)
            }
            ArrayLen::Expr(expr) => expr.to_tokens(tokens),
        }
    }
}

// TODO: Convert to struct and add span
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Type {
    Bool,

    Nibble,
    U8,
    U16,
    U32,
    U64,
    U128,

    UNib32,
    UVlq32,
    ULeb32,
    ULeb64,
    ULeb128,

    // TODO: remove add UB, IB
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

    Array(ArrayLen, Box<Type>),
    Tuple(Vec<Type>),
    Vec(Box<Type>),
    Range(Box<Type>),
    RangeInclusive(Box<Type>),

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

    /// `TailSize<N>`, the size of the rest of the enclosing value, backfilled; `None` is the default width.
    TailSize(Option<Expr>),
}

impl Type {
    pub(crate) fn make_owned(&mut self) {
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
            Type::RefBox(ref_box) => {
                ref_box.make_owned();
            }
            _ => {}
        }
    }

    /// Whether `pred` holds for this type or any type nested in it.
    pub(crate) fn any(&self, pred: &dyn Fn(&Type) -> bool) -> bool {
        if pred(self) {
            return true;
        }
        match self {
            Type::Array(_, ty)
            | Type::Vec(ty)
            | Type::Range(ty)
            | Type::RangeInclusive(ty)
            | Type::Option(_, ty)
            | Type::RefBox(ty) => ty.any(pred),
            Type::Tuple(types) => types.iter().any(|ty| ty.any(pred)),
            Type::Result(_, ok_err_ty) => ok_err_ty.0.any(pred) || ok_err_ty.1.any(pred),
            _ => false,
        }
    }

    /// Whether this type is or contains `UVlq32Backfill`, recognized by name only.
    pub(crate) fn contains_backfill(&self) -> bool {
        self.contains_external(&["UVlq32Backfill"])
    }

    /// Whether this type is or contains `TailBytes` or `TailBytesOwned`, recognized by name only.
    pub(crate) fn contains_tail_bytes(&self) -> bool {
        self.contains_external(&["TailBytes", "TailBytesOwned"])
    }

    /// Whether this type is or contains `TailSize<N>`.
    pub(crate) fn contains_tail_size(&self) -> bool {
        self.any(&|ty| matches!(ty, Type::TailSize(_)))
    }

    /// Whether this type is or contains an external type whose last path segment is one of `names`.
    fn contains_external(&self, names: &[&str]) -> bool {
        self.any(&|ty| match ty {
            Type::External(path, _) => path
                .segments
                .last()
                .is_some_and(|ident| names.iter().any(|name| ident == name)),
            _ => false,
        })
    }

    /// Every user type (`Type::External`) in this type, nested ones included, with whether it takes a lifetime.
    pub(crate) fn externals(&self, out: &mut Vec<(Path, bool)>) {
        match self {
            Type::External(path, is_lifetime) => out.push((path.clone(), *is_lifetime)),
            Type::Array(_, ty)
            | Type::Vec(ty)
            | Type::Range(ty)
            | Type::RangeInclusive(ty)
            | Type::Option(_, ty)
            | Type::RefBox(ty) => ty.externals(out),
            Type::Tuple(types) => {
                for ty in types {
                    ty.externals(out);
                }
            }
            Type::Result(_, ok_err_ty) => {
                ok_err_ty.0.externals(out);
                ok_err_ty.1.externals(out);
            }
            _ => {}
        }
    }

    /// Return ElementSize if it is known. None is returned for Unsized.
    pub(crate) fn element_size(&self) -> Option<ObjectSize> {
        let size_bits = match self {
            Type::Bool => 1,
            Type::Nibble => 4,
            Type::U8 => 8,
            Type::U16 => 16,
            Type::U32 => 32,
            Type::U64 => 64,
            Type::U128 => 128,
            Type::UNib32 => return Some(ObjectSize::SelfDescribing),
            Type::UVlq32 => return Some(ObjectSize::SelfDescribing),
            Type::ULeb32 => return Some(ObjectSize::SelfDescribing),
            Type::ULeb64 => return Some(ObjectSize::SelfDescribing),
            Type::ULeb128 => return Some(ObjectSize::SelfDescribing),
            Type::I8 => 8,
            Type::I16 => 16,
            Type::I32 => 32,
            Type::I64 => 64,
            Type::I128 => 128,
            Type::ILeb32 => return Some(ObjectSize::SelfDescribing),
            Type::ILeb64 => return Some(ObjectSize::SelfDescribing),
            Type::ILeb128 => return Some(ObjectSize::SelfDescribing),
            Type::F32 => 32,
            Type::F64 => 64,
            Type::String => return Some(ObjectSize::Unsized),
            Type::Array(len, ty) => {
                let size = match ty.element_size()? {
                    ObjectSize::Unsized => ObjectSize::Unsized,
                    ObjectSize::UnsizedFinalStructure => ObjectSize::UnsizedFinalStructure,
                    ObjectSize::SelfDescribing => ObjectSize::SelfDescribing,
                    ObjectSize::Sized {
                        size_bits,
                        symbolic,
                    } => match len {
                        ArrayLen::Lit(len) => ObjectSize::Sized {
                            size_bits: len * size_bits,
                            symbolic: symbolic
                                .iter()
                                .map(|term| quote! { #len * (#term) })
                                .collect(),
                        },
                        ArrayLen::Expr(len) => ObjectSize::sized_symbolic(
                            quote! { (#len) * (#size_bits #(+ #symbolic)*) },
                        ),
                    },
                };
                return Some(size);
            }
            Type::Tuple(types) => {
                let mut sum = ObjectSize::sized(0);
                for ty in types {
                    sum = sum.add(ty.element_size()?);
                }
                return Some(sum);
            }
            Type::Vec(_) => return Some(ObjectSize::UnsizedFinalStructure),
            Type::Range(ty) | Type::RangeInclusive(ty) => return ty.element_size(),
            Type::External(_, _) => return None, // cannot know if it's actually Unsized or not, const calculation will be performed instead
            Type::IsSome(_) | Type::IsOk(_) => return Some(ObjectSize::sized(1)),
            Type::Result(_, ok_err_ty) => {
                let mut sum = ObjectSize::SelfDescribing;
                sum = sum.add(ok_err_ty.0.element_size()?);
                sum = sum.add(ok_err_ty.1.element_size()?);
                return Some(sum);
            }
            Type::Option(_, option_ty) => {
                return Some(option_ty.element_size()?.add(ObjectSize::SelfDescribing));
            }
            Type::RefBox(_) => return Some(ObjectSize::Unsized),
            Type::TailSize(None) => 5 * 8,
            Type::TailSize(Some(width)) => {
                if let Expr::Lit(lit) = width
                    && let syn::Lit::Int(lit_int) = &lit.lit
                    && let Ok(width) = lit_int.base10_parse::<usize>()
                {
                    width * 8
                } else {
                    return Some(ObjectSize::sized_symbolic(quote! { 8 * (#width) }));
                }
            }
        };
        Some(ObjectSize::sized(size_bits))
    }
}
