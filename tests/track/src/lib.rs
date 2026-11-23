//! `#[track(..)]` (API-71) end to end: a generated server's introspection data (`WW_SELF_BYTES`) carries each
//! stream's layout, dimensions and flags as `ww_self::Meta::track`, the trait's default settled into its streams, and
//! the series' item types have the channels a recorder needs.

#[cfg(test)]
mod tests {
    use ww_self::{
        ApiBundleOwned, ApiItemKindOwned, ApiLevelOwned, SeriesShape, TrackLayout, TrackOwned,
    };

    /// A server of the API, its handlers from `ww api scaffold`: only its introspection data is read here.
    mod server {
        use wire_weaver::prelude::*;

        pub struct Server {}

        #[expect(
            dead_code,
            reason = "only the introspection data is read, no request is handled"
        )]
        impl Server {
            // ---- track_api::Track at root ----

            /// Something seen, at an instant: the trait's default, points with the input and producer dimensions.
            ///
            /// Opens the stream (a new session): `Ok` / `Ready` starts it.
            fn events(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<()> {
                RpcResult::Ready(())
            }

            /// The host closed the stream.
            fn close_events(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {}

            fn sideband_events(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _sideband: ww_client_server::StreamSideband,
            ) -> Option<ww_client_server::StreamSideband> {
                None
            }

            /// A detection with a start and an end, and a severity in its flags word.
            ///
            /// Opens the stream (a new session): `Ok` / `Ready` starts it.
            fn detections(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<()> {
                RpcResult::Ready(())
            }

            /// The host closed the stream.
            fn close_detections(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {}

            fn sideband_detections(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _sideband: ww_client_server::StreamSideband,
            ) -> Option<ww_client_server::StreamSideband> {
                None
            }

            /// Left and right levels, two channels of a series.
            ///
            /// Opens the stream (a new session): `Ok` / `Ready` starts it.
            fn levels(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<()> {
                RpcResult::Ready(())
            }

            /// The host closed the stream.
            fn close_levels(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {}

            fn sideband_levels(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _sideband: ww_client_server::StreamSideband,
            ) -> Option<ww_client_server::StreamSideband> {
                None
            }

            /// Four probe voltages at once, four channels of a series.
            ///
            /// Opens the stream (a new session): `Ok` / `Ready` starts it.
            fn probes(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<()> {
                RpcResult::Ready(())
            }

            /// The host closed the stream.
            fn close_probes(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {}

            fn sideband_probes(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _sideband: ww_client_server::StreamSideband,
            ) -> Option<ww_client_server::StreamSideband> {
                None
            }

            /// A stream function, opened by calling it: also a stream a recorder records.
            ///
            /// Opens the stream (a new session): `Ok` / `Ready` starts it.
            fn watch(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _threshold: ww_si::Fixed<
                    ww_si::units::Item,
                    ww_si::prefix::One,
                    u8,
                    ww_si::Unbounded,
                >,
            ) -> RpcResult<()> {
                RpcResult::Ready(())
            }

            /// The host closed the stream.
            fn close_watch(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {}

            fn sideband_watch(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _sideband: ww_client_server::StreamSideband,
            ) -> Option<ww_client_server::StreamSideband> {
                None
            }

            // ---- track_api::Detector at detector ----

            /// Hits, spans by the trait's default.
            ///
            /// Opens the stream (a new session): `Ok` / `Ready` starts it.
            fn detector_hits(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<()> {
                RpcResult::Ready(())
            }

            /// The host closed the stream.
            fn close_detector_hits(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {}

            fn sideband_detector_hits(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _sideband: ww_client_server::StreamSideband,
            ) -> Option<ww_client_server::StreamSideband> {
                None
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                track_api :: Track for super::Server,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
            );
        }
    }

    fn bundle() -> ApiBundleOwned {
        use wire_weaver::shrink_wrap::DeserializeShrinkWrapOwned;
        let bytes = ww_self::compressed::bundle_bytes(&server::api_impl::WW_SELF_BYTES).unwrap();
        ApiBundleOwned::from_ww_bytes_owned(&bytes).unwrap()
    }

    fn track<'a>(level: &'a ApiLevelOwned, name: &str) -> Option<&'a TrackOwned> {
        let item = level.items.iter().find(|i| i.ident == name).unwrap();
        item.meta.as_ref().and_then(|m| m.track.as_ref())
    }

    fn of(layout: TrackLayout, dims: &[&str], flags: bool) -> TrackOwned {
        TrackOwned {
            layout,
            dims: dims.iter().map(|d| (*d).to_owned()).collect(),
            flags,
        }
    }

    #[test]
    fn streams_carry_their_layout() {
        let bundle = bundle();
        let root = &bundle.root;
        let points = of(TrackLayout::Points, &["input", "producer"], false);
        assert_eq!(track(root, "events"), Some(&points));
        assert_eq!(track(root, "watch"), Some(&points), "a stream function too");
        assert_eq!(
            track(root, "detections"),
            Some(&of(TrackLayout::Spans, &["input"], true))
        );
        assert_eq!(
            track(root, "levels"),
            Some(&of(TrackLayout::Series, &["input"], false))
        );
        assert_eq!(
            track(root, "probes"),
            Some(&of(TrackLayout::Series, &[], false))
        );
        assert_eq!(root.meta.as_ref().unwrap().track.as_ref(), Some(&points));

        let detector = root
            .items
            .iter()
            .find(|i| i.ident == "detector")
            .unwrap()
            .get_as_level(&bundle)
            .unwrap();
        assert_eq!(
            track(detector, "hits"),
            Some(&of(TrackLayout::Spans, &[], false)),
            "a sub-trait's own default, not the root's"
        );
    }

    /// The channels of each series, from its item type: a struct of two `Fixed`, an array of four. The device leaves
    /// `Fixed` out (a dependency's), so they are known once a host puts its definition back.
    #[test]
    fn series_have_channels() {
        let mut bundle = bundle();
        let ty = |bundle: &ApiBundleOwned, name: &str| {
            let item = bundle.root.items.iter().find(|i| i.ident == name).unwrap();
            let ApiItemKindOwned::Stream { ty, .. } = &item.kind else {
                panic!("{name} is a stream");
            };
            ty.clone()
        };
        let (levels, probes) = (ty(&bundle, "levels"), ty(&bundle, "probes"));
        assert_eq!(
            bundle.series_shape(&levels).unwrap(),
            SeriesShape::Unresolved
        );
        tests_common::resolver().inline_skipped(&mut bundle);
        assert_eq!(
            bundle.series_shape(&levels).unwrap(),
            SeriesShape::Channels(2)
        );
        assert_eq!(
            bundle.series_shape(&probes).unwrap(),
            SeriesShape::Channels(4)
        );
    }
}
