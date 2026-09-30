//! Per-resource compatibility between the client's API and the connected device's API.
//!
//! If both sides report the same API hash, the APIs are identical and nothing is checked. Otherwise, both
//! introspection bundles are walked, and every method, property, stream and trait the client knows about is compared
//! with the resource at the same path on the device. Calling a resource that is missing or incompatible then fails
//! locally with a clear error, instead of sending bytes the device would misinterpret.
//!
//! Types are compared structurally, following [evolution rules](https://github.com/vhrdtech/wire_weaver/blob/master/docs/evolution/rules.md):
//! names don't matter, positions do. Direction matters as well: an `Unsized` struct written by one side can have more
//! trailing fields than the reader knows about (they are skipped), but a reader's extra fields must have `#[default]`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use wire_weaver::shrink_wrap::{ElementSize, UNib32};
use ww_self::signature;
use ww_self::visit::Visit;
use ww_self::{
    ApiBundleOwned, ApiItemKindOwned, ApiItemOwned, ApiLevelLocationOwned, FieldOwned, FieldsOwned,
    ItemEnumOwned, ItemStructOwned, PropertyAccess, TypeLocationOwned, TypeOwned,
};
use ww_version::{FullVersionOwned, VersionTriplet};

/// How the device's API relates to the client's, decided once after connecting.
#[derive(Clone)]
pub(crate) enum ApiMatch {
    /// Client has no API of its own (DynClient), nothing to check against.
    NoClientApi,
    /// Device reported the same API hash as the client was generated with.
    Identical,
    /// API hash is different, every resource was compared with the device's introspection data.
    Checked(Arc<ApiCompat>),
    /// API hash is different, but device's API is unknown (introspection disabled and not in the cache).
    /// Only `#[since]` annotations of the client's resources can be checked against the device's API version.
    DeviceApiUnknown(Arc<ApiCompat>),
}

/// Outcome of comparing one client resource with the device's resource at the same path.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Verdict {
    Compatible,
    /// Device has no resource at this path.
    Missing,
    /// Device has a resource at this path, but it can't be used with the client's definition.
    Incompatible(String),
}

/// Client resource tree with a verdict for each resource.
pub(crate) struct ApiCompat {
    root: HashMap<u32, Node>,
}

struct Node {
    ident: String,
    is_array: bool,
    since: Option<VersionTriplet>,
    verdict: Verdict,
    children: HashMap<u32, Node>,
}

/// Resource found at a request path, see [ApiCompat::resolve].
pub(crate) struct Resolved<'a> {
    /// Human-readable resource path, e.g. `gpio[].set_high`.
    pub(crate) name: String,
    /// First not compatible verdict along the path (a resource inside an incompatible trait is incompatible too),
    /// or the resource's own verdict.
    pub(crate) verdict: &'a Verdict,
    /// The newest `#[since]` along the path.
    pub(crate) since: Option<(u32, u32, u32)>,
}

impl ApiCompat {
    /// Compare every resource of the client's API with the device's resource at the same path.
    pub(crate) fn new(client: &ApiBundleOwned, device: &ApiBundleOwned) -> Self {
        let device_items: HashMap<Vec<u32>, &ApiItemOwned> = collect(device).into_iter().collect();
        Self::build(client, |path, item| match device_items.get(path) {
            Some(device_item) => match compare_items(client, item, device, device_item) {
                Ok(()) => Verdict::Compatible,
                Err(reason) => Verdict::Incompatible(reason),
            },
            None => Verdict::Missing,
        })
    }

    /// Client resource tree only, when the device's API is unknown, to look up `#[since]` of the client's resources.
    pub(crate) fn client_only(client: &ApiBundleOwned) -> Self {
        Self::build(client, |_, _| Verdict::Compatible)
    }

    fn build(
        client: &ApiBundleOwned,
        mut verdict: impl FnMut(&[u32], &ApiItemOwned) -> Verdict,
    ) -> Self {
        let mut root: HashMap<u32, Node> = HashMap::new();
        // parents are collected before their children
        for (path, item) in collect(client) {
            let node = Node {
                ident: item.ident.clone(),
                is_array: item.is_array(),
                since: item.since,
                verdict: verdict(&path, item),
                children: HashMap::new(),
            };
            let (last, parents) = path.split_last().expect("item path is never empty");
            let mut level = &mut root;
            for id in parents {
                level = &mut level
                    .get_mut(id)
                    .expect("parent is collected first")
                    .children;
            }
            level.insert(*last, node);
        }
        ApiCompat { root }
    }

