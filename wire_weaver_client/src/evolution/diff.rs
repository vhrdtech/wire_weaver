//! Every difference between two versions of a crate, doc comments included, for a human to review. Unlike
//! [compare](super::compare), nothing is classified here.

use std::collections::BTreeMap;

use wire_weaver::shrink_wrap::ElementSize;
use ww_self::{
    ApiBundleOwned, ApiItemKindOwned, ApiItemOwned, ApiLevelLocationOwned, ApiLevelOwned,
    FieldOwned, FieldsOwned, Multiplicity, PropertyAccess, Repr, TypeOwned,
};
use ww_version::VersionTriplet;

use super::{display, own_traits, own_types};
use crate::compat::{field_name, fields};

/// One difference, `path` is e.g. `Uart::write`, `type Config`, `Config: field `baud``.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Difference {
    Added {
        path: String,
        /// One line definition, e.g. `fn write(data: Vec<u8>)`.
        definition: String,
    },
    Removed {
        path: String,
        definition: String,
    },
    Changed {
        path: String,
        /// What changed, e.g. `signature`, `type`, `#[since]`.
        what: &'static str,
        old: String,
        new: String,
    },
    /// Doc comment lines, without `///`.
    Docs {
        path: String,
        old: Vec<String>,
        new: Vec<String>,
    },
}

/// All differences between an old version of a crate and a new one, in the order of traits, resources, types and
/// dependencies. Both are crate bundles, as loaded by `wire_weaver_core::load_crate` and saved by `ww api save`.
///
/// Matched the same way as by [compare](super::compare): traits and types by name, resources by id and enum variants by
/// discriminant, so a renamed resource or variant is a change, while a renamed type is removed and added. Fields
/// are matched by name, unnamed ones by position.
pub fn diff(old: &ApiBundleOwned, new: &ApiBundleOwned) -> Result<Vec<Difference>, String> {
    let (Some(old_version), Some(new_version)) = (old.ext_crates.first(), new.ext_crates.first())
    else {
        return Err("not a crate snapshot, ext_crates[0] must be the crate itself".into());
    };
    if old_version.crate_id != new_version.crate_id {
        return Err(format!(
            "comparing different crates: {} and {}",
            old_version.crate_id, new_version.crate_id
        ));
    }
    let mut differ = Differ {
        old,
        new,
        out: vec![],
    };
    differ.traits();
    differ.types();
    differ.dependencies();
    Ok(differ.out)
}

struct Differ<'a> {
    old: &'a ApiBundleOwned,
    new: &'a ApiBundleOwned,
    out: Vec<Difference>,
}

