use crate::codegen::util::ErrorSeq;
use proc_macro2::TokenStream;
use quote::quote;
use sha2::Digest;
use shrink_wrap::SerializeShrinkWrap;
use ww_self::ApiBundleOwned;

pub(crate) fn introspect(
    api_bundle: &ApiBundleOwned,
    enabled: bool,
    use_async: bool,
    error_seq: &mut ErrorSeq,
) -> (TokenStream, TokenStream) {
    // let mut api_bundle_no_docs = api_bundle.clone();
    // visit_api_bundle_mut(&mut api_bundle_no_docs, &mut DropDocs {});
    let mut scratch = [0u8; 16_384]; // TODO: use Vec based BufWriter here
    let bytes = api_bundle.to_ww_bytes(&mut scratch).unwrap();
    let bytes_len = bytes.len();
    let ww_self_bytes_const = quote! {
        [u8; #bytes_len] = [ #(#bytes),* ]
    };

    // TODO: calculate api signature properly
    let sha256 = sha2::Sha256::digest(bytes);
    let short_hash = &sha256[..8];
    let api_signature = quote! { [u8; 8] = [ #(#short_hash),* ]};
    crate::local_registry::cache_api_bundle(&api_bundle, short_hash);

    let api_signature = quote! { pub const WW_API_SIGNATURE: #api_signature; };
    if !use_async {
        // TODO: sync variant of MessageSink
        return (quote! {}, api_signature);
    }
    let es0 = error_seq.next_err();
    let es1 = error_seq.next_err();
    let handle_introspect = if enabled {
        quote! {
            RequestKind::Introspect => {
                pub const WW_SELF_BYTES: #ww_self_bytes_const;
                for chunk in WW_SELF_BYTES.chunks(128).chain([&[][..]]) { // TODO: auto-determine better chunk size
                    let event = Event {
                        seq: request.seq,
                        result: Ok(EventKind::Introspect { ww_self_bytes_chunk: RefVec::new_bytes(chunk) }),
                    };
                    let event_bytes = event.to_ww_bytes(scratch_event).map_err(|_| Error::new(#es0, ErrorKind::ResponseSerFailed))?;
                    msg_tx.send(event_bytes).await.map_err(|_| Error::new(#es1, ErrorKind::ResponseSerFailed))?;
                }
                Ok(&[])
            }
        }
    } else {
        quote! {
            RequestKind::Introspect => {
                let event = Event {
                    seq: request.seq,
                    result: Ok(EventKind::Introspect { ww_self_bytes_chunk: RefVec::new_bytes(&[]) }),
                };
                let event_bytes = event.to_ww_bytes(scratch_event).map_err(|_| Error::new(#es0, ErrorKind::ResponseSerFailed))?;
                msg_tx.send(event_bytes).await.map_err(|_| Error::new(#es1, ErrorKind::ResponseSerFailed))?;
                Ok(&[])
            }
        }
    };
    (handle_introspect, api_signature)
}

// struct DropDocs {}
//
// impl ww_self::visitor::VisitMut for DropDocs {
//     fn visit_doc(&mut self, doc: &mut String) {
//         *doc = String::new();
//     }
// }
