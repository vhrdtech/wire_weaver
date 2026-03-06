use crate::{
    ApiBundleOwned, ApiItemKindOwned, ApiItemOwned, ApiLevelLocationOwned, ApiLevelOwned,
    FieldsOwned, ItemEnumOwned, ItemStructOwned, TypeLocationOwned, TypeOwned,
};
use anyhow::{anyhow, Result};
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
            TypeOwned::Vec(_) => Ok(true),
            TypeOwned::Array { ty, .. } => ty.is_unsized(api_bundle),
            TypeOwned::Tuple(types) => {
                for ty in types {
                    if ty.is_unsized(api_bundle)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            TypeOwned::Struct(item_struct) => item_struct.is_unsized(api_bundle),
            TypeOwned::Enum(item_enum) => item_enum.is_unsized(api_bundle),
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
}

impl FieldsOwned {
    fn is_unsized(&self, api_bundle: &ApiBundleOwned) -> Result<bool> {
        match self {
            FieldsOwned::Named(fields) | FieldsOwned::Unnamed(fields) => {
                for field in fields {
                    if field.ty.is_unsized(api_bundle)? {
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
    pub fn is_unsized(&self, api_bundle: &ApiBundleOwned) -> Result<bool> {
        self.fields.is_unsized(api_bundle)
    }
}

impl ItemEnumOwned {
    /// Returns true if this type contains a string, vector, or box at any depth.
    pub fn is_unsized(&self, api_bundle: &ApiBundleOwned) -> Result<bool> {
        for variant in &self.variants {
            if variant.fields.is_unsized(api_bundle)? {
                return Ok(true);
            }
        }
        Ok(false)
    }
}