impl Differ<'_> {
    fn added(&mut self, path: String, definition: String) {
        self.out.push(Difference::Added { path, definition });
    }

    fn removed(&mut self, path: String, definition: String) {
        self.out.push(Difference::Removed { path, definition });
    }

    fn changed(&mut self, path: &str, what: &'static str, old: String, new: String) {
        if old != new {
            self.out.push(Difference::Changed {
                path: path.to_string(),
                what,
                old,
                new,
            });
        }
    }

    fn docs(&mut self, path: &str, old: &[String], new: &[String]) {
        if old != new {
            self.out.push(Difference::Docs {
                path: path.to_string(),
                old: old.to_vec(),
                new: new.to_vec(),
            });
        }
    }

    fn since(&mut self, path: &str, old: Option<VersionTriplet>, new: Option<VersionTriplet>) {
        self.changed(path, "#[since]", since(old), since(new));
    }

    fn traits(&mut self) {
        let (old_traits, new_traits) = (own_traits(self.old), own_traits(self.new));
        for (name, old_level) in &old_traits {
            let path = format!("trait {name}");
            match new_traits.get(name) {
                Some(new_level) => {
                    self.docs(&path, &old_level.docs, &new_level.docs);
                    self.items(name, old_level, new_level);
                }
                None => self.removed(path, format!("{} resources", old_level.items.len())),
            }
        }
        for (name, new_level) in &new_traits {
            if !old_traits.contains_key(name) {
                let path = format!("trait {name}");
                self.added(path, format!("{} resources", new_level.items.len()));
            }
        }
    }

    fn items(&mut self, trait_name: &str, old: &ApiLevelOwned, new: &ApiLevelOwned) {
        let new_items: BTreeMap<u32, &ApiItemOwned> =
            new.items.iter().map(|item| (item.id.0, item)).collect();
        for old_item in &old.items {
            let path = format!("{trait_name}::{}", old_item.ident);
            let Some(new_item) = new_items.get(&old_item.id.0) else {
                self.removed(path, item_signature(self.old, old_item));
                continue;
            };
            self.changed(
                &path,
                "name",
                old_item.ident.clone(),
                new_item.ident.clone(),
            );
            self.changed(
                &path,
                "signature",
                item_signature(self.old, old_item),
                item_signature(self.new, new_item),
            );
            self.since(&path, old_item.since, new_item.since);
            self.docs(&path, &old_item.docs, &new_item.docs);
        }
        for new_item in &new.items {
            if !old.items.iter().any(|i| i.id == new_item.id) {
                let path = format!("{trait_name}::{}", new_item.ident);
                self.added(path, item_signature(self.new, new_item));
            }
        }
    }

    fn types(&mut self) {
        let (old_types, new_types) = (own_types(self.old), own_types(self.new));
        for (name, old_ty) in &old_types {
            let path = format!("type {name}");
            match new_types.get(name) {
                Some(new_ty) => self.ty(name, old_ty, new_ty),
                None => self.removed(path, type_header(old_ty)),
            }
        }
        for (name, new_ty) in &new_types {
            if !old_types.contains_key(name) {
                self.added(format!("type {name}"), type_header(new_ty));
            }
        }
    }

    fn ty(&mut self, name: &str, old: &TypeOwned, new: &TypeOwned) {
        let path = format!("type {name}");
        self.changed(&path, "kind", type_header(old), type_header(new));
        match (old, new) {
            (TypeOwned::Struct(old), TypeOwned::Struct(new)) => {
                self.docs(&path, &old.docs, &new.docs);
                self.fields(name, (self.old, &old.fields), (self.new, &new.fields));
            }
            (TypeOwned::Enum(old), TypeOwned::Enum(new)) => {
                self.docs(&path, &old.docs, &new.docs);
                for old_variant in &old.variants {
                    let path = format!("{name}::{}", old_variant.ident);
                    let Some(new_variant) = new
                        .variants
                        .iter()
                        .find(|v| v.discriminant == old_variant.discriminant)
                    else {
                        let definition = variant_definition(self.old, old_variant);
                        self.removed(path, definition);
                        continue;
                    };
                    self.changed(
                        &path,
                        "name",
                        old_variant.ident.clone(),
                        new_variant.ident.clone(),
                    );
                    self.since(&path, old_variant.since, new_variant.since);
                    self.docs(&path, &old_variant.docs, &new_variant.docs);
                    self.fields(
                        &path,
                        (self.old, &old_variant.fields),
                        (self.new, &new_variant.fields),
                    );
                }
                for new_variant in &new.variants {
                    if !old
                        .variants
                        .iter()
                        .any(|v| v.discriminant == new_variant.discriminant)
                    {
                        let path = format!("{name}::{}", new_variant.ident);
                        let definition = variant_definition(self.new, new_variant);
                        self.added(path, definition);
                    }
                }
            }
            _ => {}
        }
    }

    fn fields(
        &mut self,
        name: &str,
        (old_bundle, old): (&ApiBundleOwned, &FieldsOwned),
        (new_bundle, new): (&ApiBundleOwned, &FieldsOwned),
    ) {
        let (old, new) = (fields(old), fields(new));
        let position = |fields: &[FieldOwned], i: usize, field: &FieldOwned| match &field.ident {
            // a relocated flag has the same name as its field
            Some(_) => fields
                .iter()
                .position(|f| f.ident == field.ident && f.is_flag() == field.is_flag()),
            None => (i < fields.len() && fields[i].ident.is_none()).then_some(i),
        };
        for (i, old_field) in old.iter().enumerate() {
            let path = format!("{name}: field {}", field_name(i, old_field));
            let Some(new_idx) = position(new, i, old_field) else {
                self.removed(path, field_definition(old_bundle, old_field));
                continue;
            };
            let new_field = &new[new_idx];
            self.changed(&path, "position", i.to_string(), new_idx.to_string());
            self.changed(
                &path,
                "type",
                type_name(old_bundle, &old_field.ty),
                type_name(new_bundle, &new_field.ty),
            );
            self.changed(
                &path,
                "#[default]",
                default(&old_field.default),
                default(&new_field.default),
            );
            self.since(&path, old_field.since, new_field.since);
            self.docs(&path, &old_field.docs, &new_field.docs);
        }
        for (i, new_field) in new.iter().enumerate() {
            if position(old, i, new_field).is_none() {
                let path = format!("{name}: field {}", field_name(i, new_field));
                self.added(path, field_definition(new_bundle, new_field));
            }
        }
    }

    /// Versions of the other crates traits and types are used from.
    fn dependencies(&mut self) {
        let deps = |bundle: &'_ ApiBundleOwned| -> BTreeMap<String, String> {
            bundle
                .ext_crates
                .iter()
                .skip(1)
                .map(|v| (v.crate_id.clone(), display(&v.version)))
                .collect()
        };
        let (old, new) = (deps(self.old), deps(self.new));
        for (crate_id, old_version) in &old {
            let path = format!("dependency {crate_id}");
            match new.get(crate_id) {
                Some(new_version) => {
                    self.changed(&path, "version", old_version.clone(), new_version.clone())
                }
                None => self.removed(path, old_version.clone()),
            }
        }
        for (crate_id, new_version) in &new {
            if !old.contains_key(crate_id) {
                self.added(format!("dependency {crate_id}"), new_version.clone());
            }
        }
    }
}

