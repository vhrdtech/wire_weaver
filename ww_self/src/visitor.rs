//! Generates [`visit`] and [`visit_mut`] from a single definition.

macro_rules! define_visitor {
    (
        $(#[$trait_meta:meta])*
        trait $Trait:ident $(<$lt:lifetime>)?,
        ref = & $($lt_ref:lifetime)? $($mut:ident)?
    ) => {
        use crate::{
            ApiBundleOwned, ApiItemKindOwned, ApiItemOwned, ApiLevelLocationOwned, ApiLevelOwned,
            ArgumentOwned, FieldOwned, FieldsOwned, ItemEnumOwned, ItemStructOwned, TypeLocationOwned,
            TypeOwned, VariantOwned,
        };

        $(#[$trait_meta])*
        #[allow(clippy::ptr_arg)]
        pub trait $Trait $(<$lt>)? {
            fn visit_api_bundle(&mut self, node: & $($lt_ref)? $($mut)? ApiBundleOwned) {
                visit_api_bundle(self, node)
            }

            fn visit_type_location(&mut self, node: & $($lt_ref)? $($mut)? TypeLocationOwned) {
                visit_type_location(self, node)
            }

            fn visit_api_level_location(&mut self, node: & $($lt_ref)? $($mut)? ApiLevelLocationOwned) {
                visit_api_level_location(self, node)
            }

            fn visit_api_level(&mut self, node: & $($lt_ref)? $($mut)? ApiLevelOwned) {
                visit_api_level(self, node)
            }

            fn visit_api_item(&mut self, node: & $($lt_ref)? $($mut)? ApiItemOwned) {
                visit_api_item(self, node)
            }

            fn visit_api_item_kind(&mut self, node: & $($lt_ref)? $($mut)? ApiItemKindOwned) {
                visit_api_item_kind(self, node)
            }

            fn visit_argument(&mut self, node: & $($lt_ref)? $($mut)? ArgumentOwned) {
                visit_argument(self, node)
            }

            fn visit_type(&mut self, node: & $($lt_ref)? $($mut)? TypeOwned) {
                visit_type(self, node)
            }

            fn visit_item_struct(&mut self, node: & $($lt_ref)? $($mut)? ItemStructOwned) {
                visit_item_struct(self, node)
            }

            fn visit_item_enum(&mut self, node: & $($lt_ref)? $($mut)? ItemEnumOwned) {
                visit_item_enum(self, node)
            }

            fn visit_variant(&mut self, node: & $($lt_ref)? $($mut)? VariantOwned) {
                visit_variant(self, node)
            }

            fn visit_fields(&mut self, node: & $($lt_ref)? $($mut)? FieldsOwned) {
                visit_fields(self, node)
            }

            fn visit_field(&mut self, node: & $($lt_ref)? $($mut)? FieldOwned) {
                visit_field(self, node)
            }

            /// Leaf: name of a declared trait, item, argument, struct, enum, variant or field.
            fn visit_ident(&mut self, node: & $($lt_ref)? $($mut)? String) {
                let _ = node;
            }

            /// Leaf: doc comment lines attached to a trait, item, struct, enum, variant or field.
            fn visit_docs(&mut self, node: & $($lt_ref)? $($mut)? Vec<String>) {
                let _ = node;
            }
        }

        pub fn visit_api_bundle<$($lt,)? V: $Trait $(<$lt>)? + ?Sized>(
            v: &mut V,
            node: & $($lt_ref)? $($mut)? ApiBundleOwned,
        ) {
            v.visit_api_level(& $($mut)? node.root);
            for location in & $($mut)? node.types {
                v.visit_type_location(location);
            }
            for location in & $($mut)? node.traits {
                v.visit_api_level_location(location);
            }
        }

        pub fn visit_type_location<$($lt,)? V: $Trait $(<$lt>)? + ?Sized>(
            v: &mut V,
            node: & $($lt_ref)? $($mut)? TypeLocationOwned,
        ) {
            match node {
                TypeLocationOwned::InLine { ty, .. } => v.visit_type(ty),
                TypeLocationOwned::SkippedFullVersion { .. } => {}
            }
        }

        pub fn visit_api_level_location<$($lt,)? V: $Trait $(<$lt>)? + ?Sized>(
            v: &mut V,
            node: & $($lt_ref)? $($mut)? ApiLevelLocationOwned,
        ) {
            match node {
                ApiLevelLocationOwned::InLine { level, .. } => v.visit_api_level(level),
                ApiLevelLocationOwned::SkippedFullVersion { .. }
                | ApiLevelLocationOwned::SkippedCompactVersion { .. } => {}
            }
        }

        pub fn visit_api_level<$($lt,)? V: $Trait $(<$lt>)? + ?Sized>(
            v: &mut V,
            node: & $($lt_ref)? $($mut)? ApiLevelOwned,
        ) {
            v.visit_docs(& $($mut)? node.docs);
            v.visit_ident(& $($mut)? node.trait_name);
            for item in & $($mut)? node.items {
                v.visit_api_item(item);
            }
        }

        pub fn visit_api_item<$($lt,)? V: $Trait $(<$lt>)? + ?Sized>(
            v: &mut V,
            node: & $($lt_ref)? $($mut)? ApiItemOwned,
        ) {
            v.visit_docs(& $($mut)? node.docs);
            v.visit_ident(& $($mut)? node.ident);
            v.visit_api_item_kind(& $($mut)? node.kind);
        }

        pub fn visit_api_item_kind<$($lt,)? V: $Trait $(<$lt>)? + ?Sized>(
            v: &mut V,
            node: & $($lt_ref)? $($mut)? ApiItemKindOwned,
        ) {
            match node {
                ApiItemKindOwned::Method { args, return_ty } => {
                    for arg in args {
                        v.visit_argument(arg);
                    }
                    if let Some(ty) = return_ty {
                        v.visit_type(ty);
                    }
                }
                ApiItemKindOwned::Property { ty, write_err_ty, .. } => {
                    v.visit_type(ty);
                    if let Some(ty) = write_err_ty {
                        v.visit_type(ty);
                    }
                }
                ApiItemKindOwned::Stream { ty, .. } => v.visit_type(ty),
                ApiItemKindOwned::Trait { .. } => {}
            }
        }

        pub fn visit_argument<$($lt,)? V: $Trait $(<$lt>)? + ?Sized>(
            v: &mut V,
            node: & $($lt_ref)? $($mut)? ArgumentOwned,
        ) {
            v.visit_ident(& $($mut)? node.ident);
            v.visit_type(& $($mut)? node.ty);
        }

        pub fn visit_type<$($lt,)? V: $Trait $(<$lt>)? + ?Sized>(
            v: &mut V,
            node: & $($lt_ref)? $($mut)? TypeOwned,
        ) {
            match node {
                TypeOwned::Vec(ty)
                | TypeOwned::Array { ty, .. }
                | TypeOwned::Option { some_ty: ty }
                | TypeOwned::Box(ty) => v.visit_type(ty),
                TypeOwned::Result { ok_ty, err_ty } => {
                    v.visit_type(ok_ty);
                    v.visit_type(err_ty);
                }
                TypeOwned::Tuple(types) => {
                    for ty in types {
                        v.visit_type(ty);
                    }
                }
                TypeOwned::Struct(item_struct) => v.visit_item_struct(item_struct),
                TypeOwned::Enum(item_enum) => v.visit_item_enum(item_enum),
                TypeOwned::Bool
                | TypeOwned::NumericAny(_)
                | TypeOwned::OutOfLine { .. }
                | TypeOwned::Flag
                | TypeOwned::String
                | TypeOwned::Range(_)
                | TypeOwned::RangeInclusive(_) => {}
            }
        }

        pub fn visit_item_struct<$($lt,)? V: $Trait $(<$lt>)? + ?Sized>(
            v: &mut V,
            node: & $($lt_ref)? $($mut)? ItemStructOwned,
        ) {
            v.visit_docs(& $($mut)? node.docs);
            v.visit_ident(& $($mut)? node.ident);
            v.visit_fields(& $($mut)? node.fields);
        }

        pub fn visit_item_enum<$($lt,)? V: $Trait $(<$lt>)? + ?Sized>(
            v: &mut V,
            node: & $($lt_ref)? $($mut)? ItemEnumOwned,
        ) {
            v.visit_docs(& $($mut)? node.docs);
            v.visit_ident(& $($mut)? node.ident);
            for variant in & $($mut)? node.variants {
                v.visit_variant(variant);
            }
        }

        pub fn visit_variant<$($lt,)? V: $Trait $(<$lt>)? + ?Sized>(
            v: &mut V,
            node: & $($lt_ref)? $($mut)? VariantOwned,
        ) {
            v.visit_docs(& $($mut)? node.docs);
            v.visit_ident(& $($mut)? node.ident);
            v.visit_fields(& $($mut)? node.fields);
        }

        pub fn visit_fields<$($lt,)? V: $Trait $(<$lt>)? + ?Sized>(
            v: &mut V,
            node: & $($lt_ref)? $($mut)? FieldsOwned,
        ) {
            match node {
                FieldsOwned::Named(fields) | FieldsOwned::Unnamed(fields) => {
                    for field in fields {
                        v.visit_field(field);
                    }
                }
                FieldsOwned::Unit => {}
            }
        }

        pub fn visit_field<$($lt,)? V: $Trait $(<$lt>)? + ?Sized>(
            v: &mut V,
            node: & $($lt_ref)? $($mut)? FieldOwned,
        ) {
            v.visit_docs(& $($mut)? node.docs);
            if let Some(ident) = & $($mut)? node.ident {
                v.visit_ident(ident);
            }
            v.visit_type(& $($mut)? node.ty);
        }
    };
}

/// Shared-reference ([`visit`]) and mutable ([`visit_mut`]) traversals over the owned AST.
///
/// Follows the same pattern as `syn::visit` / `syn::visit_mut`: every node type has a trait method whose
/// default implementation calls the free function of the same name, which recurses into that node's children.
/// Override a method to act on a node; call the free function from the override to keep descending,
/// or don't to skip the whole subtree.
///
/// Both modules are generated by one macro, so the traversal order and set of visited nodes are identical.
///
/// Notes:
/// * [`TypeOwned::OutOfLine`](crate::TypeOwned::OutOfLine) and [`ApiItemKindOwned::Trait`](crate::ApiItemKindOwned::Trait) are indices, they are not followed. Their targets
///   are visited exactly once when walking [`ApiBundleOwned::types`](crate::ApiBundleOwned::types) and [`ApiBundleOwned::traits`](crate::ApiBundleOwned::traits).
/// * `Skipped*` type and trait locations carry no AST, walking them does nothing
///   (override `visit_type_location` / `visit_api_level_location` to observe them).
/// * `visit_ident` is only called for declared names (traits, items, arguments, structs, enums, variants, fields).
/// * Field default values are not walked.
///
/// This module is the read-only traversal: the `'ast` lifetime lets a visitor keep references into the tree.
pub mod visit {
    define_visitor!(
        /// Read-only AST visitor, see the [module docs](crate::visit) for how traversal works.
        trait Visit<'ast>,
        ref = &'ast
    );
}

/// Mutable traversal, allows editing the tree in place. See [`visit`] for how traversal works.
pub mod visit_mut {
    define_visitor!(
        /// Mutable AST visitor, see the [module docs](crate::visit) for how traversal works.
        trait VisitMut,
        ref = &mut
    );
}

#[cfg(test)]
mod tests {
    use super::visit::{self, Visit};
    use super::visit_mut::VisitMut;
    use crate::*;
    use shrink_wrap::ElementSize;

    fn docs(s: &str) -> Vec<String> {
        vec![s.to_string()]
    }

    fn field(ident: Option<&str>, ty: TypeOwned) -> FieldOwned {
        FieldOwned {
            docs: docs("field"),
            ident: ident.map(String::from),
            default: None,
            since: None,
            ty,
        }
    }

    fn bundle() -> ApiBundleOwned {
        let item_struct = TypeOwned::Struct(ItemStructOwned {
            size: ElementSize::Unsized,
            crate_idx: UNib32(0),
            docs: docs("struct"),
            ident: "S".into(),
            fields: FieldsOwned::Named(vec![field(Some("a"), TypeOwned::Bool)]),
        });
        let item_enum = TypeOwned::Enum(ItemEnumOwned {
            size: ElementSize::Unsized,
            repr: Repr::UNib32,
            crate_idx: UNib32(0),
            docs: docs("enum"),
            ident: "E".into(),
            variants: vec![VariantOwned {
                docs: docs("variant"),
                ident: "V".into(),
                fields: FieldsOwned::Unnamed(vec![field(None, TypeOwned::String)]),
                discriminant: UNib32(0),
                since: None,
            }],
        });
        let item = |id: u32, ident: &str, kind| ApiItemOwned {
            id: UNib32(id),
            kind,
            multiplicity: Multiplicity::Flat,
            since: None,
            ident: ident.into(),
            docs: docs("item"),
        };
        ApiBundleOwned {
            magic: MAGIC,
            ww_self_version: VERSION,
            root: ApiLevelOwned {
                docs: docs("root"),
                crate_idx: UNib32(0),
                trait_name: "Root".into(),
                items: vec![
                    item(
                        0,
                        "method",
                        ApiItemKindOwned::Method {
                            args: vec![ArgumentOwned {
                                ident: "arg".into(),
                                ty: TypeOwned::Vec(Box::new(item_struct)),
                            }],
                            return_ty: Some(TypeOwned::Result {
                                ok_ty: Box::new(TypeOwned::OutOfLine {
                                    type_idx: UNib32(0),
                                }),
                                err_ty: Box::new(item_enum),
                            }),
                        },
                    ),
                    item(
                        1,
                        "sub",
                        ApiItemKindOwned::Trait {
                            trait_idx: UNib32(0),
                        },
                    ),
                ],
            },
            types: vec![
                TypeLocationOwned::InLine {
                    ty: TypeOwned::Tuple(vec![TypeOwned::Bool, TypeOwned::Flag]),
                    crate_idx: UNib32(0),
                },
                TypeLocationOwned::SkippedFullVersion {
                    crate_idx: UNib32(0),
                    type_name: "Skipped".into(),
                    signature: vec![],
                },
            ],
            traits: vec![
                ApiLevelLocationOwned::InLine {
                    level: ApiLevelOwned {
                        docs: docs("sub"),
                        crate_idx: UNib32(0),
                        trait_name: "Sub".into(),
                        items: vec![],
                    },
                    crate_idx: UNib32(0),
                },
                ApiLevelLocationOwned::SkippedFullVersion {
                    crate_idx: UNib32(0),
                    trait_name: "Skipped".into(),
                    signature: vec![],
                },
            ],
            ext_crates: vec![],
        }
    }

    #[derive(Default)]
    struct Collect<'ast> {
        idents: Vec<&'ast str>,
        docs: usize,
        types: usize,
    }

    impl<'ast> Visit<'ast> for Collect<'ast> {
        fn visit_type(&mut self, node: &'ast TypeOwned) {
            self.types += 1;
            visit::visit_type(self, node);
        }

        fn visit_ident(&mut self, node: &'ast String) {
            self.idents.push(node);
        }

        fn visit_docs(&mut self, node: &'ast Vec<String>) {
            self.docs += node.len();
        }
    }

    #[test]
    fn visits_every_node_once() {
        let bundle = bundle();
        let mut c = Collect::default();
        c.visit_api_bundle(&bundle);
        assert_eq!(
            c.idents,
            ["Root", "method", "arg", "S", "a", "E", "V", "sub", "Sub"]
        );
        // root, 2 items, struct, field, enum, variant, field, sub level
        assert_eq!(c.docs, 9);
        // arg: Vec, S, bool; return: Result, OutOfLine, E, String; types[0]: Tuple, Bool, Flag
        assert_eq!(c.types, 10);
    }

    #[test]
    fn override_can_skip_subtree() {
        struct SkipTypes(usize);
        impl Visit<'_> for SkipTypes {
            fn visit_type(&mut self, _: &TypeOwned) {}
            fn visit_ident(&mut self, _: &String) {
                self.0 += 1;
            }
        }
        let mut v = SkipTypes(0);
        v.visit_api_bundle(&bundle());
        // Root, method, arg, sub, Sub
        assert_eq!(v.0, 5);
    }

    #[test]
    fn mutate_in_place() {
        struct DropDocs;
        impl VisitMut for DropDocs {
            fn visit_docs(&mut self, node: &mut Vec<String>) {
                node.clear();
            }
        }
        let mut bundle = bundle();
        DropDocs.visit_api_bundle(&mut bundle);
        let mut c = Collect::default();
        c.visit_api_bundle(&bundle);
        assert_eq!(c.docs, 0);
        assert_eq!(c.idents.len(), 9);
    }
}
