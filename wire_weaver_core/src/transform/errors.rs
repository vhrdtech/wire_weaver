//! `#[errors(Common(InUse, Refused), Nack, ..)]` on a trait method, stream function or property (API-32): parsed and
//! checked against the item's error enum, see `docs/api/errors.md#which-errors-a-method-returns`.

use super::crate_walker::Scratch;
use proc_macro2::Ident;
use std::fmt::{Display, Formatter};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Attribute, Token, parenthesized};
use ww_self::{ErrorVariantOwned, FieldsOwned, ItemEnumOwned, TypeLocationOwned, TypeOwned};

/// A wrong `#[errors(..)]` list. `ww_trait` finds `name` in the attribute to point at it.
#[derive(Debug)]
pub struct ErrorListError {
    /// The method, stream function or property the list is on.
    pub method: String,
    /// The name in the list the error is about (`Nack`, or `InUse` of `Common(InUse)`), `None` for the whole
    /// attribute.
    pub name: Option<String>,
    pub message: String,
}

impl Display for ErrorListError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "#[errors(..)] on {}: {}", self.method, self.message)
    }
}

impl std::error::Error for ErrorListError {}

/// What an `#[errors(..)]` list is on, and so where its error type comes from.
#[derive(Clone, Copy)]
pub(crate) enum ListedOn<'a> {
    /// A method's or stream function's return type, a `Result`.
    Fn {
        stream: bool,
        return_ty: Option<&'a TypeOwned>,
    },
    /// A property's write error type (`property!(rw name: T, SetError)`).
    Property { write_err_ty: Option<&'a TypeOwned> },
}

/// `#[errors(..)]` of a method, stream function or property, checked against its error type, `None` without the
/// attribute.
pub(crate) fn get_errors_attr(
    attrs: &[Attribute],
    method: &str,
    on: ListedOn,
    scratch: &Scratch,
) -> Result<Option<Vec<ErrorVariantOwned>>, ErrorListError> {
    let Some(attr) = attrs.iter().find(|a| a.path().is_ident("errors")) else {
        return Ok(None);
    };
    let err = |name: Option<&Ident>, message: String| ErrorListError {
        method: method.to_string(),
        name: name.map(|n| n.to_string()),
        message,
    };
    let list: ErrorList = attr.parse_args().map_err(|e| {
        err(
            None,
            format!("expected a list of variants like #[errors(Common, Nack)]: {e}"),
        )
    })?;
    let noun = match on {
        ListedOn::Fn { stream: false, .. } => "method",
        ListedOn::Fn { stream: true, .. } => "stream function",
        ListedOn::Property { .. } => "property",
    };
    if list.0.is_empty() {
        return Err(err(
            None,
            match on {
                ListedOn::Fn { .. } => {
                    format!("the list is empty, a {noun} that returns no errors has no Result")
                }
                ListedOn::Property { .. } => {
                    "the list is empty, a property whose write can't fail has no error type".into()
                }
            },
        ));
    }
    let types = &scratch.root_bundle.types;
    let err_ty = match on {
        ListedOn::Fn { return_ty, .. } => match return_ty.map(|ty| in_line(ty, types)) {
            Some(TypeOwned::Result { err_ty, .. }) => err_ty,
            _ => {
                return Err(err(
                    None,
                    format!("the {noun} doesn't return a Result, there are no errors to list"),
                ));
            }
        },
        ListedOn::Property { write_err_ty } => write_err_ty.ok_or_else(|| {
            err(
                None,
                "the property has no write error type (property!(rw name: T, SetError)), there are \
                 no errors to list"
                    .into(),
            )
        })?,
    };
    let TypeOwned::Enum(err_enum) = in_line(err_ty, types) else {
        return Err(err(
            None,
            format!("the {noun}'s error type is not an enum, there are no variants to list"),
        ));
    };

    let mut out: Vec<ErrorVariantOwned> = vec![];
    for entry in list.0 {
        let Some(variant) = err_enum.variants.iter().find(|v| entry.ident == v.ident) else {
            return Err(err(
                Some(&entry.ident),
                format!(
                    "{} has no variant {}, it has: {}",
                    err_enum.ident,
                    entry.ident,
                    variant_names(err_enum)
                ),
            ));
        };
        if out.iter().any(|e| entry.ident == e.ident) {
            return Err(err(
                Some(&entry.ident),
                format!("{} is listed twice", entry.ident),
            ));
        }
        let nested = match entry.nested {
            None => None,
            Some(nested) => {
                let inner = match &variant.fields {
                    FieldsOwned::Unnamed(fields) if fields.len() == 1 => {
                        match in_line(&fields[0].ty, types) {
                            TypeOwned::Enum(inner) => Some(inner),
                            _ => None,
                        }
                    }
                    _ => None,
                };
                let Some(inner) = inner else {
                    return Err(err(
                        Some(&entry.ident),
                        format!(
                            "{}::{} doesn't hold one enum, list it without (..)",
                            err_enum.ident, entry.ident
                        ),
                    ));
                };
                if nested.is_empty() {
                    return Err(err(
                        Some(&entry.ident),
                        format!("{}() lists nothing, write {0} for all of it", entry.ident),
                    ));
                }
                let mut names: Vec<String> = vec![];
                for n in nested {
                    if !inner.variants.iter().any(|v| n == v.ident) {
                        return Err(err(
                            Some(&n),
                            format!(
                                "{} has no variant {n}, it has: {}",
                                inner.ident,
                                variant_names(inner)
                            ),
                        ));
                    }
                    if names.iter().any(|m| n == m) {
                        return Err(err(Some(&n), format!("{n} is listed twice")));
                    }
                    names.push(n.to_string());
                }
                Some(names)
            }
        };
        out.push(ErrorVariantOwned {
            ident: entry.ident.to_string(),
            nested,
        });
    }
    Ok(Some(out))
}

