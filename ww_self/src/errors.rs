//! Per-method error lists (`#[errors(..)]`, API-32): how they read, what they document, and whether an error a
//! device returned is in one. Used by the generated client's docs, the dynamic client, Python and the ww CLI.

use crate::{
    ApiBundleOwned, ApiItemKindOwned, ErrorVariantOwned, FieldsOwned, FieldsValueOwned,
    ItemEnumOwned, TypeOwned, ValueOwned, VariantOwned,
};

impl ApiItemKindOwned {
    /// `#[errors(..)]` of a method, `None` without one and for other items.
    pub fn error_list(&self) -> Option<&[ErrorVariantOwned]> {
        match self {
            ApiItemKindOwned::Method { errors, .. } => errors.as_deref(),
            _ => None,
        }
    }
}

impl ErrorVariantOwned {
    /// The entries of `nested`, each as written in `#[errors(..)]`: `["Nack"]`, `["Common(InUse)",
    /// "Common(Refused)"]`, `["Common"]` for all of a nested enum's variants.
    pub fn entries(&self) -> Vec<String> {
        match &self.nested {
            None => vec![self.ident.clone()],
            Some(nested) => nested
                .iter()
                .map(|n| format!("{}({n})", self.ident))
                .collect(),
        }
    }
}

/// The list as written in `#[errors(..)]`: `Common(InUse, Refused), Nack`.
pub fn fmt_error_list(list: &[ErrorVariantOwned]) -> String {
    list.iter()
        .map(|v| match &v.nested {
            None => v.ident.clone(),
            Some(nested) => format!("{}({})", v.ident, nested.join(", ")),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The `Err` type of a method's return type, `None` when it doesn't return a `Result`.
pub fn method_err_ty<'a>(
    return_ty: Option<&'a TypeOwned>,
    bundle: &'a ApiBundleOwned,
) -> Option<&'a TypeOwned> {
    match return_ty?.get_in_line(bundle).ok()? {
        TypeOwned::Result { err_ty, .. } => Some(err_ty),
        _ => None,
    }
}

/// One line per entry of `list`: its name (`Nack`, `Common(InUse)`) and the first doc line of the variant without
/// the final period, the convention of `display` (DER-12) and [ValueOwned::doc_message]; `any CommonError` for a
/// variant holding an enum listed without `(..)`. `err_ty` is the method's
/// error type; a variant whose definition is not in `bundle` (left out, known from snapshots) has no doc line.
pub fn error_list_docs(
    list: &[ErrorVariantOwned],
    err_ty: &TypeOwned,
    bundle: &ApiBundleOwned,
) -> Vec<(String, Option<String>)> {
    let item_enum = as_enum(err_ty, bundle);
    let mut lines = vec![];
    for entry in list {
        let variant = item_enum.and_then(|e| e.variants.iter().find(|v| v.ident == entry.ident));
        match &entry.nested {
            None => {
                // `Common` alone: every variant of the nested enum
                let doc = match variant.and_then(|v| nested_enum(v, bundle)) {
                    Some(inner) => Some(format!("any {}", inner.ident)),
                    None => variant.and_then(doc_line),
                };
                lines.push((entry.ident.clone(), doc));
            }
            Some(nested) => {
                let nested_enum = variant.and_then(|v| nested_enum(v, bundle));
                for n in nested {
                    let doc = nested_enum
                        .and_then(|e| e.variants.iter().find(|v| &v.ident == n))
                        .and_then(doc_line);
                    lines.push((format!("{}({n})", entry.ident), doc));
                }
            }
        }
    }
    lines
}

/// Whether the error value `err` (a method's `Err`) is in `list`, by variant names. A value that
/// isn't a variant of an enum (or a variant from a newer version, `<unknown N>`) is not in any list.
pub fn error_list_contains(list: &[ErrorVariantOwned], err: &ValueOwned) -> bool {
    let ValueOwned::Enum { variant, fields } = err else {
        return false;
    };
    let Some(entry) = list.iter().find(|e| &e.ident == variant) else {
        return false;
    };
    let Some(nested) = &entry.nested else {
        return true;
    };
    let FieldsValueOwned::Unnamed(values) = fields else {
        return false;
    };
    match values.as_slice() {
        [ValueOwned::Enum { variant: inner, .. }] => nested.iter().any(|n| n == inner),
        _ => false,
    }
}

/// Name of an error value for messages: `Nack`, `Common(InUse)`, the [Debug] form for anything else.
pub fn error_value_name(err: &ValueOwned) -> String {
    match err {
        ValueOwned::Enum {
            variant,
            fields: FieldsValueOwned::Unnamed(values),
        } if values.len() == 1 => match &values[0] {
            ValueOwned::Enum { variant: inner, .. } => format!("{variant}({inner})"),
            _ => variant.clone(),
        },
        ValueOwned::Enum { variant, .. } => variant.clone(),
        other => format!("{other:?}"),
    }
}

fn as_enum<'a>(ty: &'a TypeOwned, bundle: &'a ApiBundleOwned) -> Option<&'a ItemEnumOwned> {
    match ty.get_in_line(bundle).ok()? {
        TypeOwned::Enum(item_enum) => Some(item_enum),
        _ => None,
    }
}

/// The enum in a variant's only unnamed field (`Common(CommonError)`).
fn nested_enum<'a>(
    variant: &'a VariantOwned,
    bundle: &'a ApiBundleOwned,
) -> Option<&'a ItemEnumOwned> {
    match &variant.fields {
        FieldsOwned::Unnamed(fields) if fields.len() == 1 => as_enum(&fields[0].ty, bundle),
        _ => None,
    }
}

fn doc_line(variant: &VariantOwned) -> Option<String> {
    let line = variant.docs.first()?.trim();
    let line = line.strip_suffix('.').unwrap_or(line);
    (!line.is_empty()).then(|| line.to_string())
}