    /// Find a resource by a request path (item ids interleaved with array indices, as sent to the device).
    /// Returns None for paths not in the client's API.
    pub(crate) fn resolve(&self, path: &[UNib32]) -> Option<Resolved<'_>> {
        let mut level = &self.root;
        let mut name = String::new();
        let mut since = None;
        let mut path = path.iter();
        let mut resolved = None;
        while let Some(id) = path.next() {
            let node = level.get(&id.0)?;
            if !name.is_empty() {
                name.push('.');
            }
            name.push_str(&node.ident);
            if node.is_array {
                name.push_str("[]");
                // array index, absent when reading valid indices of the array itself
                path.next();
            }
            if let Some(s) = node.since {
                since = since.max(Some((s.major.0, s.minor.0, s.patch.0)));
            }
            resolved = match resolved {
                Some(verdict) if verdict != &Verdict::Compatible => Some(verdict),
                _ => Some(&node.verdict),
            };
            level = &node.children;
        }
        Some(Resolved {
            name,
            verdict: resolved?,
            since,
        })
    }

    /// Resources that are missing or incompatible, not descending into ones already reported.
    pub(crate) fn mismatches(&self) -> Vec<(String, &Verdict)> {
        fn walk<'a>(
            level: &'a HashMap<u32, Node>,
            prefix: &str,
            out: &mut Vec<(String, &'a Verdict)>,
        ) {
            let mut ids: Vec<_> = level.keys().collect();
            ids.sort();
            for id in ids {
                let node = &level[id];
                let array = if node.is_array { "[]" } else { "" };
                let name = format!("{prefix}{}{array}", node.ident);
                if node.verdict == Verdict::Compatible {
                    walk(&node.children, &format!("{name}."), out);
                } else {
                    out.push((name, &node.verdict));
                }
            }
        }
        let mut out = vec![];
        walk(&self.root, "", &mut out);
        out
    }
}

/// Walks an API bundle from the root, descending into traits, and collects every item with its id path.
struct ItemCollector<'ast> {
    bundle: &'ast ApiBundleOwned,
    path: Vec<u32>,
    trait_stack: Vec<u32>,
    items: Vec<(Vec<u32>, &'ast ApiItemOwned)>,
}

impl<'ast> Visit<'ast> for ItemCollector<'ast> {
    fn visit_api_item(&mut self, node: &'ast ApiItemOwned) {
        self.path.push(node.id.0);
        self.items.push((self.path.clone(), node));
        // traits are referred to by index, follow them to get the resource tree as the device sees it
        if let ApiItemKindOwned::Trait { trait_idx } = &node.kind
            && !self.trait_stack.contains(&trait_idx.0)
            && let Some(location) = self.bundle.traits.get(trait_idx.0 as usize)
        {
            self.trait_stack.push(trait_idx.0);
            self.visit_api_level_location(location);
            self.trait_stack.pop();
        }
        self.path.pop();
    }
}

fn collect(bundle: &ApiBundleOwned) -> Vec<(Vec<u32>, &ApiItemOwned)> {
    let mut collector = ItemCollector {
        bundle,
        path: vec![],
        trait_stack: vec![],
        items: vec![],
    };
    collector.visit_api_level(&bundle.root);
    collector.items
}

fn compare_items(
    client: &ApiBundleOwned,
    client_item: &ApiItemOwned,
    device: &ApiBundleOwned,
    device_item: &ApiItemOwned,
) -> Result<(), String> {
    let array = |item: &ApiItemOwned| {
        if item.is_array() {
            "an array"
        } else {
            "not an array"
        }
    };
    if client_item.is_array() != device_item.is_array() {
        return Err(format!(
            "{} on the client, {} on the device",
            array(client_item),
            array(device_item)
        ));
    }
    // types flowing from client to device, and from device to client
    let mut to_device = TypeCheck::new(client, device, "client", "device");
    let mut to_client = TypeCheck::new(device, client, "device", "client");
    use ApiItemKindOwned::*;
    match (&client_item.kind, &device_item.kind) {
        (
            Method { args, return_ty },
            Method {
                args: device_args,
                return_ty: device_return_ty,
            },
        ) => {
            if args.len() != device_args.len() {
                return Err(format!(
                    "takes {} argument(s) on the client, {} on the device",
                    args.len(),
                    device_args.len()
                ));
            }
            for (arg, device_arg) in args.iter().zip(device_args) {
                to_device
                    .check(&arg.ty, &device_arg.ty)
                    .map_err(|e| format!("argument `{}`: {e}", arg.ident))?;
            }
            match (return_ty, device_return_ty) {
                (None, None) => {}
                (Some(ty), Some(device_ty)) => to_client
                    .check(device_ty, ty)
                    .map_err(|e| format!("return type: {e}"))?,
                (Some(_), None) => return Err("device returns nothing".into()),
                (None, Some(_)) => return Err("device returns a value, client expects none".into()),
            }
        }
        (
            Property {
                ty,
                access,
                write_err_ty,
            },
            Property {
                ty: device_ty,
                access: device_access,
                write_err_ty: device_write_err_ty,
            },
        ) => {
            let (readable, writable, observable) = capabilities(access);
            let (device_readable, device_writable, device_observable) = capabilities(device_access);
            if (readable && !device_readable)
                || (writable && !device_writable)
                || (observable && !device_observable)
            {
                return Err(format!(
                    "{access:?} on the client, {device_access:?} on the device"
                ));
            }
            if readable {
                to_client.check(device_ty, ty)?;
            }
            if writable {
                to_device.check(ty, device_ty)?;
                match (write_err_ty, device_write_err_ty) {
                    (None, None) => {}
                    (Some(ty), Some(device_ty)) => to_client
                        .check(device_ty, ty)
                        .map_err(|e| format!("write error type: {e}"))?,
                    _ => return Err("write error type is only defined on one side".into()),
                }
            }
        }
        (
            Stream { ty, is_up },
            Stream {
                ty: device_ty,
                is_up: device_is_up,
            },
        ) => {
            if is_up != device_is_up {
                return Err("stream on one side, sink on the other".into());
            }
            if *is_up {
                to_client.check(device_ty, ty)?;
            } else {
                to_device.check(ty, device_ty)?;
            }
        }
        (
            Trait { trait_idx },
            Trait {
                trait_idx: device_trait_idx,
            },
        ) => {
            let (Some(identity), Some(device_identity)) = (
                trait_identity(client, trait_idx.0),
                trait_identity(device, device_trait_idx.0),
            ) else {
                // not enough information (e.g., compact version only), items are still compared one by one
                return Ok(());
            };
            check_same_origin("trait", identity, device_identity)?;
            // when both are in-line, their items are compared one by one instead
            let skipped = |bundle: &ApiBundleOwned, idx: u32| {
                !matches!(
                    bundle.traits.get(idx as usize),
                    Some(ApiLevelLocationOwned::InLine { .. })
                )
            };
            if skipped(client, trait_idx.0) || skipped(device, device_trait_idx.0) {
                check_same_definition(
                    "trait",
                    identity,
                    device_identity,
                    trait_signature(client, trait_idx.0),
                    trait_signature(device, device_trait_idx.0),
                )?;
            }
        }
        (kind, device_kind) => {
            return Err(format!(
                "{} on the client, {} on the device",
                kind_name(kind),
                kind_name(device_kind)
            ));
        }
    }
    Ok(())
}