/// `#[errors(..)]` is for methods, stream functions and properties only.
/// `message` says why `item` can't have one.
pub(crate) fn reject_errors_attr(
    attrs: &[Attribute],
    item: &str,
    message: &str,
) -> Result<(), ErrorListError> {
    if attrs.iter().any(|a| a.path().is_ident("errors")) {
        return Err(ErrorListError {
            method: item.to_string(),
            name: None,
            message: message.into(),
        });
    }
    Ok(())
}

fn in_line<'a>(ty: &'a TypeOwned, types: &'a [TypeLocationOwned]) -> &'a TypeOwned {
    if let TypeOwned::OutOfLine { type_idx } | TypeOwned::Instance { type_idx, .. } = ty
        && let Some(TypeLocationOwned::InLine { ty, .. }) = types.get(type_idx.0 as usize)
    {
        ty
    } else {
        ty
    }
}

fn variant_names(item_enum: &ItemEnumOwned) -> String {
    item_enum
        .variants
        .iter()
        .map(|v| v.ident.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

struct ErrorList(Vec<Entry>);

struct Entry {
    ident: Ident,
    nested: Option<Vec<Ident>>,
}

impl Parse for ErrorList {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let entries = Punctuated::<Entry, Token![,]>::parse_terminated(input)?;
        Ok(ErrorList(entries.into_iter().collect()))
    }
}

impl Parse for Entry {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let ident = input.parse()?;
        let nested = if input.peek(syn::token::Paren) {
            let content;
            parenthesized!(content in input);
            let names = Punctuated::<Ident, Token![,]>::parse_terminated(&content)?;
            Some(names.into_iter().collect())
        } else {
            None
        };
        Ok(Entry { ident, nested })
    }
}

#[cfg(test)]
mod tests {
    use super::ErrorListError;
    use std::path::PathBuf;
    use ww_self::{ApiBundleOwned, ErrorVariantOwned};

