use crate::{
    ApiBundleOwned, ApiItemKindOwned, ApiItemOwned, ApiLevelLocationOwned, ApiLevelOwned,
    FieldsOwned, ItemEnumOwned, ItemStructOwned, Repr, TypeLocationOwned, TypeOwned,
};
use anyhow::{anyhow, Result};
use shrink_wrap::ElementSize;
use ww_numeric::{NumericAnyTypeOwned, NumericBaseType};

impl ApiLevelOwned {
    pub fn crate_name<'i>(&self, bundle: &'i ApiBundleOwned) -> Result<&'i str> {
        bundle.crate_name(self.crate_idx.0)
    }
}

impl ApiItemOwned {
    pub fn get_as_level<'i>(&self, bundle: &'i ApiBundleOwned) -> Result<&'i ApiLevelOwned> {
        let ApiItemKindOwned::Trait { trait_idx } = &self.kind else {
            return Err(anyhow!("ApiItem is not Trait"));
        };
        bundle.get_trait(trait_idx.0)
    }
}

impl ApiBundleOwned {
    pub fn get_trait(&self, trait_idx: u32) -> Result<&ApiLevelOwned> {
        let location = self
            .traits
            .get(trait_idx as usize)
            .ok_or(anyhow!("Bad ApiBundle: no trait with index: {}", trait_idx))?;
        let ApiLevelLocationOwned::InLine {
            level,
            crate_idx: _,
        } = location
        else {
            return Err(anyhow!(
                "Not resolved ApiBundle: trait with index: {} is not inlined",
                trait_idx
            ));
        };
        Ok(level)
    }

    pub fn get_ty(&self, type_idx: u32) -> Result<(&TypeOwned, u32)> {
        let location = self
            .types
            .get(type_idx as usize)
            .ok_or(anyhow!("Bad ApiBundle: no type with index: {}", type_idx))?;
        if let TypeLocationOwned::InLine { ty, crate_idx } = location {
            Ok((ty, crate_idx.0))
        } else {
            Err(anyhow!(
                "Not resolved ApiBundle: type with index: {} is not inlined",
                type_idx
            ))
        }
    }

    pub fn crate_name(&self, crate_idx: u32) -> Result<&str> {
        let Some(version) = self.ext_crates.get(crate_idx as usize) else {
            return Err(anyhow!("Bad ApiBundle: no crate with index: {}", crate_idx));
        };
        Ok(version.crate_id.as_str())
    }
}