/// (readable, writable, observable)
fn capabilities(access: &PropertyAccess) -> (bool, bool, bool) {
    match access {
        PropertyAccess::Const => (true, false, false),
        PropertyAccess::ReadOnly { observe } => (true, false, *observe),
        PropertyAccess::ReadWrite { observe } => (true, true, *observe),
        PropertyAccess::WriteOnly => (false, true, false),
    }
}

fn kind_name(kind: &ApiItemKindOwned) -> &'static str {
    match kind {
        ApiItemKindOwned::Method { .. } => "a method",
        ApiItemKindOwned::Property { .. } => "a property",
        ApiItemKindOwned::Stream { is_up: true, .. } => "a stream",
        ApiItemKindOwned::Stream { is_up: false, .. } => "a sink",
        ApiItemKindOwned::Trait { .. } => "a trait",
    }
}

/// Crate and name of a trait or type, and it's version.
type Identity<'a> = (&'a FullVersionOwned, &'a str);

fn trait_identity(bundle: &ApiBundleOwned, trait_idx: u32) -> Option<Identity<'_>> {
    let (crate_idx, name) = match bundle.traits.get(trait_idx as usize)? {
        ApiLevelLocationOwned::InLine { level, .. } => (level.crate_idx, level.trait_name.as_str()),
        ApiLevelLocationOwned::SkippedFullVersion {
            crate_idx,
            trait_name,
            ..
        } => (*crate_idx, trait_name.as_str()),
        ApiLevelLocationOwned::SkippedCompactVersion { .. } => return None,
    };
    Some((bundle.crate_version(crate_idx.0).ok()?, name))
}

fn check_same_origin(what: &str, a: Identity, b: Identity) -> Result<(), String> {
    if a.0.crate_id != b.0.crate_id || a.1 != b.1 {
        return Err(format!(
            "{what} {}::{} vs {}::{}",
            a.0.crate_id, a.1, b.0.crate_id, b.1
        ));
    }
    if !a.0.is_protocol_compatible(b.0) {
        return Err(format!(
            "{what} {} from incompatible crate versions: {:?} vs {:?}",
            a.1, a.0, b.0
        ));
    }
    Ok(())
}

/// Catches a definition that was changed without bumping its crate version: same version, but a different
/// signature. Only possible to check when both signatures are known.
fn check_same_definition(
    what: &str,
    a: Identity,
    b: Identity,
    a_signature: Option<Vec<u8>>,
    b_signature: Option<Vec<u8>>,
) -> Result<(), String> {
    let (Some(a_signature), Some(b_signature)) = (a_signature, b_signature) else {
        return Ok(());
    };
    if a.0 == b.0 && a_signature != b_signature {
        let v = &a.0.version;
        return Err(format!(
            "{what} {}::{} {}.{}.{} has a different definition on each side, likely changed without bumping the crate version",
            a.0.crate_id, a.1, v.major.0, v.minor.0, v.patch.0
        ));
    }
    Ok(())
}

