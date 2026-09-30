//! Human-readable resource tree of an [ApiBundleOwned], shared by `ww introspect` and `ww api tree`.

use console::style;
use std::fmt::Write;
use wire_weaver_client::ww_self::visit::{self, Visit};
use wire_weaver_client::ww_self::{
    ApiBundleOwned, ApiItemKindOwned, ApiItemOwned, ApiLevelLocationOwned, ApiLevelOwned,
    Multiplicity, PropertyAccess, TypeLocationOwned, TypeOwned,
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
        let guide = if is_last { "└─ " } else { "├─ " };
        _ = write!(
            self.out,
            "{}{}",
            style(&self.prefix).dim(),
            style(guide).dim()
        );
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
