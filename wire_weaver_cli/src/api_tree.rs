//! Human-readable resource tree of an [ApiBundleOwned], shared by `ww introspect` and `ww api tree`.

use console::style;
use shrink_wrap::ElementSize;
use std::collections::HashMap;
use std::fmt::Write;
use wire_weaver_client::ww_self::errors::{error_list_docs, fmt_error_list};
use wire_weaver_client::ww_self::visit::{self, Visit};
use wire_weaver_client::ww_self::{
    ApiBundleOwned, ApiItemKindOwned, ApiItemOwned, ApiLevelLocationOwned, ApiLevelOwned, Delivery,
    FieldsOwned, FlowControl, GenericParamOwned, ItemEnumOwned, ItemStructOwned, Multiplicity,
    PropertyAccess, Repr, TypeLocationOwned, TypeOwned,
};

/// Render the resource tree starting from the bundle root, ending with a newline.
pub(crate) fn render(bundle: &ApiBundleOwned, skip_docs: bool) -> String {
    render_with_values(bundle, skip_docs, &HashMap::new())
}

/// [render], with `= value` after the `const` items in `values`, by resource ids from the root.
pub(crate) fn render_with_values(
    bundle: &ApiBundleOwned,
    skip_docs: bool,
    values: &HashMap<Vec<u32>, String>,
) -> String {
    let mut printer = TreePrinter {
        bundle,
        skip_docs,
        out: String::new(),
        prefix: String::new(),
        is_last: false,
        omitted: ids(&bundle.root_omitted),
        path: vec![],
        values,
        params: &[],
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
        omitted: vec![],
        path: vec![],
        values: &HashMap::new(),
        params: &[],
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
    /// Ids of the current level's optional items the device leaves out (API-16).
    omitted: Vec<u32>,
    /// Resource ids from the root to the item being visited
    path: Vec<u32>,
    /// Values of `const` items read from a device, shown after their type
    values: &'a HashMap<Vec<u32>, String>,
    /// Parameters of the generic definition being printed, to name them in its fields (API-43).
    params: &'a [GenericParamOwned],
}

fn ids(omitted: &Option<Vec<shrink_wrap::UNib32>>) -> Vec<u32> {
    omitted.iter().flatten().map(|id| id.0).collect()
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
        self.path.push(item.id.0);
        self.guide(is_last);
        self.item_line(item);
        self.out.push('\n');

        let parent_prefix_len = self.prefix.len();
        self.prefix.push_str(if is_last { "   " } else { "│  " });
        self.visit_docs(&item.docs);
        self.error_list_docs(item);
        if let ApiItemKindOwned::Trait { trait_idx, omitted } = &item.kind
            && let Some(location) = self.bundle.traits.get(trait_idx.0 as usize)
        {
            let parent_omitted = std::mem::replace(&mut self.omitted, ids(omitted));
            self.visit_api_level_location(location);
            self.omitted = parent_omitted;
        }
        self.prefix.truncate(parent_prefix_len);
        self.path.pop();
    }

    fn visit_docs(&mut self, docs: &'a Vec<String>) {
        self.doc_lines(docs);
    }
}

