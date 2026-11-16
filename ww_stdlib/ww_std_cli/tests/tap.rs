//! The tap names calls of a standard library trait: by the API's own path on a full connection, by the trait's
//! `ww_global` name when the client is connected `--traits-only`.

use wire_weaver::prelude::*;
use wire_weaver_cli::cmd::tap::Layer;
use wire_weaver_cli::cmd::tap::test_support::{replayed, replayed_view, tapped_session};
use ww_user_info::{NameMax, NotesMax, UserInfoCaps, UserInfoError};

/// A device whose user info is fixed: the name is "tapped".
struct Server;

#[allow(non_snake_case)]
impl Server {
    fn set_user_name(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _value: MaxLen<&'_ str, 32>,
    ) -> SetResult<UserInfoError<'_>> {
        Set
    }

    fn get_user_name(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
    ) -> GetResult<MaxLen<&'_ str, 32>, UserInfoError<'_>> {
        Value(MaxLen::new_unchecked("tapped"))
    }

    fn set_user_notes(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _value: MaxLen<&'_ str, 256>,
    ) -> SetResult<UserInfoError<'_>> {
        Set
    }

    fn get_user_notes(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
    ) -> GetResult<MaxLen<&'_ str, 256>, UserInfoError<'_>> {
        Value(MaxLen::new_unchecked(""))
    }
}

impl api_impl::UserInfoTestConsts for Server {
    fn user_capabilities(&self) -> UserInfoCaps {
        UserInfoCaps {
            name_max: NameMax::bounded(32),
            notes_max: NotesMax::bounded(256),
            persistent: false,
        }
    }
}

mod api_impl {
    wire_weaver::ww_codegen!(
        user_info_api :: UserInfoTest for super::Server,
        server = true, no_alloc = true, use_async = false,
        method_model = "_=immediate",
        property_model = "_=get_set",
        const_model = "CAPABILITIES=fn",
    );
}

impl tests_common::TestProcessEvents for Server {
    type Medium = ();

    fn process_request_bytes<'a>(
        &mut self,
        bytes: &[u8],
        scratch: &'a mut [u8],
        out: &mut impl BlockingEventOut,
        medium: (),
    ) -> Result<&'a [u8], ShrinkWrapError> {
        self.process_request_bytes(bytes, scratch, out, medium)
    }
}

fn user_info_device(path: &str) -> tests_common::TestDevice {
    tests_common::start_device_usb(
        path,
        64,
        Server,
        user_info_api::USER_INFO_TEST_FULL_GID,
        user_info_api::USER_INFO_TEST_WIRE_VERSION,
        api_impl::api_hash(),
    )
}

/// API-20: a call to a global trait is shown with its name, from the device's API when connected in full and from
/// the `ww_global` registry when connected `--traits-only`, and decoded the same from a capture.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn global_trait_calls_are_named_and_decoded() {
    for (traits_only, name) in [(false, "global"), (true, "global_traits_only")] {
        let device = user_info_device(&format!("std_cli/tap_{name}"));
        let (printed, path) = tapped_session(name, device.path(), traits_only, async |client| {
            let user = ww_report::clients::UserInfoClient::new(client);
            let n = user.read_name().read().await.unwrap();
            assert_eq!(&*n, "tapped");
        })
        .await;
        let resource = match traits_only {
            true => "ww_user_info::UserInfo.name",
            false => "user.name",
        };
        assert!(
            printed.contains(&format!("Request Read {resource}")),
            "{printed}"
        );
        assert!(
            printed.lines().any(
                |l| l.contains(&format!("Event Value {resource}  \"tapped\""))
                    && l.contains("reply to #")
            ),
            "{printed}"
        );
        assert_eq!(replayed(&path, Layer::Api), printed, "{name}");
        if traits_only {
            // the call went out by the trait's ww_global id (API-20): 6 bytes, the crate name alone would take 12;
            // the name shown comes from the registry
            let with_bytes = replayed_view(&path, Layer::Api, true);
            let mut lines = with_bytes.lines();
            lines.find(|l| l.contains("Request Read ww_user_info::UserInfo.name"));
            let request_len = lines.next().unwrap().split_whitespace().skip(1).count();
            assert_eq!(request_len, 6, "{with_bytes}");
        }
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