impl TypeOwned {
    /// Returns true if this type contains a string, vector, or box at any depth.
    pub fn is_lifetime(&self, api_bundle: &ApiBundleOwned) -> Result<bool> {
        match self {
            TypeOwned::Bool => Ok(false),
            TypeOwned::NumericAny(_) => Ok(false),
            TypeOwned::OutOfLine { type_idx } => {
                let ty = api_bundle.get_ty(type_idx.0)?;
                ty.0.is_lifetime(api_bundle)
            }
            TypeOwned::Flag => Ok(false),
            TypeOwned::String => Ok(true),
            TypeOwned::Vec(_) => Ok(true),
            TypeOwned::Array { ty, .. } => ty.is_lifetime(api_bundle),
            TypeOwned::Tuple(types) => {
                for ty in types {
                    if ty.is_lifetime(api_bundle)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            TypeOwned::Struct(item_struct) => item_struct.is_lifetime(api_bundle),
            TypeOwned::Enum(item_enum) => item_enum.is_lifetime(api_bundle),
            TypeOwned::Option { some_ty } => some_ty.is_lifetime(api_bundle),
            TypeOwned::Result { ok_ty, err_ty } => {
                if ok_ty.is_lifetime(api_bundle)? {
                    return Ok(true);
                }
                err_ty.is_lifetime(api_bundle)
            }
            TypeOwned::Box(_) => Ok(true),
            TypeOwned::Range(_) => Ok(false),
            TypeOwned::RangeInclusive(_) => Ok(false),
        }
    }

    pub fn is_unsized(&self, api_bundle: &ApiBundleOwned) -> Result<bool> {
        match self {
            TypeOwned::Bool => Ok(false),
            TypeOwned::NumericAny(_) => Ok(false),
            TypeOwned::OutOfLine { type_idx } => {
                let ty = api_bundle.get_ty(type_idx.0)?;
                ty.0.is_unsized(api_bundle)
            }
            TypeOwned::Flag => Ok(false),
            TypeOwned::String => Ok(true),
            TypeOwned::Vec(_) => Ok(false), // Vec is UnsizedFinalStructure, see shrink_wrap::ElementSize
            TypeOwned::Array { ty, .. } => ty.is_unsized(api_bundle),
            TypeOwned::Tuple(types) => {
                for ty in types {
                    if ty.is_unsized(api_bundle)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            TypeOwned::Struct(item_struct) => Ok(item_struct.is_unsized()),
            TypeOwned::Enum(item_enum) => Ok(item_enum.is_unsized()),
            TypeOwned::Option { some_ty } => some_ty.is_unsized(api_bundle),
            TypeOwned::Result { ok_ty, err_ty } => {
                if ok_ty.is_unsized(api_bundle)? {
                    return Ok(true);
                }
                err_ty.is_unsized(api_bundle)
            }
            TypeOwned::Box(_) => Ok(true),
            TypeOwned::Range(_) => Ok(false),
            TypeOwned::RangeInclusive(_) => Ok(false),
        }
    }

    pub fn is_byte_slice(&self, api_bundle: &ApiBundleOwned) -> Result<bool> {
        match self {
            TypeOwned::OutOfLine { type_idx } => {
                let ty = api_bundle.get_ty(type_idx.0)?;
                ty.0.is_byte_slice(api_bundle)
            }
            TypeOwned::Vec(inner) => Ok(matches!(
                inner.as_ref(),
                TypeOwned::NumericAny(NumericAnyTypeOwned::Base(NumericBaseType::U8))
            )),
            _ => Ok(false),
        }
    }

    pub fn get_in_line<'i>(&'i self, api_bundle: &'i ApiBundleOwned) -> Result<&'i TypeOwned> {
        if let TypeOwned::OutOfLine { type_idx } = self {
            let (ty, _) = api_bundle.get_ty(type_idx.0)?;
            Ok(ty)
        } else {
            Ok(self)
        }
    }

    pub fn human_name(&self, api_bundle: &ApiBundleOwned) -> Result<String> {
        match self {
            TypeOwned::Bool => Ok("bool".to_string()),
            TypeOwned::NumericAny(numeric) => Ok(numeric.human_name()),
            TypeOwned::OutOfLine { type_idx } => {
                let ty = api_bundle.get_ty(type_idx.0)?.0;
                ty.human_name(api_bundle)
            }
            TypeOwned::Flag => Ok("flag".to_string()),
            TypeOwned::String => Ok("String".to_string()),
            TypeOwned::Vec(inner) => Ok(format!("Vec<{}>", inner.human_name(api_bundle)?)),
            TypeOwned::Array { len, ty } => {
                Ok(format!("[{}; {}]", ty.human_name(api_bundle)?, len.0))
            }
            TypeOwned::Tuple(types) => {
                let mut names = Vec::with_capacity(types.len());
                for ty in types {
                    names.push(ty.human_name(api_bundle)?);
                }
                Ok(format!("({})", names.join(", ")))
            }
            TypeOwned::Struct(item_struct) => Ok(format!(
                "{}::{}",
                api_bundle.crate_name(item_struct.crate_idx.0)?,
                item_struct.ident
            )),
            TypeOwned::Enum(item_enum) => Ok(format!(
                "{}::{}",
                api_bundle.crate_name(item_enum.crate_idx.0)?,
                item_enum.ident
            )),
            TypeOwned::Option { some_ty } => {
                Ok(format!("Option<{}>", some_ty.human_name(api_bundle)?))
            }
            TypeOwned::Result { ok_ty, err_ty } => Ok(format!(
                "Result<{}, {}>",
                ok_ty.human_name(api_bundle)?,
                err_ty.human_name(api_bundle)?
            )),
            TypeOwned::Box(inner) => Ok(format!("Box<{}>", inner.human_name(api_bundle)?)),
            TypeOwned::Range(base) => Ok(format!("Range<{}>", base.name())),
            TypeOwned::RangeInclusive(base) => Ok(format!("RangeInclusive<{}>", base.name())),
        }
    }

    pub fn human_definition(
        &self,
        api_bundle: &ApiBundleOwned,
        single_line: bool,
    ) -> Result<String> {
        match self {
            TypeOwned::Bool => Ok("bool".to_string()),
            TypeOwned::NumericAny(numeric) => Ok(numeric.human_name()),
            TypeOwned::OutOfLine { type_idx } => {
                let ty = api_bundle.get_ty(type_idx.0)?.0;
                ty.human_definition(api_bundle, single_line)
            }
            TypeOwned::Flag => Ok("flag".to_string()),
            TypeOwned::String => Ok("String".to_string()),
            TypeOwned::Vec(inner) => Ok(format!(
                "Vec<{}>",
                inner.human_definition(api_bundle, single_line)?
            )),
            TypeOwned::Array { len, ty } => Ok(format!(
                "[{}; {}]",
                ty.human_definition(api_bundle, single_line)?,
                len.0
            )),
            TypeOwned::Tuple(types) => {
                let mut names = Vec::with_capacity(types.len());
                for ty in types {
                    names.push(ty.human_definition(api_bundle, single_line)?);
                }
                Ok(format!("({})", names.join(", ")))
            }
            TypeOwned::Struct(item_struct) => {
                let mut s = format!(
                    "struct {}::{} {{",
                    api_bundle.crate_name(item_struct.crate_idx.0)?,
                    item_struct.ident
                );
                s +=
                    fields_human_definition(&item_struct.fields, api_bundle, single_line)?.as_str();
                Ok(s)
            }
            TypeOwned::Enum(item_enum) => {
                let repr = match item_enum.repr {
                    Repr::Nibble => "nib".to_string(),
                    Repr::BitAligned(bits) => format!("ub{bits}"),
                    Repr::UNib32 => "unib32".to_string(),
                    Repr::ByteAlignedU8 => "u8".to_string(),
                    Repr::ByteAlignedU16 => "u16".to_string(),
                    Repr::ByteAlignedU32 => "u32".to_string(),
                };
                let mut s = format!(
                    "enum {repr} {}::{} {{",
                    api_bundle.crate_name(item_enum.crate_idx.0)?,
                    item_enum.ident
                );
                for (idx, variant) in item_enum.variants.iter().enumerate() {
                    s += &variant.ident;
                    s +=
                        fields_human_definition(&variant.fields, api_bundle, single_line)?.as_str();
                    if idx + 1 < item_enum.variants.len() {
                        s += if single_line { ", " } else { ",\n" };
                    }
                }
                s += "}";
                Ok(s)
            }
            TypeOwned::Option { some_ty } => Ok(format!(
                "Option<{}>",
                some_ty.human_definition(api_bundle, single_line)?
            )),
            TypeOwned::Result { ok_ty, err_ty } => Ok(format!(
                "Result<{}, {}>",
                ok_ty.human_definition(api_bundle, single_line)?,
                err_ty.human_definition(api_bundle, single_line)?
            )),
            TypeOwned::Box(inner) => Ok(format!(
                "Box<{}>",
                inner.human_definition(api_bundle, single_line)?
            )),
            TypeOwned::Range(base) => Ok(format!("Range<{}>", base.name())),
            TypeOwned::RangeInclusive(base) => Ok(format!("RangeInclusive<{}>", base.name())),
        }
    }
}

fn fields_human_definition(
    fields: &FieldsOwned,
    api_bundle: &ApiBundleOwned,
    single_line: bool,
) -> Result<String> {
    match fields {
        FieldsOwned::Named(named) => {
            let mut s = "{".to_string();
            for (idx, field) in named.iter().enumerate() {
                s += field.ident.as_deref().unwrap_or("");
                s += ": ";
                s += &field.ty.human_definition(api_bundle, single_line)?;
                if idx + 1 < named.len() {
                    s += if single_line { ", " } else { ",\n" };
                }
            }
            s += "}";
            Ok(s)
        }
        FieldsOwned::Unnamed(unnamed) => {
            let mut s = "(".to_string();
            for (idx, field) in unnamed.iter().enumerate() {
                s += &field.ty.human_definition(api_bundle, single_line)?;
                if idx + 1 < unnamed.len() {
                    s += if single_line { ", " } else { ",\n" };
                }
            }
            s += ")";
            Ok(s)
        }
        FieldsOwned::Unit => Ok("".to_string()),
    }
}

impl FieldsOwned {
    fn is_lifetime(&self, api_bundle: &ApiBundleOwned) -> Result<bool> {
        match self {
            FieldsOwned::Named(fields) | FieldsOwned::Unnamed(fields) => {
                for field in fields {
                    if field.ty.is_lifetime(api_bundle)? {
                        return Ok(true);
                    }
                }
            }
            FieldsOwned::Unit => {}
        }
        Ok(false)
    }
}

impl ItemStructOwned {
    /// Returns true if this type contains a string, vector, or box at any depth.
    pub fn is_lifetime(&self, api_bundle: &ApiBundleOwned) -> Result<bool> {
        self.fields.is_lifetime(api_bundle)
    }

    pub fn is_unsized(&self) -> bool {
        self.size == ElementSize::Unsized
    }
}

impl ItemEnumOwned {
    /// Returns true if this type contains a string, vector, or box at any depth.
    pub fn is_lifetime(&self, api_bundle: &ApiBundleOwned) -> Result<bool> {
        for variant in &self.variants {
            if variant.fields.is_lifetime(api_bundle)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn is_unsized(&self) -> bool {
        self.size == ElementSize::Unsized
    }
}

impl ItemEnumOwned {
    pub fn discriminant(&self, variant_name: &str) -> Result<u32> {
        self.variants
            .iter()
            .find_map(|variant| {
                if variant.ident == variant_name {
                    Some(variant.discriminant.0)
                } else {
                    None
                }
            })
            .ok_or(anyhow!(
                "Enum {} does not have variant {}",
                self.ident,
                variant_name
            ))
    }
}
