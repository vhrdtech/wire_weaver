//! Human-readable resource tree of an [ApiBundleOwned], shared by `ww introspect` and `ww api tree`.

use console::style;
use shrink_wrap::ElementSize;
use std::fmt::Write;
use wire_weaver_client::ww_self::visit::{self, Visit};
use wire_weaver_client::ww_self::{
    ApiBundleOwned, ApiItemKindOwned, ApiItemOwned, ApiLevelLocationOwned, ApiLevelOwned,
    FieldsOwned, ItemEnumOwned, ItemStructOwned, Multiplicity, PropertyAccess, Repr,
    TypeLocationOwned, TypeOwned,
};

/// Render the resource tree starting from the bundle root, ending with a newline.
pub(crate) fn render(bundle: &ApiBundleOwned, skip_docs: bool) -> String {
    let mut printer = TreePrinter {
        bundle,
        skip_docs,
        out: String::new(),
        prefix: String::new(),
        is_last: false,
    };
    printer.header();
    printer.visit_api_level(&bundle.root);
    printer.out
}

/// Render all types the bundle refers to, in the order of their type index, ending with a newline.
pub(crate) fn render_types(bundle: &ApiBundleOwned, skip_docs: bool) -> String {
    let mut printer = TreePrinter {
        bundle,
        skip_docs,
        out: String::new(),
        prefix: String::new(),
        is_last: false,
    };
    printer.types();
    printer.out
}

/// One line describing the bundle contents: how many traits and types are included, and how many were left out.
pub(crate) fn summary(bundle: &ApiBundleOwned) -> String {
    let mut counter = Counter {
        bundle,
        resources: 0,
    };
    counter.visit_api_level(&bundle.root);
    let skipped_traits = bundle
        .traits
        .iter()
        .filter(|l| !matches!(l, ApiLevelLocationOwned::InLine { .. }))
        .count();
    let skipped_types = bundle
        .types
        .iter()
        .filter(|l| !matches!(l, TypeLocationOwned::InLine { .. }))
        .count();
    let mut s = format!(
        "{} resources, {} traits, {} types",
        counter.resources,
        bundle.traits.len(),
        bundle.types.len()
    );
    if skipped_traits + skipped_types > 0 {
        _ = write!(
            s,
            " ({skipped_traits} traits and {skipped_types} types not included)"
        );
    }
    s
}

struct TreePrinter<'a> {
    bundle: &'a ApiBundleOwned,
    skip_docs: bool,
    out: String,
    /// Tree guides drawn in front of the current level's items.
    prefix: String,
    /// Whether the item being visited is the last one of its level.
    is_last: bool,
}

impl<'a> Visit<'a> for TreePrinter<'a> {
    // Items are drawn by visit_api_item, which needs to know whether each one is last.
    // Level docs are shown by the header (root) or replaced by the item's own docs (nested traits).
    fn visit_api_level(&mut self, level: &'a ApiLevelOwned) {
        for (idx, item) in level.items.iter().enumerate() {
            self.is_last = idx + 1 == level.items.len();
            self.visit_api_item(item);
        }
    }

    fn visit_api_item(&mut self, item: &'a ApiItemOwned) {
        let is_last = self.is_last;
        self.guide(is_last);
        self.item_line(item);
        self.out.push('\n');

        let parent_prefix_len = self.prefix.len();
        self.prefix.push_str(if is_last { "   " } else { "│  " });
        self.visit_docs(&item.docs);
        if let ApiItemKindOwned::Trait { trait_idx } = &item.kind
            && let Some(location) = self.bundle.traits.get(trait_idx.0 as usize)
        {
            self.visit_api_level_location(location);
        }
        self.prefix.truncate(parent_prefix_len);
    }

    fn visit_docs(&mut self, docs: &'a Vec<String>) {
        if self.skip_docs {
            return;
        }
        for line in docs {
            let line = format!("/// {line}").trim_end().to_string();
            _ = writeln!(
                self.out,
                "{}{}",
                style(&self.prefix).dim(),
                style(line).green().dim()
            );
        }
    }
}