/// Signature of a trait: stored for a skipped one, calculated for an in-line one (definitions it refers to that are
/// skipped are looked up in embedded snapshots).
fn trait_signature(bundle: &ApiBundleOwned, trait_idx: u32) -> Option<Vec<u8>> {
    match bundle.traits.get(trait_idx as usize)? {
        ApiLevelLocationOwned::InLine { .. } => {
            signature::trait_signature(bundle, trait_idx, &|v| crate::snapshots::get(v)).ok()
        }
        ApiLevelLocationOwned::SkippedFullVersion { signature, .. }
        | ApiLevelLocationOwned::SkippedCompactVersion { signature, .. } => {
            (!signature.is_empty()).then(|| signature.clone())
        }
    }
}

/// Signature of a type: stored for a skipped one, calculated for an in-line one (definitions it refers to that are
/// skipped are looked up in embedded snapshots).
fn type_signature(bundle: &ApiBundleOwned, type_idx: u32) -> Option<Vec<u8>> {
    match bundle.types.get(type_idx as usize)? {
        TypeLocationOwned::InLine { .. } => {
            signature::type_signature(bundle, type_idx, &|v| crate::snapshots::get(v)).ok()
        }
        TypeLocationOwned::SkippedFullVersion { signature, .. } => {
            (!signature.is_empty()).then(|| signature.clone())
        }
    }
}

/// Checks that a value written as one type can be read as the other.
struct TypeCheck<'a> {
    writer: &'a ApiBundleOwned,
    reader: &'a ApiBundleOwned,
    writer_name: &'static str,
    reader_name: &'static str,
    /// Out-of-line types already compared or being compared, to stop on self-referential types.
    seen: HashSet<(u32, u32)>,
}