fn type_name(bundle: &ApiBundleOwned, ty: &TypeOwned) -> String {
    ty.human_name(false, bundle)
        .unwrap_or_else(|e| format!("<{e}>"))
}

/// `fn set(c: Coord) -> u8`, `rw property speed: u32 observable`, `impl gpio[]: ww_gpio::Gpio`, ...
fn item_signature(bundle: &ApiBundleOwned, item: &ApiItemOwned) -> String {
    let ident = match &item.multiplicity {
        Multiplicity::Flat => item.ident.clone(),
        Multiplicity::Array {
            index_type_idx: None,
        } => format!("{}[]", item.ident),
        Multiplicity::Array {
            index_type_idx: Some(type_idx),
        } => {
            let index_ty = TypeOwned::OutOfLine {
                type_idx: *type_idx,
            };
            format!("{}[{}]", item.ident, type_name(bundle, &index_ty))
        }
    };
    match &item.kind {
        ApiItemKindOwned::Method { args, return_ty } => {
            let args = args
                .iter()
                .map(|arg| format!("{}: {}", arg.ident, type_name(bundle, &arg.ty)))
                .collect::<Vec<_>>()
                .join(", ");
            let mut s = format!("fn {ident}({args})");
            if let Some(ty) = return_ty {
                s += &format!(" -> {}", type_name(bundle, ty));
            }
            s
        }
        ApiItemKindOwned::Property {
            ty,
            access,
            write_err_ty,
        } => {
            let (access, observe) = match access {
                PropertyAccess::Const => ("const", false),
                PropertyAccess::ReadOnly { observe } => ("ro", *observe),
                PropertyAccess::ReadWrite { observe } => ("rw", *observe),
                PropertyAccess::WriteOnly => ("wo", false),
            };
            let mut s = format!("{access} property {ident}: {}", type_name(bundle, ty));
            if let Some(err_ty) = write_err_ty {
                s += &format!(", write error: {}", type_name(bundle, err_ty));
            }
            if observe {
                s += " observable";
            }
            s
        }
        ApiItemKindOwned::Stream { ty, is_up } => {
            let kind = if *is_up { "stream" } else { "sink" };
            format!("{kind} {ident}: {}", type_name(bundle, ty))
        }
        ApiItemKindOwned::Trait { trait_idx } => {
            let crate_name = |crate_idx: u32| bundle.crate_name(crate_idx).unwrap_or("?");
            let name = match bundle.traits.get(trait_idx.0 as usize) {
                Some(ApiLevelLocationOwned::InLine { level, .. }) => {
                    format!("{}::{}", crate_name(level.crate_idx.0), level.trait_name)
                }
                Some(ApiLevelLocationOwned::SkippedFullVersion {
                    crate_idx,
                    trait_name,
                    ..
                }) => format!("{}::{trait_name}", crate_name(crate_idx.0)),
                Some(ApiLevelLocationOwned::SkippedCompactVersion {
                    version, trait_id, ..
                }) => format!("{version:?}#{}", trait_id.0),
                None => format!("<no trait with index {}>", trait_idx.0),
            };
            format!("impl {ident}: {name}")
        }
    }
}