impl TreePrinter<'_> {
    fn header(&mut self) {
        let root = &self.bundle.root;
        self.visit_docs(&root.docs);
        _ = write!(
            self.out,
            "{} {}",
            style("trait").true_color(0xCF, 0x8E, 0x6D),
            style(&root.trait_name).true_color(0x8D, 0x91, 0xDC).bold()
        );
        if let Ok(version) = self.bundle.crate_version(root.crate_idx.0) {
            _ = write!(self.out, " {}", style(format!("{version:?}")).dim());
        }
        self.out.push('\n');
    }

    fn item_line(&mut self, item: &ApiItemOwned) {
        _ = write!(self.out, "{} ", style(item.id.0).dim());
        match &item.kind {
            ApiItemKindOwned::Method { args, return_ty } => {
                _ = write!(self.out, "{} ", style("fn").blue());
                self.ident(item);
                self.out.push('(');
                for (idx, arg) in args.iter().enumerate() {
                    if idx > 0 {
                        self.out.push_str(", ");
                    }
                    _ = write!(self.out, "{}: {}", arg.ident, self.ty(&arg.ty));
                }
                self.out.push(')');
                if let Some(ty) = return_ty {
                    _ = write!(self.out, " -> {}", self.ty(ty));
                }
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
                let kind = format!("{access} property");
                _ = write!(self.out, "{} ", style(kind).true_color(0xC7, 0x7D, 0xBB));
                self.ident(item);
                _ = write!(self.out, ": {}", self.ty(ty));
                if let Some(err_ty) = write_err_ty {
                    _ = write!(self.out, ", write error: {}", self.ty(err_ty));
                }
                if observe {
                    _ = write!(self.out, " {}", style("observable").dim());
                }
            }
            ApiItemKindOwned::Stream { ty, is_up } => {
                let kind = if *is_up { "stream" } else { "sink" };
                _ = write!(self.out, "{} ", style(kind).true_color(0x8C, 0xC8, 0xD4));
                self.ident(item);
                _ = write!(self.out, ": {}", self.ty(ty));
            }
            ApiItemKindOwned::Trait { trait_idx } => {
                _ = write!(self.out, "{} ", style("impl").true_color(0xCF, 0x8E, 0x6D));
                self.ident(item);
                _ = write!(self.out, ": {}", self.trait_name(trait_idx.0));
            }
        }
        if let Some(since) = &item.since {
            _ = write!(self.out, " {}", style(format!("since {since:?}")).dim());
        }
    }

    /// Resource name, followed by `[]` or `[IndexType]` for arrays of resources.
    fn ident(&mut self, item: &ApiItemOwned) {
        _ = write!(self.out, "{}", style(&item.ident).bold());
        if let Multiplicity::Array { index_type_idx } = &item.multiplicity {
            let index_ty = match index_type_idx {
                Some(type_idx) => self.ty(&TypeOwned::OutOfLine {
                    type_idx: *type_idx,
                }),
                None => String::new(),
            };
            _ = write!(self.out, "[{index_ty}]");
        }
    }

    fn ty(&self, ty: &TypeOwned) -> String {
        match ty.human_name(false, self.bundle) {
            Ok(name) => style(name).true_color(0xA6, 0xBB, 0x77).to_string(),
            Err(e) => style(format!("<{e}>")).red().to_string(),
        }
    }

    /// `crate::Trait` of a trait resource, also for traits whose definition was left out of the bundle.
    fn trait_name(&self, trait_idx: u32) -> String {
        let crate_name = |crate_idx: u32| self.bundle.crate_name(crate_idx).unwrap_or("?");
        let name = match self.bundle.traits.get(trait_idx as usize) {
            Some(ApiLevelLocationOwned::InLine { level, .. }) => {
                format!("{}::{}", crate_name(level.crate_idx.0), level.trait_name)
            }
            Some(ApiLevelLocationOwned::SkippedFullVersion {
                crate_idx,
                trait_name,
                ..
            }) => {
                let skipped = style("(definition not included)").dim();
                format!("{}::{trait_name} {skipped}", crate_name(crate_idx.0))
            }
            Some(ApiLevelLocationOwned::SkippedCompactVersion {
                version, trait_id, ..
            }) => {
                let skipped = style("(definition not included)").dim();
                format!("{version:?}#{} {skipped}", trait_id.0)
            }
            None => {
                return style(format!("<no trait with index {trait_idx}>"))
                    .red()
                    .to_string();
            }
        };
        style(name).true_color(0x8D, 0x91, 0xDC).to_string()
    }
}

