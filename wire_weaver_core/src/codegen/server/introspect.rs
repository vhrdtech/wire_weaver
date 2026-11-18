use crate::codegen::api_server::IntrospectCompression;
use crate::codegen::omit::Omissions;
use crate::codegen::util::fixed_err_seq;
use convert_case::{Case, Casing};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use shrink_wrap::SerializeShrinkWrapOwned;
use ww_self::signature;
use ww_self::visit::Visit;
use ww_self::{ApiBundleOwned, ApiLevelLocationOwned, TypeLocationOwned};

/// Introspection request handler and API hash consts. With omitted items (API-16) the bundle a device sends
/// lists them, so with `omit_if` conditions the bytes and hashes are generated for each combination, under `#[cfg]`.
/// With `compression` the device keeps and sends `WW_SELF_BYTES` compressed (INT-16), the hashes are those of the
/// uncompressed bytes.
pub(crate) fn introspect(
    api_bundle: &crate::codegen::CodegenBundle,
    omissions: &Omissions,
    enabled: bool,
    include_docs: bool,
    compression: IntrospectCompression,
    use_async: bool,
) -> (TokenStream, TokenStream) {
    let mut bytes_consts = TokenStream::new();
    let mut hash_consts = TokenStream::new();
    for (cfg, active) in omissions.cfg_combinations() {
        let device_bundle = omissions.mark(api_bundle, &active);
        let IntrospectTs {
            introspect_bytes,
            no_docs_hash,
            with_docs_hash,
        } = introspect_prepare(
            &device_bundle,
            api_bundle.crate_dirs(),
            include_docs,
            compression,
        );
        let maybe_cfg = cfg.map(|cfg| quote! { #[cfg(#cfg)] });
        bytes_consts.extend(quote! {
            #maybe_cfg
            pub const WW_SELF_BYTES: #introspect_bytes;
        });
        hash_consts.extend(quote! {
            #maybe_cfg
            pub const API_HASH_NO_DOCS: #no_docs_hash;
            #maybe_cfg
            pub const API_HASH_WITH_DOCS: #with_docs_hash;
        });
    }
    let root = &api_bundle.root;
    let crate_name = api_bundle.trait_module_path(root);
    let full_gid = Ident::new(
        &format!("{}_FULL_GID", root.trait_name).to_case(Case::Constant),
        Span::call_site(),
    );
    // at module level, so that tests and tools see the bytes too, also of a device that doesn't send them (a host
    // seeds its cache with them); flash holds them only where the arm uses them
    let api_hash = quote! {
        #bytes_consts
        #hash_consts
        /// API identity string (`ww:<crate>@<version> h=<hash>`), see [wire_weaver::api_id].
        pub const API_ID: &str = wire_weaver::api_id_string!(#crate_name::#full_gid, API_HASH_NO_DOCS);

        pub fn api_hash() -> wire_weaver::ww_version::ApiHashPair<'static> {
            use wire_weaver::ww_version::ApiHash;
            wire_weaver::ww_version::ApiHashPair {
                no_docs: ApiHash::new(&API_HASH_NO_DOCS),
                with_docs: ApiHash::new(&API_HASH_WITH_DOCS),
            }
        }
    };
    let es0 = fixed_err_seq("INTROSPECT");
    // written in place: `Event`'s serializer would link every `EventKind`'s (SRV-54)
    let send_with = |ser: TokenStream| {
        if use_async {
            quote! { wire_weaver::EventOut::send_with(cx, |scratch| #ser.map(|bytes| bytes.len())).await }
        } else {
            quote! { wire_weaver::BlockingEventOut::send_with_blocking(cx, |scratch| #ser.map(|bytes| bytes.len())) }
        }
    };
    let send_chunk = send_with(quote! {
        ww_client_server::util::ser_stream_data_bytes_event(scratch, request.seq.0, &[], chunk)
    });
    let send_close = send_with(quote! {
        ww_client_server::util::ser_stream_sideband_event(scratch, request.seq.0, &[], ww_client_server::StreamSideband::Close)
    });
    let send_close = quote! {
        __ww_try!(wr, d, #send_close, #es0, ResponseSerFailed);
    };
    // compressed or not, the same answer: the data says what it is (`ww_self::compressed::MAGIC`)
    let send_all = quote! {
        for chunk in WW_SELF_BYTES.chunks(CHUNK) {
            __ww_try!(wr, d, #send_chunk, #es0, ResponseSerFailed);
        }
        #send_close
    };
    let send_all = quote! {
        const CHUNK: usize = 128; // TODO: auto-determine better chunk size
        #send_all
    };
    let handle_introspect = if enabled {
        quote! {
            RequestKind::Introspect => {
                #send_all
                WrAction::Deferred
            }
        }
    } else {
        quote! {
            RequestKind::Introspect => {
                #send_close
                WrAction::Deferred
            }
        }
    };
    (handle_introspect, api_hash)
}

pub(crate) struct IntrospectTs {
    pub(crate) introspect_bytes: TokenStream,
    pub(crate) no_docs_hash: TokenStream,
    pub(crate) with_docs_hash: TokenStream,
}

/// Introspection bytes and API hashes.
///
/// Traits and types known from the walked crates' snapshots are left out ([crate::transform::skip_known]), both in
/// what a device sends and in what a client embeds (a client puts them back at runtime). The API hash is calculated
/// over these bytes.
///
/// With `compression` the bytes are compressed (`ww_self::compressed`) after the hashes are taken: a device's API
/// identity doesn't depend on how it keeps its introspection data.
pub(crate) fn introspect_prepare(
    api_bundle: &ApiBundleOwned,
    crate_dirs: &[std::path::PathBuf],
    include_docs: bool,
    compression: IntrospectCompression,
) -> IntrospectTs {
    let resolver = ww_snapshots::Resolver::new().with_crate_dirs(crate_dirs);
    let sent = match crate::transform::skip_known(api_bundle, &resolver) {
        Ok(sent) => sent,
        // a dependency's snapshot file exists but cannot be read: never inline its whole API over this, stop the
        // build instead (EVO-82, the bug that silently bloated usb_io_fw's introspection data 50x)
        Err(e)
            if e.downcast_ref::<crate::transform::UnreadableDependencySnapshot>()
                .is_some() =>
        {
            panic!("{e}")
        }
        Err(e) => {
            eprintln!(
                "Failed to leave known traits and types out of the introspection data: {e:?}"
            );
            api_bundle.clone()
        }
    };
    crate::local_registry::cache_definitions(api_bundle, &sent, &resolver);
    crate::local_registry::cache_snapshots(&resolver);
    let mut contains_docs = ContainsDocs::default();
    contains_docs.visit_api_bundle(api_bundle);
    // a left out definition's reference carries its docs signature (EVO-69): docs too, of the dependencies
    contains_docs.contains |= sent.types.iter().any(|l| {
        matches!(l, TypeLocationOwned::SkippedFullVersion { signature, .. } if signature::docs_part(signature).is_some())
    }) || sent.traits.iter().any(|l| {
        matches!(l, ApiLevelLocationOwned::SkippedFullVersion { signature, .. } if signature::docs_part(signature).is_some())
    });
    // without docs: no doc comment, and references cut to their shape signatures, so a dependency's doc fix changes
    // neither these bytes nor API_HASH_NO_DOCS
    let no_docs = |bundle: &ApiBundleOwned| {
        let mut bundle = bundle.clone();
        signature::drop_docs(&mut bundle);
        bundle
    };

    let sent_no_docs = no_docs(&sent);
    let (sent_no_docs_bytes, no_docs_hash) = ser_hash_and_cache(&sent_no_docs, false);
    let (sent_with_docs_bytes, with_docs_hash) = if contains_docs.contains {
        ser_hash_and_cache(&sent, true)
    } else {
        (sent_no_docs_bytes.clone(), bytes_to_ts(&[]))
    };
    let introspect_bytes = if include_docs && contains_docs.contains {
        sent_with_docs_bytes
    } else {
        sent_no_docs_bytes
    };
    let introspect_bytes = match compression {
        IntrospectCompression::None => introspect_bytes,
        IntrospectCompression::Lz4 => ww_self::compressed::compress(&introspect_bytes),
    };
    IntrospectTs {
        introspect_bytes: bytes_to_ts(&introspect_bytes),
        no_docs_hash,
        with_docs_hash,
    }
}

fn ser_hash_and_cache(api_bundle: &ApiBundleOwned, contains_docs: bool) -> (Vec<u8>, TokenStream) {
    let api_bytes = api_bundle.to_ww_bytes_owned().unwrap();
    // wire_weaver_client/src/local_registry.rs hashes downloaded bundles with the same function
    let hash = ww_self::signature::api_hash(&api_bytes);
    let hash = &hash[..];
    crate::local_registry::cache_api_bundle(api_bundle, contains_docs, hash);
    (api_bytes, bytes_to_ts(hash))
}

fn bytes_to_ts(bytes: &[u8]) -> TokenStream {
    let bytes_len = bytes.len();
    quote! {
        [u8; #bytes_len] = [ #(#bytes),* ]
    }
}

#[derive(Default)]
struct ContainsDocs {
    contains: bool,
}

impl Visit<'_> for ContainsDocs {
    fn visit_docs(&mut self, docs: &Vec<String>) {
        if !docs.is_empty() {
            self.contains = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// A dependency's snapshot (here `ww_si`'s) that exists in an old model format (EVO-82) must stop codegen with
    /// the same re-save error the checker and `ww_snapshots`' readers give, never fall back to inlining its whole
    /// API into the introspection data: that fallback once bloated usb_io_fw's `WW_SELF_BYTES` 50x and overflowed
    /// its flash with no error or warning.
    #[test]
    #[should_panic(expected = "this tool reads")]
    fn an_unreadable_dependency_snapshot_stops_codegen_instead_of_inlining() {
        let crate_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/skipped_dep_api");
        let (full, _hints, _unreasoned, _paths) =
            crate::load_with_hints(&crate_path, Some("Port".into()), false).unwrap();
        let ww_si_idx = full
            .ext_crates
            .iter()
            .position(|c| c.crate_id == "ww_si")
            .expect("skipped_dep_api depends on ww_si");
        let version = full.ext_crates[ww_si_idx].clone();

        let dir = std::env::temp_dir()
            .join("ww_core_introspect_tests")
            .join(format!("unreadable_dep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let snapshots_dir = dir.join(ww_snapshots::SNAPSHOTS_DIR);
        std::fs::create_dir_all(&snapshots_dir).unwrap();
        let file_name = format!(
            "{}_{}_{}_{}.ron",
            version.crate_id,
            version.version.major.0,
            version.version.minor.0,
            version.version.patch.0
        );
        std::fs::write(
            snapshots_dir.join(file_name),
            "// an old snapshot\n\
             // shrink_wrap sha256: 0000000000000000000000000000000000000000000000000000000000000000\n\
             (ww_self_version: (major: 0, minor: 1, patch: 0))\n",
        )
        .unwrap();

        let crate_dirs = vec![dir.clone()];
        // panics: ww_si's snapshot exists but is unreadable, so this must refuse rather than inline it
        introspect_prepare(&full, &crate_dirs, false, IntrospectCompression::None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