/// Type with out-of-line references resolved.
enum ResolvedTy<'a> {
    Ty(&'a TypeOwned),
    /// Type definition is not in the bundle, only where it comes from.
    Skipped(Identity<'a>),
}

impl<'a> TypeCheck<'a> {
    fn new(
        writer: &'a ApiBundleOwned,
        reader: &'a ApiBundleOwned,
        writer_name: &'static str,
        reader_name: &'static str,
    ) -> Self {
        TypeCheck {
            writer,
            reader,
            writer_name,
            reader_name,
            seen: HashSet::new(),
        }
    }

    fn check(&mut self, w: &'a TypeOwned, r: &'a TypeOwned) -> Result<(), String> {
        if let (TypeOwned::OutOfLine { type_idx: wi }, TypeOwned::OutOfLine { type_idx: ri }) =
            (w, r)
            && !self.seen.insert((wi.0, ri.0))
        {
            return Ok(());
        }
        let (w_ty, r_ty) = (w, r);
        let (w, r) = match (resolve(self.writer, w)?, resolve(self.reader, r)?) {
            (ResolvedTy::Ty(w), ResolvedTy::Ty(r)) => (w, r),
            (w, r) => {
                let identity = |bundle, resolved| match resolved {
                    ResolvedTy::Skipped(identity) => Some(identity),
                    ResolvedTy::Ty(ty) => user_type_identity(bundle, ty),
                };
                let (Some(w), Some(r)) = (identity(self.writer, w), identity(self.reader, r))
                else {
                    return Err("type definition is not available on one side".into());
                };
                check_same_origin("type", w, r)?;
                let signature = |bundle, ty: &TypeOwned| match ty {
                    TypeOwned::OutOfLine { type_idx } => type_signature(bundle, type_idx.0),
                    _ => None,
                };
                return check_same_definition(
                    "type",
                    w,
                    r,
                    signature(self.writer, w_ty),
                    signature(self.reader, r_ty),
                );
            }
        };
        use TypeOwned::*;
        match (w, r) {
            (Vec(w), Vec(r)) | (Box(w), Box(r)) => self.check(w, r),
            (Option { some_ty: w }, Option { some_ty: r }) => self.check(w, r),
            (Array { len, ty: w }, Array { len: r_len, ty: r }) => {
                if len != r_len {
                    return Err(format!("[_; {}] vs [_; {}]", len.0, r_len.0));
                }
                self.check(w, r)
            }
            (
                Result { ok_ty, err_ty },
                Result {
                    ok_ty: r_ok_ty,
                    err_ty: r_err_ty,
                },
            ) => {
                self.check(ok_ty, r_ok_ty)?;
                self.check(err_ty, r_err_ty)
            }
            (Tuple(w), Tuple(r)) => {
                if w.len() != r.len() {
                    return Err(format!("tuple of {} vs tuple of {}", w.len(), r.len()));
                }
                for (i, (w, r)) in w.iter().zip(r).enumerate() {
                    self.check(w, r).map_err(|e| format!(".{i}: {e}"))?;
                }
                Ok(())
            }
            (Struct(w), Struct(r)) => self
                .check_struct(w, r)
                .map_err(|e| format!("struct {}: {e}", r.ident)),
            (Enum(w), Enum(r)) => self
                .check_enum(w, r)
                .map_err(|e| format!("enum {}: {e}", r.ident)),
            (Bool, Bool) | (Flag, Flag) | (String, String) => Ok(()),
            (NumericAny(_), NumericAny(_))
            | (Range(_), Range(_))
            | (RangeInclusive(_), RangeInclusive(_))
                if w == r =>
            {
                Ok(())
            }
            _ => Err(format!(
                "{} on the {}, {} on the {}",
                describe(w),
                self.writer_name,
                describe(r),
                self.reader_name
            )),
        }
    }

    fn check_struct(
        &mut self,
        w: &'a ItemStructOwned,
        r: &'a ItemStructOwned,
    ) -> Result<(), String> {
        self.check_fields(&w.size, &w.fields, &r.size, &r.fields)
    }

    fn check_enum(&mut self, w: &'a ItemEnumOwned, r: &'a ItemEnumOwned) -> Result<(), String> {
        if w.repr != r.repr {
            return Err(format!("repr {:?} vs {:?}", w.repr, r.repr));
        }
        for variant in &w.variants {
            // unknown variants can't be read, extra variants on the reader side are never sent
            let Some(r_variant) = r
                .variants
                .iter()
                .find(|v| v.discriminant == variant.discriminant)
            else {
                return Err(format!(
                    "variant {} is unknown to the {}",
                    variant.ident, self.reader_name
                ));
            };
            self.check_fields(&w.size, &variant.fields, &r.size, &r_variant.fields)
                .map_err(|e| format!("variant {}: {e}", r_variant.ident))?;
        }
        Ok(())
    }

    fn check_fields(
        &mut self,
        w_size: &ElementSize,
        w: &'a FieldsOwned,
        r_size: &ElementSize,
        r: &'a FieldsOwned,
    ) -> Result<(), String> {
        if std::mem::discriminant(w_size) != std::mem::discriminant(r_size) {
            return Err(format!("{w_size:?} vs {r_size:?}"));
        }
        let (w, r) = (fields(w), fields(r));
        for (i, (w_field, r_field)) in w.iter().zip(r).enumerate() {
            self.check(&w_field.ty, &r_field.ty)
                .map_err(|e| format!("field {}: {e}", field_name(i, r_field)))?;
        }
        if matches!(r_size, ElementSize::Unsized) {
            // writer's extra trailing fields are skipped, reader's extra fields must have a default
            for (i, r_field) in r.iter().enumerate().skip(w.len()) {
                if r_field.default.is_none() {
                    return Err(format!(
                        "field {} is not sent by the {} and has no #[default]",
                        field_name(i, r_field),
                        self.writer_name
                    ));
                }
            }
        } else if w.len() != r.len() {
            return Err(format!(
                "{} fields on the {}, {} on the {}, only Unsized types can gain new fields",
                w.len(),
                self.writer_name,
                r.len(),
                self.reader_name
            ));
        }
        Ok(())
    }
}

fn resolve<'a>(bundle: &'a ApiBundleOwned, ty: &'a TypeOwned) -> Result<ResolvedTy<'a>, String> {
    let TypeOwned::OutOfLine { type_idx } = ty else {
        return Ok(ResolvedTy::Ty(ty));
    };
    match bundle.types.get(type_idx.0 as usize) {
        Some(TypeLocationOwned::InLine { ty, .. }) => Ok(ResolvedTy::Ty(ty)),
        Some(TypeLocationOwned::SkippedFullVersion {
            crate_idx,
            type_name,
            ..
        }) => {
            let version = bundle
                .crate_version(crate_idx.0)
                .map_err(|e| e.to_string())?;
            Ok(ResolvedTy::Skipped((version, type_name.as_str())))
        }
        None => Err(format!("bad API bundle: no type with index {}", type_idx.0)),
    }
}

fn user_type_identity<'a>(bundle: &'a ApiBundleOwned, ty: &'a TypeOwned) -> Option<Identity<'a>> {
    let (crate_idx, ident) = match ty {
        TypeOwned::Struct(s) => (s.crate_idx, &s.ident),
        TypeOwned::Enum(e) => (e.crate_idx, &e.ident),
        _ => return None,
    };
    Some((bundle.crate_version(crate_idx.0).ok()?, ident.as_str()))
}

fn fields(fields: &FieldsOwned) -> &[FieldOwned] {
    match fields {
        FieldsOwned::Named(fields) | FieldsOwned::Unnamed(fields) => fields,
        FieldsOwned::Unit => &[],
    }
}

fn field_name(idx: usize, field: &FieldOwned) -> String {
    match &field.ident {
        Some(ident) => format!("`{ident}`"),
        None => format!("{idx}"),
    }
}

fn describe(ty: &TypeOwned) -> String {
    match ty {
        TypeOwned::Bool => "bool".into(),
        TypeOwned::NumericAny(n) => {
            // Base(U8) -> u8
            let n = format!("{n:?}");
            match n.strip_prefix("Base(").and_then(|n| n.strip_suffix(')')) {
                Some(base) => base.to_lowercase(),
                None => n,
            }
        }
        TypeOwned::OutOfLine { type_idx } => format!("type #{}", type_idx.0),
        TypeOwned::Flag => "flag".into(),
        TypeOwned::String => "String".into(),
        TypeOwned::Vec(_) => "Vec".into(),
        TypeOwned::Array { len, .. } => format!("array of {}", len.0),
        TypeOwned::Tuple(t) => format!("tuple of {}", t.len()),
        TypeOwned::Struct(s) => format!("struct {}", s.ident),
        TypeOwned::Enum(e) => format!("enum {}", e.ident),
        TypeOwned::Option { .. } => "Option".into(),
        TypeOwned::Result { .. } => "Result".into(),
        TypeOwned::Box(_) => "Box".into(),
        TypeOwned::Range(_) => "Range".into(),
        TypeOwned::RangeInclusive(_) => "RangeInclusive".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Build an API bundle from source, as codegen would, by writing a throwaway API crate.
    fn bundle(name: &str, version: &str, lib_rs: &str) -> ApiBundleOwned {
        let dir: PathBuf = std::env::temp_dir()
            .join("ww_client_compat_tests")
            .join(format!("{name}-{version}-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            format!(
                "[package]\nname = \"test_api\"\nversion = \"{version}\"\nedition = \"2024\"\n\n[dependencies]\n"
            ),
        )
        .unwrap();
        let lib_rs = format!("use wire_weaver::prelude::*;\n\n{lib_rs}");
        std::fs::write(dir.join("src/lib.rs"), lib_rs).unwrap();
        let bundle = wire_weaver_core::load(&dir, None, true).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        bundle
    }

    fn compat(client: &str, device: &str) -> ApiCompat {
        let client = bundle("client", "0.1.1", client);
        let device = bundle("device", "0.1.0", device);
        ApiCompat::new(&client, &device)
    }

    fn path(ids: &[u32]) -> Vec<UNib32> {
        ids.iter().map(|id| UNib32(*id)).collect()
    }

    fn verdict(compat: &ApiCompat, ids: &[u32]) -> Verdict {
        compat.resolve(&path(ids)).unwrap().verdict.clone()
    }

    fn assert_incompatible(compat: &ApiCompat, ids: &[u32], reason_contains: &str) {
        match verdict(compat, ids) {
            Verdict::Incompatible(reason) => assert!(
                reason.contains(reason_contains),
                "'{reason}' does not contain '{reason_contains}'"
            ),
            v => panic!("expected incompatible, got {v:?}"),
        }
    }

    const BLINKY: &str = r#"
        #[ww_api_root]
        pub trait Blinky {
            fn led_on();
            fn led_off();
        }
    "#;

    #[test]
    fn identical_apis() {
        let compat = compat(BLINKY, BLINKY);
        assert!(compat.mismatches().is_empty());
        assert_eq!(verdict(&compat, &[0]), Verdict::Compatible);
        assert!(
            compat.resolve(&path(&[5])).is_none(),
            "unknown path is not checked"
        );
    }

    #[test]
    fn newer_client_older_device() {
        let client = r#"
            #[ww_api_root]
            pub trait Blinky {
                fn led_on();
                fn led_off();
                #[since = "0.1.1"]
                fn led_toggle();
            }
        "#;
        let compat = compat(client, BLINKY);
        let toggle = compat.resolve(&path(&[2])).unwrap();
        assert_eq!(toggle.name, "led_toggle");
        assert_eq!(toggle.verdict, &Verdict::Missing);
        assert_eq!(toggle.since, Some((0, 1, 1)));
        assert_eq!(compat.mismatches().len(), 1);

        // resources the client doesn't know about are irrelevant
        let compat = ApiCompat::new(
            &bundle("client", "0.1.0", BLINKY),
            &bundle("device", "0.1.1", client),
        );
        assert!(compat.mismatches().is_empty());
    }

    #[test]
    fn method_signatures() {
        let client = r#"
            #[ww_api_root]
            pub trait Api {
                fn set(x: u8);
                fn get() -> u8;
                fn two(a: u8, b: u8);
                fn was_method();
            }
        "#;
        let device = r#"
            #[ww_api_root]
            pub trait Api {
                fn set(x: u16);
                fn get() -> u8;
                fn two(a: u8);
                property!(rw was_method: u8);
            }
        "#;
        let compat = compat(client, device);
        assert_incompatible(&compat, &[0], "argument `x`");
        assert_eq!(verdict(&compat, &[1]), Verdict::Compatible);
        assert_incompatible(
            &compat,
            &[2],
            "takes 2 argument(s) on the client, 1 on the device",
        );
        assert_incompatible(
            &compat,
            &[3],
            "a method on the client, a property on the device",
        );
    }

    #[test]
    fn evolved_struct() {
        let v1 = r#"
            #[ww_api_root]
            pub trait Api {
                fn set(c: Coord);
                fn get() -> Coord;
            }

            #[derive_shrink_wrap]
            struct Coord {
                x: u8,
                y: u8,
            }
        "#;
        let v2_default = r#"
            #[ww_api_root]
            pub trait Api {
                fn set(c: Coord);
                fn get() -> Coord;
            }

            #[derive_shrink_wrap]
            struct Coord {
                x: u8,
                y: u8,
                #[default = None]
                z: Option<u8>,
            }
        "#;
        // device reads a newer struct with a default, client reads a newer struct and skips the extra field
        let compat = compat(v1, v2_default);
        assert!(compat.mismatches().is_empty(), "{:?}", compat.mismatches());
        // same the other way around
        let compat = self::compat(v2_default, v1);
        assert!(compat.mismatches().is_empty(), "{:?}", compat.mismatches());

        let v2_no_default = v2_default.replace("#[default = None]", "");
        // device can't read what an older client sends
        let compat = self::compat(v1, &v2_no_default);
        assert_incompatible(
            &compat,
            &[0],
            "field `z` is not sent by the client and has no #[default]",
        );
        assert_eq!(verdict(&compat, &[1]), Verdict::Compatible);
        // client can't read what an older device returns
        let compat = self::compat(&v2_no_default, v1);
        assert_eq!(verdict(&compat, &[0]), Verdict::Compatible);
        assert_incompatible(
            &compat,
            &[1],
            "return type: struct Coord: field `z` is not sent by the device",
        );

        let v1_final = v1.replace(
            "#[derive_shrink_wrap]",
            "#[derive_shrink_wrap(final_structure)]",
        );
        let v2_final = v2_default.replace(
            "#[derive_shrink_wrap]",
            "#[derive_shrink_wrap(final_structure)]",
        );
        let compat = self::compat(&v1_final, &v2_final);
        assert_incompatible(&compat, &[1], "only Unsized types can gain new fields");
    }

    #[test]
    fn enum_variants() {
        let v1 = r#"
            #[ww_api_root]
            pub trait Api {
                fn get() -> Mode;
                fn set(m: Mode);
            }

            #[derive_shrink_wrap(ww_repr = u4)]
            enum Mode {
                A,
                B(u8),
            }
        "#;
        let v2 = r#"
            #[ww_api_root]
            pub trait Api {
                fn get() -> Mode;
                fn set(m: Mode);
            }

            #[derive_shrink_wrap(ww_repr = u4)]
            enum Mode {
                A,
                B(u8),
                C,
            }
        "#;
        // newer device can return a variant the client doesn't know, but can read all the client sends
        let compat = compat(v1, v2);
        assert_incompatible(&compat, &[0], "variant C is unknown to the client");
        assert_eq!(verdict(&compat, &[1]), Verdict::Compatible);
    }

    #[test]
    fn properties_and_streams() {
        let client = r#"
            #[ww_api_root]
            pub trait Api {
                property!(rw a: u8);
                property!(ro b: u8);
                stream!(s: u8);
                sink!(k: u8);
            }
        "#;
        let device = r#"
            #[ww_api_root]
            pub trait Api {
                property!(ro a: u8);
                property!(rw b: u8);
                sink!(s: u8);
                sink!(k: u32);
            }
        "#;
        let compat = compat(client, device);
        assert_incompatible(&compat, &[0], "ReadWrite");
        assert_eq!(verdict(&compat, &[1]), Verdict::Compatible);
        assert_incompatible(&compat, &[2], "stream on one side, sink on the other");
        assert_incompatible(&compat, &[3], "u8 on the client");
    }

    #[test]
    fn traits_and_arrays() {
        let client = r#"
            #[ww_api_root]
            pub trait Api {
                ww_impl!(periph[]: Peripheral);
                ww_impl!(sub: Subgroup);
            }

            #[ww_trait]
            trait Peripheral {
                ww_impl!(channel[]: Channel);
            }

            #[ww_trait]
            trait Channel {
                property!(rw gain: f32);
                fn run();
            }

            #[ww_trait]
            trait Subgroup {
                fn m1();
            }
        "#;
        let device = client
            .replace("fn run();", "fn run(fast: bool);")
            .replace("trait Subgroup", "trait Other")
            .replace("sub: Subgroup", "sub: Other");
        let compat = compat(client, &device);

        // periph[3].channel[1].gain, periph[3].channel[1].run
        assert_eq!(verdict(&compat, &[0, 3, 0, 1, 0]), Verdict::Compatible);
        let run = compat.resolve(&path(&[0, 3, 0, 1, 1])).unwrap();
        assert_eq!(run.name, "periph[].channel[].run");
        assert!(matches!(run.verdict, Verdict::Incompatible(_)));
        // reading valid indices of the array itself (no index at the end of the path)
        assert_eq!(verdict(&compat, &[0, 3, 0]), Verdict::Compatible);

        // everything inside a different trait is incompatible as well
        assert_incompatible(&compat, &[1], "trait test_api::Subgroup vs test_api::Other");
        assert_incompatible(&compat, &[1, 0], "trait test_api::Subgroup");

        let mismatches: Vec<_> = compat
            .mismatches()
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(mismatches, ["periph[].channel[].run", "sub"]);
    }

    /// Device bundle with the `Subgroup` trait definition left out, as it would be when it's in a snapshot.
    fn skip_subgroup(mut device: ApiBundleOwned) -> ApiBundleOwned {
        let idx = device
            .traits
            .iter()
            .position(|l| matches!(l, ApiLevelLocationOwned::InLine { level, .. } if level.trait_name == "Subgroup"))
            .unwrap();
        let signature =
            signature::trait_signature(&device, idx as u32, &signature::no_resolve).unwrap();
        let ApiLevelLocationOwned::InLine { crate_idx, .. } = device.traits[idx] else {
            unreachable!()
        };
        device.traits[idx] = ApiLevelLocationOwned::SkippedFullVersion {
            crate_idx,
            trait_name: "Subgroup".into(),
            signature,
        };
        device
    }

    #[test]
    fn skipped_trait_changed_without_version_bump() {
        let source = r#"
            #[ww_api_root]
            pub trait Api {
                fn m0();
                ww_impl!(sub: Subgroup);
            }

            #[ww_trait]
            trait Subgroup {
                fn m1();
            }
        "#;
        let client = bundle("client", "0.1.0", source);

        let device = skip_subgroup(bundle("device", "0.1.0", source));
        let compat = ApiCompat::new(&client, &device);
        assert_eq!(verdict(&compat, &[1]), Verdict::Compatible);

        // same version, but Subgroup gained a method (a doc change would be caught the same way)
        let changed = source.replace("fn m1();", "fn m1();\nfn m2();");
        let device = skip_subgroup(bundle("device", "0.1.0", &changed));
        let compat = ApiCompat::new(&client, &device);
        assert_incompatible(&compat, &[1], "without bumping the crate version");
    }

    #[test]
    fn client_only_tracks_since() {
        let client = r#"
            #[ww_api_root]
            pub trait Api {
                fn a();
                #[since = "0.1.2"]
                ww_impl!(sub: Subgroup);
            }

            #[ww_trait]
            trait Subgroup {
                #[since = "0.1.1"]
                fn m1();
            }
        "#;
        let compat = ApiCompat::client_only(&bundle("client", "0.1.2", client));
        let m1 = compat.resolve(&path(&[1, 0])).unwrap();
        assert_eq!(m1.verdict, &Verdict::Compatible);
        assert_eq!(m1.since, Some((0, 1, 2)), "newest since along the path");
    }
    #[test]
    fn commander_fails_locally() {
        use crate::config::IntrospectBundle;
        use crate::{Commander, Error};
        use ww_client_server::PathKind;
        use ww_version::{ApiHashOwned, ApiHashPairOwned, VersionOwned};

        let client = r#"
            #[ww_api_root]
            pub trait Api {
                fn a(x: u8);
                #[since = "0.1.1"]
                fn b();
                fn c();
            }
        "#;
        let device = r#"
            #[ww_api_root]
            pub trait Api {
                fn a(x: u16);
            }
        "#;
        let introspect = |bundle, hash| IntrospectBundle {
            sent_size: 0,
            api_bundle: Arc::new(bundle),
            api_hash: ApiHashPairOwned {
                no_docs: ApiHashOwned { hash: vec![hash] },
                with_docs: ApiHashOwned { hash: vec![] },
            },
        };
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let mut cmd = Commander::new(tx);
        cmd.connected_device.user_api_version =
            FullVersionOwned::new("test_api".into(), VersionOwned::new(0, 1, 0));
        cmd.connected_device.user_api_hash = introspect(bundle("d", "0.1.0", device), 2).api_hash;
        cmd.set_client_introspect(introspect(bundle("c", "0.1.1", client), 1));
        cmd.set_device_introspect(introspect(bundle("d", "0.1.0", device), 2));
        cmd.resolve_api_match();

        fn call(cmd: &Commander, id: u32) -> Result<(), Error> {
            cmd.prepare_call::<()>(PathKind::absolute(&[UNib32(id)]), Ok(vec![]))
                .postpone_err
        }
        assert!(
            matches!(call(&cmd, 0), Err(Error::IncompatibleResource { resource, .. }) if resource == "a")
        );
        assert!(matches!(call(&cmd, 1), Err(Error::OlderProtocol(..))));
        assert!(
            matches!(call(&cmd, 2), Err(Error::NotImplementedByDevice { resource, .. }) if resource == "c")
        );

        // same hash: nothing is checked
        cmd.connected_device.user_api_hash.no_docs.hash = vec![1];
        cmd.resolve_api_match();
        assert!(call(&cmd, 2).is_ok());

        // device API unknown: only since is checked
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let mut cmd2 = Commander::new(tx);
        cmd2.connected_device = cmd.connected_device.clone();
        cmd2.connected_device.user_api_hash.no_docs.hash = vec![2];
        cmd2.set_client_introspect(introspect(bundle("c", "0.1.1", client), 1));
        cmd2.resolve_api_match();
        assert!(call(&cmd2, 0).is_ok());
        assert!(matches!(call(&cmd2, 1), Err(Error::OlderProtocol(..))));
    }
}
