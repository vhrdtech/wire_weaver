use crate::ast::object_size::ObjectSize;
use crate::ast::path::Path;
use proc_macro2::Ident;

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

    // TODO: remove I4, add UB, IB
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
            Type::RefBox(ref_box) => {
                ref_box.make_owned();
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

    pub fn visit_external_types_mut<F: FnMut(&mut Path, bool)>(&mut self, f: &mut F) {
        match self {
            Type::External(path, potential_lifetimes) => {
                f(path, *potential_lifetimes);
            }
            Type::Option(_, some_ty) => {
                some_ty.visit_external_types_mut(f);
            }
            Type::Result(_, ok_err_ty) => {
                let (ok_ty, err_ty) = &mut **ok_err_ty;
                ok_ty.visit_external_types_mut(f);
                err_ty.visit_external_types_mut(f);
            }
            Type::Array(_, ty) => {
                ty.visit_external_types_mut(f);
            }
            Type::Tuple(types) => {
                for ty in types {
                    ty.visit_external_types_mut(f);
                }
            }
            Type::Vec(ty) => {
                ty.visit_external_types_mut(f);
            }
            Type::RefBox(ty) => {
                ty.visit_external_types_mut(f);
            }
            _ => {}
        }
    }

    pub fn prepend_ext_paths(&self, ident: &Ident) -> Type {
        let mut ty = self.clone();
        ty.visit_external_types_mut(&mut |path, _| {
            path.prepend(ident);
        });
        ty
    }

    /// Return ElementSize if it is known. None is returned for Unsized.
    pub fn element_size(&self) -> Option<ObjectSize> {
        let size_bits = match self {
            Type::Bool => 1,
            Type::U4 => 4,
            Type::U8 => 8,
            Type::U16 => 16,
            Type::U32 => 32,
            Type::U64 => 64,
            Type::U128 => 128,
            Type::UNib32 => return Some(ObjectSize::SelfDescribing),
            Type::ULeb32 => return Some(ObjectSize::SelfDescribing),
            Type::ULeb64 => return Some(ObjectSize::SelfDescribing),
            Type::ULeb128 => return Some(ObjectSize::SelfDescribing),
            Type::I4 => 4,
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
                    ObjectSize::Sized { size_bits } => ObjectSize::Sized {
                        size_bits: len * size_bits,
                    },
                };
                return Some(size);
            }
            Type::Tuple(types) => {
                let mut sum = ObjectSize::Sized { size_bits: 0 };
                for ty in types {
                    sum = sum.add(ty.element_size()?);
                }
                return Some(sum);
            }
            Type::Vec(_) => return Some(ObjectSize::UnsizedFinalStructure),
            Type::Range(ty) | Type::RangeInclusive(ty) => return ty.element_size(),
            Type::External(_, _) => return None, // cannot know if it's actually Unsized or not, const calculation will be performed instead
            Type::IsSome(_) | Type::IsOk(_) => return Some(ObjectSize::Sized { size_bits: 1 }),
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
        };
        Some(ObjectSize::Sized { size_bits })
    }
}