/// `struct Name (unsized)`, `enum Name (sized, repr u4)`.
fn type_header(ty: &TypeOwned) -> String {
    let size = |size: &ElementSize| match size {
        ElementSize::Unsized => "unsized",
        ElementSize::UnsizedFinalStructure => "final_structure",
        ElementSize::SelfDescribing => "self_describing",
        ElementSize::Sized { .. } => "sized",
    };
    let kind = |fields: &FieldsOwned| match fields {
        FieldsOwned::Named(_) => "",
        FieldsOwned::Unnamed(_) => ", tuple",
        FieldsOwned::Unit => ", unit",
    };
    match ty {
        TypeOwned::Struct(s) => {
            format!("struct {} ({}{})", s.ident, size(&s.size), kind(&s.fields))
        }
        TypeOwned::Enum(e) => {
            let repr = match e.repr {
                Repr::Nibble => "nib".to_string(),
                Repr::BitAligned(bits) => format!("u{bits}"),
                Repr::UNib32 => "unib32".to_string(),
                Repr::ByteAlignedU8 => "u8".to_string(),
                Repr::ByteAlignedU16 => "u16".to_string(),
                Repr::ByteAlignedU32 => "u32".to_string(),
            };
            format!("enum {} ({}, repr {repr})", e.ident, size(&e.size))
        }
        _ => "?".into(),
    }
}

fn variant_definition(bundle: &ApiBundleOwned, variant: &ww_self::VariantOwned) -> String {
    let fields = match &variant.fields {
        FieldsOwned::Named(fields) => format!(
            " {{ {} }}",
            fields
                .iter()
                .map(|f| field_definition(bundle, f))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        FieldsOwned::Unnamed(fields) => format!(
            "({})",
            fields
                .iter()
                .map(|f| type_name(bundle, &f.ty))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        FieldsOwned::Unit => String::new(),
    };
    format!("{}{fields} = {}", variant.ident, variant.discriminant.0)
}

fn field_definition(bundle: &ApiBundleOwned, field: &FieldOwned) -> String {
    let ty = type_name(bundle, &field.ty);
    match &field.ident {
        Some(ident) => format!("{ident}: {ty}"),
        None => ty,
    }
}

fn since(since: Option<VersionTriplet>) -> String {
    since.map_or_else(|| "none".into(), |v| format!("{v:?}"))
}

fn default(default: &Option<ww_self::ValueOwned>) -> String {
    default
        .as_ref()
        .map_or_else(|| "none".into(), |v| format!("{v:?}"))
}