impl<'a> TreePrinter<'a> {
    fn types(&mut self) {
        _ = writeln!(self.out, "{}", style("types").true_color(0xCF, 0x8E, 0x6D));
        let types = &self.bundle.types;
        for (idx, location) in types.iter().enumerate() {
            let is_last = idx + 1 == types.len();
            self.guide(is_last);
            _ = write!(self.out, "{} ", style(idx).dim());
            let parent_prefix_len = self.prefix.len();
            self.prefix.push_str(if is_last { "   " } else { "│  " });
            match location {
                TypeLocationOwned::InLine {
                    ty: TypeOwned::Struct(item_struct),
                    ..
                } => self.item_struct(item_struct),
                TypeLocationOwned::InLine {
                    ty: TypeOwned::Enum(item_enum),
                    ..
                } => self.item_enum(item_enum),
                TypeLocationOwned::InLine { ty, .. } => {
                    _ = writeln!(self.out, "{}", self.ty(ty));
                }
                TypeLocationOwned::SkippedFullVersion {
                    crate_idx,
                    type_name,
                    ..
                } => {
                    let crate_name = self.bundle.crate_name(crate_idx.0).unwrap_or("?");
                    _ = writeln!(
                        self.out,
                        "{} {}",
                        style(format!("{crate_name}::{type_name}")).true_color(0xA6, 0xBB, 0x77),
                        style("(definition not included)").dim()
                    );
                }
            }
            self.prefix.truncate(parent_prefix_len);
        }
    }

    fn item_struct(&mut self, item_struct: &'a ItemStructOwned) {
        _ = write!(self.out, "{} ", style("struct").blue());
        self.type_ident(item_struct.crate_idx.0, &item_struct.ident);
        self.unnamed_fields(&item_struct.fields);
        _ = writeln!(self.out, " {}", style(size_name(&item_struct.size)).dim());
        self.visit_docs(&item_struct.docs);
        self.fields(&item_struct.fields);
    }

    fn item_enum(&mut self, item_enum: &'a ItemEnumOwned) {
        _ = write!(self.out, "{} ", style("enum").blue());
        self.type_ident(item_enum.crate_idx.0, &item_enum.ident);
        let repr = match item_enum.repr {
            Repr::Nibble => "nib".to_string(),
            Repr::BitAligned(bits) => format!("u{bits}"),
            Repr::UNib32 => "unib32".to_string(),
            Repr::ByteAlignedU8 => "u8".to_string(),
            Repr::ByteAlignedU16 => "u16".to_string(),
            Repr::ByteAlignedU32 => "u32".to_string(),
        };
        let size = size_name(&item_enum.size);
        _ = writeln!(self.out, " {}", style(format!("{size}, repr {repr}")).dim());
        self.visit_docs(&item_enum.docs);
        for (idx, variant) in item_enum.variants.iter().enumerate() {
            let is_last = idx + 1 == item_enum.variants.len();
            self.guide(is_last);
            _ = write!(
                self.out,
                "{} {}",
                style(variant.discriminant.0).dim(),
                style(&variant.ident).bold()
            );
            self.unnamed_fields(&variant.fields);
            if let Some(since) = &variant.since {
                _ = write!(self.out, " {}", style(format!("since {since:?}")).dim());
            }
            self.out.push('\n');
            let parent_prefix_len = self.prefix.len();
            self.prefix.push_str(if is_last { "   " } else { "│  " });
            self.visit_docs(&variant.docs);
            self.fields(&variant.fields);
            self.prefix.truncate(parent_prefix_len);
        }
    }

    /// Tuple fields, `(T1, T2)`, on the same line as the struct or variant name.
    fn unnamed_fields(&mut self, fields: &FieldsOwned) {
        let FieldsOwned::Unnamed(fields) = fields else {
            return;
        };
        let types = fields
            .iter()
            .map(|field| self.ty(&field.ty))
            .collect::<Vec<_>>()
            .join(", ");
        _ = write!(self.out, "({types})");
    }

    /// Named fields, one per line.
    fn fields(&mut self, fields: &'a FieldsOwned) {
        let FieldsOwned::Named(fields) = fields else {
            return;
        };
        for (idx, field) in fields.iter().enumerate() {
            let is_last = idx + 1 == fields.len();
            self.guide(is_last);
            let ident = field.ident.as_deref().unwrap_or("?");
            _ = write!(self.out, "{}: {}", style(ident).bold(), self.ty(&field.ty));
            if field.default.is_some() {
                _ = write!(self.out, " {}", style("has default").dim());
            }
            if let Some(since) = &field.since {
                _ = write!(self.out, " {}", style(format!("since {since:?}")).dim());
            }
            self.out.push('\n');
            let parent_prefix_len = self.prefix.len();
            self.prefix.push_str(if is_last { "   " } else { "│  " });
            self.visit_docs(&field.docs);
            self.prefix.truncate(parent_prefix_len);
        }
    }

