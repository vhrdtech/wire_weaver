//! Per-item error lists (`#[errors(..)]` on methods, stream functions and properties, API-32): how they read, what
//! they document, and whether an error a device returned is in one. Used by the generated client's docs, the dynamic client, Python and the ww CLI.
//!
//! The common errors (STD-16) are listed as `CommonError` or `CommonError(Busy, InUse)`: not a variant of the
//! item's error enum, the protocol carries them beside it, so the entry is allowed on items without an error type
//! of their own too.

use crate::{
    ApiBundleOwned, ApiItemKindOwned, ErrorVariantOwned, FieldsOwned, FieldsValueOwned,
    ItemEnumOwned, TypeOwned, ValueOwned, VariantOwned,
};
use ww_error::CommonError;

/// The entry of an error list that names the common errors (STD-16): `CommonError` for all of them,
/// `CommonError(Busy, InUse)` for some.
pub const COMMON_ENTRY: &str = "CommonError";

/// The first doc line of the common error `name` (`Busy`), from [CommonError::DOCS]; `None` for an unknown name.
pub fn common_error_doc(name: &str) -> Option<&'static str> {
    CommonError::DOCS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, doc)| *doc)
}

/// The names of the common errors, as `CommonError(..)` lists them.
pub fn common_error_names() -> impl Iterator<Item = &'static str> {
    CommonError::DOCS.iter().map(|(name, _)| *name)
}

/// Whether the common error `name` (`Busy`) is in `list` (STD-16): yes without a `CommonError` entry (the list
/// says nothing about common errors then) and with a bare `CommonError`, else when `CommonError(..)` names it.
pub fn error_list_allows_common(list: &[ErrorVariantOwned], name: &str) -> bool {
    match list.iter().find(|e| e.ident == COMMON_ENTRY) {
        None => true,
        Some(entry) => entry
            .nested
            .as_ref()
            .is_none_or(|nested| nested.iter().any(|n| n == name)),
    }
}

impl ApiItemKindOwned {
    /// `#[errors(..)]` of a method, stream function (the open's result) or property (its write), `None` without one
    /// and for other items.
    pub fn error_list(&self) -> Option<&[ErrorVariantOwned]> {
        match self {
            ApiItemKindOwned::Method { errors, .. }
            | ApiItemKindOwned::StreamFn { errors, .. }
            | ApiItemKindOwned::Property { errors, .. } => errors.as_deref(),
            _ => None,
        }
    }

    /// The error type an `#[errors(..)]` list is about: the `Err` of a method's or stream function's `Result`, a
    /// property's write error type; `None` for other items and infallible ones.
    pub fn err_ty<'a>(&'a self, bundle: &'a ApiBundleOwned) -> Option<&'a TypeOwned> {
        match self {
            ApiItemKindOwned::Method { return_ty, .. }
            | ApiItemKindOwned::StreamFn { return_ty, .. } => {
                method_err_ty(return_ty.as_ref(), bundle)
            }
            ApiItemKindOwned::Property { write_err_ty, .. } => write_err_ty.as_ref(),
            _ => None,
        }
    }
}

impl ErrorVariantOwned {
    /// The entries of `nested`, each as written in `#[errors(..)]`: `["Nack"]`, `["CommonError(InUse)",
    /// "CommonError(Refused)"]`, `["CommonError"]` for all of the common errors.
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

/// The list as written in `#[errors(..)]`: `CommonError(InUse, Refused), Nack`.
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

/// One line per entry of `list`: its name (`Nack`, `CommonError(InUse)`) and the first doc line of the variant
/// without the final period, the convention of `display` (DER-12) and [ValueOwned::doc_message]; `any Inner` for
/// a variant holding an enum listed without `(..)`, `any common error` for a bare `CommonError`. `err_ty` is the
/// item's error type, `None` for an item without one; a variant whose definition is not in `bundle` (left out,
/// known from snapshots) has no doc line.
pub fn error_list_docs(
    list: &[ErrorVariantOwned],
    err_ty: Option<&TypeOwned>,
    bundle: &ApiBundleOwned,
) -> Vec<(String, Option<String>)> {
    let item_enum = err_ty.and_then(|ty| as_enum(ty, bundle));
    let mut lines = vec![];
    for entry in list {
        if entry.ident == COMMON_ENTRY {
            match &entry.nested {
                None => lines.push((COMMON_ENTRY.into(), Some("any common error".into()))),
                Some(nested) => lines.extend(nested.iter().map(|n| {
                    (
                        format!("{COMMON_ENTRY}({n})"),
                        common_error_doc(n).map(str::to_owned),
                    )
                })),
            }
            continue;
        }
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

/// Whether the error value `err` (a method's `Err`, an error of the API's own) is in `list`, by variant names. A
/// value that isn't a variant of an enum (or a variant from a newer version, `<unknown N>`) is not in any list;
/// a common error is checked with [error_list_allows_common].
pub fn error_list_contains(list: &[ErrorVariantOwned], err: &ValueOwned) -> bool {
    let ValueOwned::Enum { variant, fields } = err else {
        return false;
    };
    let Some(entry) = list
        .iter()
        .find(|e| &e.ident == variant && e.ident != COMMON_ENTRY)
    else {
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

/// Name of an error value for messages: `Nack`, `Mode(Fast)`, the [Debug] form for anything else.
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
