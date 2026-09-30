use crate::codegen::util::ErrorSeq;
use convert_case::{Case, Casing};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use sha2::Digest;
use shrink_wrap::SerializeShrinkWrapOwned;
use ww_self::ApiBundleOwned;
use ww_self::visit::Visit;
use ww_self::visit_mut::VisitMut;

pub(crate) fn introspect(
    api_bundle: &ApiBundleOwned,
    enabled: bool,
    include_docs: bool,
    use_async: bool,
    error_seq: &mut ErrorSeq,
) -> (TokenStream, TokenStream) {
    let IntrospectTs {
        introspect_bytes,
        no_docs_hash,
        with_docs_hash,
    } = introspect_prepare(api_bundle, include_docs);
    let root = &api_bundle.root;
    let crate_name = Ident::new(root.crate_name(api_bundle).unwrap(), Span::call_site());
    let full_gid = Ident::new(
        &format!("{}_FULL_GID", root.trait_name).to_case(Case::Constant),
        Span::call_site(),
    );
    let api_hash = quote! {
        pub const API_HASH_NO_DOCS: #no_docs_hash;
        pub const API_HASH_WITH_DOCS: #with_docs_hash;
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
    if !use_async {
        // TODO: sync variant of MessageSink
        return (quote! {}, api_hash);
    }
    let es0 = error_seq.next();
    let es1 = error_seq.next();
    let handle_introspect = if enabled {
        quote! {
            RequestKind::Introspect => {
                pub const WW_SELF_BYTES: #introspect_bytes;
                for chunk in WW_SELF_BYTES.chunks(128) { // TODO: auto-determine better chunk size
                    let event = Event {
                        seq: request.seq,
                        result: Ok(EventKind::StreamData { path: RefVec::Slice { slice: &[] }, data: TailBytes(chunk) }),
                    };
                    wr.reset();
                    event.ser_shrink_wrap(wr).map_err(|_| Error::new(#es0, ErrorKind::ResponseSerFailed))?;
                    let event_bytes = wr.finish().map_err(|_| Error::new(#es0, ErrorKind::ResponseSerFailed))?;
                    msg_tx.send(event_bytes).await.map_err(|_| Error::new(#es1, ErrorKind::ResponseSerFailed))?;
                }
                let event = Event {
                    seq: request.seq,
                    result: Ok(EventKind::StreamSideband { path: RefVec::Slice { slice: &[] }, sideband: ww_client_server::StreamSideband::Close }),
                };
                wr.reset();
                event.ser_shrink_wrap(wr).map_err(|_| Error::new(#es0, ErrorKind::ResponseSerFailed))?;
                let event_bytes = wr.finish().map_err(|_| Error::new(#es0, ErrorKind::ResponseSerFailed))?;
                msg_tx.send(event_bytes).await.map_err(|_| Error::new(#es1, ErrorKind::ResponseSerFailed))?;
                Ok(WrAction::Deferred)
            }
        }
    } else {
        quote! {
            RequestKind::Introspect => {
                let event = Event {
                    seq: request.seq,
                    result: Ok(EventKind::StreamSideband { path: RefVec::Slice { slice: &[] }, sideband: ww_client_server::StreamSideband::Close }),
                };
                wr.reset();
                event.ser_shrink_wrap(wr).map_err(|_| Error::new(#es0, ErrorKind::ResponseSerFailed))?;
                let event_bytes = wr.finish().map_err(|_| Error::new(#es0, ErrorKind::ResponseSerFailed))?;
                msg_tx.send(event_bytes).await.map_err(|_| Error::new(#es1, ErrorKind::ResponseSerFailed))?;
                Ok(WrAction::Deferred)
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
/// Traits and types known from embedded snapshots are left out ([crate::transform::skip_known]), both in what a device
/// sends and in what a client embeds (a client puts them back at runtime). The API hash is calculated over these bytes.
pub(crate) fn introspect_prepare(api_bundle: &ApiBundleOwned, include_docs: bool) -> IntrospectTs {
    let sent = match crate::transform::skip_known(api_bundle) {
        Ok(sent) => sent,
        Err(e) => {
            eprintln!(
                "Failed to leave known traits and types out of the introspection data: {e:?}"
            );
            api_bundle.clone()
        }
    };
    let mut contains_docs = ContainsDocs::default();
    contains_docs.visit_api_bundle(api_bundle);
    let no_docs = |bundle: &ApiBundleOwned| {
        let mut bundle = bundle.clone();
        DropDocs.visit_api_bundle(&mut bundle);
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
    IntrospectTs {
        introspect_bytes: bytes_to_ts(&introspect_bytes),
        no_docs_hash,
        with_docs_hash,
    }
}

fn ser_hash_and_cache(api_bundle: &ApiBundleOwned, contains_docs: bool) -> (Vec<u8>, TokenStream) {
    let api_bytes = api_bundle.to_ww_bytes_owned().unwrap();
    // TODO: calculate api signature properly?
    // NOTE: wire_weaver_client/src/local_registry.rs hashes downloaded bundles the same way, keep in sync
    let hash = sha2::Sha256::digest(&api_bytes);
    let hash = &hash[..8];
    crate::local_registry::cache_api_bundle(api_bundle, contains_docs, hash);
    (api_bytes, bytes_to_ts(hash))
}

fn bytes_to_ts(bytes: &[u8]) -> TokenStream {
    let bytes_len = bytes.len();
    quote! {
        [u8; #bytes_len] = [ #(#bytes),* ]
    }
}

struct DropDocs;

impl VisitMut for DropDocs {
    fn visit_docs(&mut self, docs: &mut Vec<String>) {
        docs.clear();
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