impl TreePrinter<'_> {
    fn doc_lines(&mut self, docs: &[String]) {
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
    /// `# Errors` with each entry of the `#[errors(..)]` of a method, stream function or property and its variant's
    /// doc line (API-32), like the docs of the generated client.
    fn error_list_docs(&mut self, item: &ApiItemOwned) {
        let (Some(errors), Some(err_ty)) = (item.kind.error_list(), item.kind.err_ty(self.bundle))
        else {
            return;
        };
        let mut lines = vec![];
        if !item.docs.is_empty() {
            lines.push(String::new());
        }
        lines.push("# Errors".to_string());
        for (name, doc) in error_list_docs(errors, err_ty, self.bundle) {
            lines.push(match doc {
                Some(doc) => format!("- `{name}`: {doc}"),
                None => format!("- `{name}`"),
            });
        }
        self.doc_lines(&lines);
    }

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
        if let Some(optional) = &item.optional {
            let attr = match &optional.group {
                Some(group) => format!("#[optional({group})] "),
                None => "#[optional] ".to_string(),
            };
            _ = write!(self.out, "{}", style(attr).dim());
        }
        if let Some(errors) = item.kind.error_list() {
            let attr = format!("#[errors({})] ", fmt_error_list(errors));
            _ = write!(self.out, "{}", style(attr).dim());
        }
        match &item.kind {
            ApiItemKindOwned::Method {
                args, return_ty, ..
            } => {
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
                ..
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
                if let Some(value) = self.values.get(&self.path) {
                    _ = write!(self.out, " = {value}");
                }
            }
            ApiItemKindOwned::Stream { ty, is_up } => {
                let kind = if *is_up { "stream" } else { "sink" };
                _ = write!(self.out, "{} ", style(kind).true_color(0x8C, 0xC8, 0xD4));
                self.ident(item);
                _ = write!(self.out, ": {}", self.ty(ty));
            }
            ApiItemKindOwned::FlowSink { ty, flow } => {
                let flow = match flow {
                    FlowControl::Credit => "#[credit] ",
                };
                _ = write!(self.out, "{}", style(flow).dim());
                _ = write!(self.out, "{} ", style("sink").true_color(0x8C, 0xC8, 0xD4));
                self.ident(item);
                _ = write!(self.out, ": {}", self.ty(ty));
            }
            ApiItemKindOwned::StreamFn {
                ty,
                delivery,
                args,
                return_ty,
                ..
            } => {
                let delivery = match delivery {
                    Delivery::Plain => "",
                    Delivery::Counted => "#[counted] ",
                    Delivery::Reliable => "#[reliable] ",
                };
                _ = write!(self.out, "{}", style(delivery).dim());
                _ = write!(
                    self.out,
                    "{} ",
                    style("stream").true_color(0x8C, 0xC8, 0xD4)
                );
                self.ident(item);
                let args: Vec<String> = args
                    .iter()
                    .map(|arg| format!("{}: {}", arg.ident, self.ty(&arg.ty)))
                    .collect();
                _ = write!(self.out, "({}): {}", args.join(", "), self.ty(ty));
                if let Some(return_ty) = return_ty {
                    _ = write!(self.out, " -> {}", self.ty(return_ty));
                }
            }
            ApiItemKindOwned::Trait { trait_idx, .. } => {
                _ = write!(self.out, "{} ", style("impl").true_color(0xCF, 0x8E, 0x6D));
                self.ident(item);
                _ = write!(self.out, ": {}", self.trait_name(trait_idx.0));
            }
        }
        if let Some(since) = &item.since {
            _ = write!(self.out, " {}", style(format!("since {since:?}")).dim());
        }
        if self.omitted.contains(&item.id.0) {
            _ = write!(
                self.out,
                " {}",
                style("(left out by this device)").red().dim()
            );
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
        match ty.human_name_in(false, self.bundle, self.params) {
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
                } => {
                    let ty = TypeOwned::OutOfLine {
                        type_idx: (idx as u32).into(),
                    };
                    self.item_enum(item_enum, ty.is_common_error(self.bundle))
                }
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
        self.params = &item_struct.generics;
        self.generics();
        self.unnamed_fields(&item_struct.fields);
        _ = writeln!(self.out, " {}", style(size_name(&item_struct.size)).dim());
        self.visit_docs(&item_struct.docs);
        self.fields(&item_struct.fields);
        self.params = &[];
    }

    /// `<T, const N: u64>` of a generic definition.
    fn generics(&mut self) {
        match wire_weaver_client::ww_self::generics::params_human_name(self.params, self.bundle) {
            Ok(params) => _ = write!(self.out, "{}", style(params).true_color(0xA6, 0xBB, 0x77)),
            Err(e) => _ = write!(self.out, "{}", style(format!("<{e}>")).red()),
        }
    }

    /// `is_common`: `ww_error::CommonError`, the errors every API can return (STD-16).
    fn item_enum(&mut self, item_enum: &'a ItemEnumOwned, is_common: bool) {
        _ = write!(self.out, "{} ", style("enum").blue());
        self.type_ident(item_enum.crate_idx.0, &item_enum.ident);
        self.params = &item_enum.generics;
        self.generics();
        let repr = match item_enum.repr {
            Repr::Nibble => "nib".to_string(),
            Repr::BitAligned(bits) => format!("u{bits}"),
            Repr::UNib32 => "unib32".to_string(),
            Repr::ByteAlignedU8 => "u8".to_string(),
            Repr::ByteAlignedU16 => "u16".to_string(),
            Repr::ByteAlignedU32 => "u32".to_string(),
        };
        let size = size_name(&item_enum.size);
        let common = if is_common { ", common errors" } else { "" };
        _ = writeln!(
            self.out,
            " {}",
            style(format!("{size}, repr {repr}{common}")).dim()
        );
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
            if matches!(&variant.fields, FieldsOwned::Unnamed(f) if f.len() == 1 && f[0].ty.is_common_error(self.bundle))
            {
                _ = write!(self.out, " {}", style("common errors").dim());
            }
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
        self.params = &[];
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
        if let ApiItemKindOwned::Trait { trait_idx, .. } = &item.kind
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
    fn renders_const_values() {
        console::set_colors_enabled(false);
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/consts_api");
        let bundle = wire_weaver_core::load(&path, Some("ConstsTest".into()), false).unwrap();
        let values = HashMap::from([
            (vec![1], "\"E661A410\"".to_string()),
            (vec![4, 0], "\"info\"".to_string()),
        ]);
        let tree = render_with_values(&bundle, true, &values);
        assert!(
            tree.contains("├─ 1 const property SERIAL: String = \"E661A410\"\n"),
            "{tree}"
        );
        assert!(
            tree.contains("└─ 0 const property NAME: String = \"info\"\n"),
            "{tree}"
        );
        assert!(tree.contains("├─ 0 const property LIMITS: "), "{tree}");
        assert!(!tree.contains("LIMITS: Limits ="), "{tree}");
    }

    #[test]
    fn renders_type_aliases() {
        console::set_colors_enabled(false);
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/generics_api");
        let bundle = wire_weaver_core::load(&path, Some("Generics".into()), false).unwrap();
        let tree = render(&bundle, false);
        // `type Volt = Quantity<units::Volt>`, `type Sample<T> = Pair<Window<2>, Reading<T>>`: what they name
        for line in [
            "├─ 6 fn double(v: Quantity<Volt>) -> Quantity<Volt>\n",
            "├─ 7 rw property voltage: Quantity<Volt>\n",
            "├─ 8 rw property timed_voltage: Timed<Quantity<Volt>>\n",
            "└─ 9 rw property sample: Pair<Window<2>, Reading<u8>>\n",
        ] {
            assert!(tree.contains(line), "{tree}");
        }
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
├─ 2 impl periph[]: traits_api::Peripheral
│  └─ 0 impl channel[]: traits_api::Channel
│     ├─ 0 rw property gain: f32
│     └─ 1 fn run()
├─ 3 impl i2c: traits_api::I2c
│  ├─ 0 fn i2c_write(data: u8) -> u8
│  ├─ 1 #[optional] fn i2c_wiggle_clock(count: u8) -> u8
│  │  /// Clocks SCL until SDA is released.
│  ├─ 2 #[optional(pullups)] fn i2c_set_pullups(enabled: bool) -> Result<(), u8>
│  │  /// Switchable pull-ups: both items or neither.
│  │  /// Fallible: an omitted method with an error type gives `CallError::Unimplemented` on the client.
│  ├─ 3 #[optional(pullups)] rw property i2c_pullups: bool
│  └─ 4 #[optional] stream i2c_monitor: u8
└─ 4 #[optional] fn reset()
   /// Optional item of the root level itself.
";
        assert_eq!(tree, expected);
        assert_eq!(summary(&traits_api()), "15 resources, 5 traits, 0 types");
    }

    #[test]
    fn renders_omitted_items() {
        console::set_colors_enabled(false);
        let mut bundle = traits_api();
        bundle.root_omitted = Some(vec![shrink_wrap::UNib32(4)]);
        let i2c = bundle
            .root
            .items
            .iter_mut()
            .find(|i| i.ident == "i2c")
            .unwrap();
        if let ApiItemKindOwned::Trait { omitted, .. } = &mut i2c.kind {
            *omitted = Some(vec![shrink_wrap::UNib32(2), shrink_wrap::UNib32(3)]);
        }
        let tree = render(&bundle, true);
        for line in [
            "├─ 1 #[optional] fn i2c_wiggle_clock(count: u8) -> u8\n",
            "├─ 2 #[optional(pullups)] fn i2c_set_pullups(enabled: bool) -> Result<(), u8> (left out by this device)\n",
            "├─ 3 #[optional(pullups)] rw property i2c_pullups: bool (left out by this device)\n",
            "└─ 4 #[optional] fn reset() (left out by this device)\n",
        ] {
            assert!(tree.contains(line), "{line}{tree}");
        }
    }

    /// `#[errors(..)]` of a property and a stream function (API-32), with its `# Errors`.
    #[test]
    fn renders_error_lists_of_properties_and_stream_fns() {
        console::set_colors_enabled(false);
        let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests");
        let bundle = wire_weaver_core::load(
            &tests.join("properties_api"),
            Some("Properties".into()),
            false,
        )
        .unwrap();
        let tree = render(&bundle, false);
        for line in [
            "#[errors(TooBig, NotSet)] rw property mode: u8, write error: ModeError\n",
            "/// # Errors\n",
            "/// - `NotSet`: Not set yet\n",
        ] {
            assert!(tree.contains(line), "{line}{tree}");
        }
        let bundle =
            wire_weaver_core::load(&tests.join("streams_reliable_api"), None, false).unwrap();
        let tree = render(&bundle, false);
        assert!(
            tree.contains("#[errors(BadRate)] #[reliable] stream samples(config: "),
            "{tree}"
        );
        assert!(tree.contains("/// - `BadRate`: Rate 0\n"), "{tree}");
    }

    #[test]
    fn renders_types() {
        console::set_colors_enabled(false);
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/properties_api");
        let mut bundle = wire_weaver_core::load(&path, Some("Properties".into()), false).unwrap();
        let expected = "\
types
├─ 0 enum properties_api::ModeError unsized, repr u4
│  ├─ 0 TooBig
│  │  /// Above 10.
│  ├─ 1 NotSet
│  │  /// Not set yet.
│  └─ 2 Locked
│     /// Not in the list of `mode`, returned for 99 to test the client's warning.
├─ 1 struct properties_api::Inner unsized
│  ├─ u: u8
│  └─ v: String
└─ 2 struct properties_api::Custom unsized
   ├─ z: u8
   └─ inner: Vec<Inner>
";
        assert_eq!(render_types(&bundle, false), expected);

        // as sent by a device, with a definition left out
        bundle.types[1] = TypeLocationOwned::SkippedFullVersion {
            crate_idx: bundle.root.crate_idx,
            type_name: "Inner".into(),
            signature: vec![],
        };
        let types = render_types(&bundle, false);
        assert!(
            types.contains("├─ 1 properties_api::Inner (definition not included)\n"),
            "{types}"
        );
        assert!(types.contains("└─ inner: Vec<Inner>\n"), "{types}");
    }

    #[test]
    fn renders_recursive_types() {
        console::set_colors_enabled(false);
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/recursive_api");
        let mut bundle = wire_weaver_core::load(&path, Some("Recursive".into()), false).unwrap();
        // as introspection data is used
        wire_weaver_client::ww_self::generics::prepare(&mut bundle).unwrap();
        let expected = "\
trait Recursive recursive_api@0.1.0
├─ 0 fn eval(expr: Expr) -> Option<i32>
│  /// Value of the expression, `None` when it is nested too deep or has a variant from a newer version
├─ 1 fn unwrap_neg(expr: Expr) -> Option<Expr>
│  /// The expression with its outermost negation removed
├─ 2 rw property tree: Tree
└─ 3 stream exprs: Expr
";
        assert_eq!(render(&bundle, false), expected);
        let expected = "\
types
├─ 0 enum recursive_api::Expr unsized, repr u4
│  /// An integer expression
│  ├─ 0 Add(Box<Expr>, Box<Expr>)
│  ├─ 1 Mul
│  │  ├─ a: Box<Expr>
│  │  └─ b: Box<Expr>
│  ├─ 2 Neg(Box<Expr>)
│  └─ 3 Lit(i32)
└─ 1 struct recursive_api::Tree unsized
   /// A labelled tree, children in a list
   ├─ label: String
   └─ children: Vec<Tree>
";
        assert_eq!(render_types(&bundle, false), expected);
    }

    #[test]
    fn renders_common_errors() {
        console::set_colors_enabled(false);
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/common_error_api");
        let bundle = wire_weaver_core::load(&path, None, false).unwrap();
        let types = render_types(&bundle, true);
        assert!(types.contains("enum ww_error::CommonError unsized, repr unib32, common errors\n"));
        assert!(types.contains("├─ 0 Common(CommonError) common errors\n"));
        assert!(types.contains("└─ 1 Nack\n"));
    }

    #[test]
    fn renders_error_lists() {
        console::set_colors_enabled(false);
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/methods_api");
        let bundle = wire_weaver_core::load(&path, Some("Methods".into()), false).unwrap();
        let tree = render(&bundle, false);
        let expected = "\
├─ 13 #[errors(Common(Busy, InUse), Nack)] fn listed_errors(pick: u8) -> Result<(), ListedError>
│  /// Errors from a list (API-32): `pick` 0 is `Ok`, 1 `Common(Busy)`, 2 `Nack`, 3 `Common(Timeout)` and
│  /// 4 `Overrun`, which are not in the list
│  ///
│  /// # Errors
│  /// - `Common(Busy)`: Busy with another operation, try again later
│  /// - `Common(InUse)`: In use by {by}
│  /// - `Nack`: The sensor did not answer
├─ 14 #[errors(Common)] fn all_common(pick: u8) -> Result<(), ListedError>
│  /// Any `CommonError` (API-32): `Common` lists all of the nested enum's variants
│  ///
│  /// # Errors
│  /// - `Common`: any CommonError
";
        assert!(tree.contains(expected), "{tree}");
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
            tree.contains("├─ 1 impl gpio[]: traits_api::Gpio (definition not included)\n├─ 2"),
            "{tree}"
        );
        assert_eq!(
            summary(&bundle),
            "14 resources, 5 traits, 0 types (1 traits and 0 types not included)"
        );
    }
}