    /// Loads trait `Api` from a throwaway crate whose `lib.rs` has `methods` in it, next to two error enums.
    fn load(name: &str, methods: &str) -> anyhow::Result<ApiBundleOwned> {
        let dir: PathBuf = std::env::temp_dir()
            .join("ww_core_error_list_tests")
            .join(format!("{name}-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"test_api\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        let lib_rs = format!(
            r#"
            #[ww_api_root]
            pub trait Api {{
                {methods}
            }}

            #[derive_shrink_wrap(ww_repr = u4)]
            pub enum ReadError {{
                Common(Inner),
                Nack,
                Overrun {{ bytes: u8 }},
            }}

            #[derive_shrink_wrap(ww_repr = u4)]
            pub enum Inner {{
                Busy,
                InUse,
            }}
            "#
        );
        std::fs::write(dir.join("src/lib.rs"), lib_rs).unwrap();
        let bundle = crate::load(&dir, Some("Api".into()), false);
        std::fs::remove_dir_all(&dir).unwrap();
        bundle
    }

    fn errors(bundle: &ApiBundleOwned) -> Option<&[ErrorVariantOwned]> {
        bundle.root.items[0].kind.error_list()
    }

    fn list_error(name: &str, methods: &str) -> ErrorListError {
        let e = load(name, methods).unwrap_err();
        let Some(e) = e.chain().find_map(|e| e.downcast_ref::<ErrorListError>()) else {
            panic!("not an ErrorListError: {e:?}");
        };
        ErrorListError {
            method: e.method.clone(),
            name: e.name.clone(),
            message: e.message.clone(),
        }
    }

    #[test]
    fn list_in_the_model() {
        let bundle = load(
            "model",
            "#[errors(Common(InUse, Busy), Overrun, Nack)] fn read() -> Result<u8, ReadError>;",
        )
        .unwrap();
        let entry = |ident: &str, nested: Option<&[&str]>| ErrorVariantOwned {
            ident: ident.into(),
            nested: nested.map(|n| n.iter().map(|s| s.to_string()).collect()),
        };
        assert_eq!(
            errors(&bundle).unwrap(),
            [
                entry("Common", Some(&["InUse", "Busy"])),
                entry("Overrun", None),
                entry("Nack", None)
            ]
        );

        let bundle = load(
            "all",
            "#[errors(Common)] fn read() -> Result<u8, ReadError>;",
        )
        .unwrap();
        assert_eq!(errors(&bundle).unwrap(), [entry("Common", None)]);

        let bundle = load("none", "fn read() -> Result<u8, ReadError>;").unwrap();
        assert_eq!(errors(&bundle), None);
    }

    #[test]
    fn stream_fn_and_property_lists() {
        let entry = |ident: &str| ErrorVariantOwned {
            ident: ident.into(),
            nested: None,
        };
        let bundle = load(
            "stream_fn",
            "#[errors(Nack)] #[stream(u8, counted)] fn samples(n: u8) -> Result<(), ReadError>;",
        )
        .unwrap();
        assert_eq!(errors(&bundle).unwrap(), [entry("Nack")]);

        let bundle = load(
            "property_list",
            "#[errors(Overrun, Common(Busy))] property!(rw value: u8, ReadError);",
        )
        .unwrap();
        assert_eq!(
            errors(&bundle).unwrap(),
            [
                entry("Overrun"),
                ErrorVariantOwned {
                    ident: "Common".into(),
                    nested: Some(vec!["Busy".into()]),
                }
            ]
        );

        let e = list_error(
            "property_unknown",
            "#[errors(Timeout)] property!(wo value: u8, ReadError);",
        );
        assert_eq!(e.method, "value");
        assert_eq!(e.name.as_deref(), Some("Timeout"));
    }

    #[test]
    fn unknown_names_point_at_the_name() {
        let e = list_error(
            "unknown",
            "#[errors(Nack, Timeout)] fn read() -> Result<u8, ReadError>;",
        );
        assert_eq!(e.method, "read");
        assert_eq!(e.name.as_deref(), Some("Timeout"));
        assert_eq!(
            e.message,
            "ReadError has no variant Timeout, it has: Common, Nack, Overrun"
        );

        let e = list_error(
            "unknown_nested",
            "#[errors(Common(Busy, Refused))] fn read() -> Result<u8, ReadError>;",
        );
        assert_eq!(e.name.as_deref(), Some("Refused"));
        assert_eq!(
            e.message,
            "Inner has no variant Refused, it has: Busy, InUse"
        );
    }

    #[test]
    fn wrong_lists() {
        let message = |name: &str, methods: &str| list_error(name, methods).message;
        assert_eq!(
            message(
                "twice",
                "#[errors(Nack, Nack)] fn read() -> Result<u8, ReadError>;"
            ),
            "Nack is listed twice"
        );
        assert_eq!(
            message(
                "twice_nested",
                "#[errors(Common(Busy, Busy))] fn read() -> Result<u8, ReadError>;"
            ),
            "Busy is listed twice"
        );
        assert_eq!(
            message(
                "not_nested",
                "#[errors(Overrun(Busy))] fn read() -> Result<u8, ReadError>;"
            ),
            "ReadError::Overrun doesn't hold one enum, list it without (..)"
        );
        assert_eq!(
            message("empty", "#[errors()] fn read() -> Result<u8, ReadError>;"),
            "the list is empty, a method that returns no errors has no Result"
        );
        assert_eq!(
            message("no_result", "#[errors(Nack)] fn read() -> u8;"),
            "the method doesn't return a Result, there are no errors to list"
        );
        assert_eq!(
            message("not_enum", "#[errors(Nack)] fn read() -> Result<u8, u8>;"),
            "the method's error type is not an enum, there are no variants to list"
        );
        assert_eq!(
            message("property", "#[errors(Nack)] property!(ro value: u8);"),
            "the property has no write error type (property!(rw name: T, SetError)), there are no errors to list"
        );
        assert_eq!(
            message(
                "property_empty",
                "#[errors()] property!(rw value: u8, ReadError);"
            ),
            "the list is empty, a property whose write can't fail has no error type"
        );
        assert_eq!(
            message(
                "stream_fn",
                "#[errors(Nack)] #[stream(u8)] fn samples(n: u8);"
            ),
            "the stream function doesn't return a Result, there are no errors to list"
        );
        assert_eq!(
            message("stream", "#[errors(Nack)] stream!(samples: u8);"),
            "ww_stream! has no result to list errors of, a stream function can have one: #[errors(..)] \
             #[stream(T)] fn name() -> Result<(), E>"
        );
        assert_eq!(
            message("const", "#[errors(Nack)] const LIMIT: u8 = 1;"),
            "a const can't fail, only methods, stream functions and properties have error lists"
        );
        assert!(
            message(
                "syntax",
                "#[errors(Nack = 1)] fn read() -> Result<u8, ReadError>;"
            )
            .starts_with("expected a list of variants like #[errors(Common, Nack)]"),
        );
    }
}