    /// `crate::Name` of a struct or enum.
    fn type_ident(&mut self, crate_idx: u32, ident: &str) {
        let crate_name = self.bundle.crate_name(crate_idx).unwrap_or("?");
        _ = write!(
            self.out,
            "{}{}",
            style(format!("{crate_name}::")).dim(),
            style(ident).true_color(0xA6, 0xBB, 0x77).bold()
        );
    }

    /// Tree guide in front of an item, `is_last` of its level.
    fn guide(&mut self, is_last: bool) {
        let guide = if is_last { "└─ " } else { "├─ " };
        _ = write!(
            self.out,
            "{}{}",
            style(&self.prefix).dim(),
            style(guide).dim()
        );
    }
}

fn size_name(size: &ElementSize) -> &'static str {
    // size_bits is not shown, as it's not always accurate for enums
    match size {
        ElementSize::Unsized => "unsized",
        ElementSize::UnsizedFinalStructure => "final structure",
        ElementSize::SelfDescribing => "self-describing",
        ElementSize::Sized { .. } => "sized",
    }
}

/// Counts resources reachable from the root, following trait resources into their definitions.
struct Counter<'a> {
    bundle: &'a ApiBundleOwned,
    resources: usize,
}

impl<'a> Visit<'a> for Counter<'a> {
    fn visit_api_item(&mut self, item: &'a ApiItemOwned) {
        self.resources += 1;
        if let ApiItemKindOwned::Trait { trait_idx } = &item.kind
            && let Some(location) = self.bundle.traits.get(trait_idx.0 as usize)
        {
            visit::visit_api_level_location(self, location);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn traits_api() -> ApiBundleOwned {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/traits_api");
        wire_weaver_core::load(&path, Some("Traits".into()), false).unwrap()
    }

    #[test]
    fn renders_nested_traits() {
        console::set_colors_enabled(false);
        let tree = render(&traits_api(), false);
        let expected = "\
trait Traits traits_api@0.1.0
├─ 0 impl g1: traits_api::Subgroup
│  └─ 0 fn m1()
├─ 1 impl gpio[]: traits_api::Gpio
│  └─ 0 fn set_high()
└─ 2 impl periph[]: traits_api::Peripheral
   └─ 0 impl channel[]: traits_api::Channel
      ├─ 0 rw property gain: f32
      └─ 1 fn run()
";
        assert_eq!(tree, expected);
        assert_eq!(summary(&traits_api()), "8 resources, 4 traits, 0 types");
    }

    #[test]
    fn renders_types() {
        console::set_colors_enabled(false);
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/properties_api");
        let mut bundle = wire_weaver_core::load(&path, None, false).unwrap();
        let expected = "\
types
├─ 0 struct properties_api::Inner unsized
│  ├─ u: u8
│  └─ v: String
└─ 1 struct properties_api::Custom unsized
   ├─ z: u8
   └─ inner: Vec<Inner>
";
        assert_eq!(render_types(&bundle, false), expected);

        // as sent by a device, with a definition left out
        bundle.types[0] = TypeLocationOwned::SkippedFullVersion {
            crate_idx: bundle.root.crate_idx,
            type_name: "Inner".into(),
            signature: vec![],
        };
        let types = render_types(&bundle, false);
        assert!(
            types.contains("├─ 0 properties_api::Inner (definition not included)\n"),
            "{types}"
        );
        assert!(types.contains("└─ inner: Vec<Inner>\n"), "{types}");
    }

    #[test]
    fn renders_skipped_trait() {
        console::set_colors_enabled(false);
        let mut bundle = traits_api();
        let gpio_idx = bundle
            .traits
            .iter()
            .position(|l| matches!(l, ApiLevelLocationOwned::InLine { level, .. } if level.trait_name == "Gpio"))
            .unwrap();
        bundle.traits[gpio_idx] = ApiLevelLocationOwned::SkippedFullVersion {
            crate_idx: bundle.root.crate_idx,
            trait_name: "Gpio".into(),
            signature: vec![],
        };
        let tree = render(&bundle, false);
        assert!(
            tree.contains("├─ 1 impl gpio[]: traits_api::Gpio (definition not included)\n└─ 2"),
            "{tree}"
        );
        assert_eq!(
            summary(&bundle),
            "7 resources, 4 traits, 0 types (1 traits and 0 types not included)"
        );
    }
}
