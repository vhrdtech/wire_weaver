//! `ww_user_info` end to end: a mock server on `ww_device::Server` through `tests_common::start_device`, the
//! generated client through the real host event loop.

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use tests_common::TestDevice;
    use wire_weaver::prelude::*;
    use wire_weaver_client::CallError;
    use ww_user_info::{UserInfoCaps, UserInfoError};

    /// Flash of the mock: what survives a power cycle, and how often it was written.
    #[derive(Default)]
    struct Storage {
        name: String,
        notes: String,
        flash_writes: u32,
        /// The next write fails as if the flash could not be programmed.
        fail_next: bool,
        /// The flash is erasing, writes are refused.
        busy: bool,
    }

    mod server {
        use super::*;
        use tests_common::TestProcessEvents;
        use wire_weaver::BlockingEventOut;

        pub struct Server {
            pub storage: Arc<Mutex<Storage>>,
            pub name_max: u16,
            pub name_copy: String,
            pub notes_copy: String,
        }

        impl Server {
            /// A write is saved before the call answers, and the flash is touched only when the value changed.
            fn save(
                &self,
                value: &str,
                max: usize,
                field: impl Fn(&mut Storage) -> &mut String,
            ) -> SetResult<UserInfoError<'static>> {
                if value.len() > max {
                    return SetError(UserInfoError::TooLong { max: max as u16 });
                }
                let mut st = self.storage.lock().unwrap();
                if st.busy {
                    return SetError(ww_error::CommonError::Busy.into());
                }
                if *field(&mut st) == value {
                    return Set;
                }
                if std::mem::take(&mut st.fail_next) {
                    return SetError(ww_error::CommonError::Fault { code: 7 }.into());
                }
                *field(&mut st) = value.to_string();
                st.flash_writes += 1;
                Set
            }
        }

        #[allow(non_snake_case)]
        impl Server {
            fn get_user_CAPABILITIES(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<UserInfoCaps, ()> {
                Value(UserInfoCaps {
                    name_max: self.name_max,
                    notes_max: 256,
                    persistent: true,
                })
            }

            fn set_user_name(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: MaxLen<&'_ str, 32>,
            ) -> SetResult<UserInfoError<'_>> {
                self.save(&value, self.name_max as usize, |s| &mut s.name)
            }

            fn get_user_name(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<MaxLen<&'_ str, 32>, UserInfoError<'_>> {
                // returned from the struct's own copy: the storage is behind a mutex
                self.name_copy = self.storage.lock().unwrap().name.clone();
                Value(MaxLen::new_unchecked(&self.name_copy))
            }

            fn set_user_notes(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: MaxLen<&'_ str, 256>,
            ) -> SetResult<UserInfoError<'_>> {
                self.save(&value, 256, |s| &mut s.notes)
            }

            fn get_user_notes(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<MaxLen<&'_ str, 256>, UserInfoError<'_>> {
                self.notes_copy = self.storage.lock().unwrap().notes.clone();
                Value(MaxLen::new_unchecked(&self.notes_copy))
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                user_info_api :: UserInfoTest for super::Server,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
            );
        }

        impl TestProcessEvents for Server {
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
    }

    mod std_client {
        use wire_weaver_client::{ClientConfig, Commander, WwClient};

        pub struct StdClient {
            pub cmd: Commander,
        }

        impl WwClient for StdClient {
            fn default_config() -> ClientConfig {
                ClientConfig::default()
            }

            fn from_cmd(cmd: Commander) -> Self {
                Self { cmd }
            }
        }

        mod api_client {
            wire_weaver::ww_codegen!(
                user_info_api :: UserInfoTest for super::StdClient,
                client = "std_client",
            );
        }
    }

    use std_client::StdClient;

    async fn connect(device: &TestDevice) -> StdClient {
        StdClient::config(|c| c.in_process_path(device.path()))
            .connect()
            .await
            .expect("connect")
    }

    fn start(path: &str, name_max: u16) -> (TestDevice, Arc<Mutex<Storage>>) {
        let _ = tracing_subscriber::fmt::try_init();
        let storage = Arc::new(Mutex::new(Storage::default()));
        let device = tests_common::start_device(
            path,
            server::Server {
                storage: storage.clone(),
                name_max,
                name_copy: String::new(),
                notes_copy: String::new(),
            },
            user_info_api::USER_INFO_TEST_FULL_GID,
            server::api_impl::api_hash(),
        );
        (device, storage)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn capabilities_and_roundtrip() {
        let (device, storage) = start("user_info/roundtrip", 16);
        let client = connect(&device).await;
        let caps = client.user().read_CAPABILITIES().read().await.unwrap();
        assert_eq!(
            (caps.name_max, caps.notes_max, caps.persistent),
            (16, 256, true)
        );

        client
            .user()
            .write_name(MaxLen::new("bench rePort".to_string()).unwrap())
            .write()
            .await
            .unwrap();
        client
            .user()
            .write_notes(MaxLen::new("left USB port".to_string()).unwrap())
            .write()
            .await
            .unwrap();
        assert_eq!(
            &*client.user().read_name().read().await.unwrap(),
            "bench rePort"
        );
        assert_eq!(
            &*client.user().read_notes().read().await.unwrap(),
            "left USB port"
        );
        assert_eq!(storage.lock().unwrap().name, "bench rePort");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn too_long_for_this_device_names_the_limit() {
        let (device, storage) = start("user_info/too_long", 8);
        let client = connect(&device).await;
        let r = client
            .user()
            .write_name(MaxLen::new("0123456789".to_string()).unwrap())
            .write()
            .await;
        assert!(
            matches!(
                r,
                Err(CallError::Device(
                    ww_user_info::UserInfoErrorOwned::TooLong { max: 8 }
                ))
            ),
            "{r:?}"
        );
        assert_eq!(
            storage.lock().unwrap().flash_writes,
            0,
            "a refused write touches nothing"
        );
        client
            .user()
            .write_name(MaxLen::new("01234567".to_string()).unwrap())
            .write()
            .await
            .unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn flash_is_written_only_when_the_value_changed() {
        let (device, storage) = start("user_info/wear", 32);
        let client = connect(&device).await;
        let name = || MaxLen::new("unit 7".to_string()).unwrap();
        client.user().write_name(name()).write().await.unwrap();
        client.user().write_name(name()).write().await.unwrap();
        client.user().write_name(name()).write().await.unwrap();
        assert_eq!(storage.lock().unwrap().flash_writes, 1);
        client
            .user()
            .write_name(MaxLen::new("unit 8".to_string()).unwrap())
            .write()
            .await
            .unwrap();
        assert_eq!(storage.lock().unwrap().flash_writes, 2);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn busy_and_fault_leave_the_old_value() {
        use ww_error::CommonErrorOwned;
        use ww_user_info::UserInfoErrorOwned;
        let (device, storage) = start("user_info/busy_fault", 32);
        let client = connect(&device).await;
        client
            .user()
            .write_name(MaxLen::new("old".to_string()).unwrap())
            .write()
            .await
            .unwrap();

        storage.lock().unwrap().busy = true;
        let r = client
            .user()
            .write_name(MaxLen::new("new".to_string()).unwrap())
            .write()
            .await;
        assert!(
            matches!(
                r,
                Err(CallError::Device(UserInfoErrorOwned::Common(
                    CommonErrorOwned::Busy
                )))
            ),
            "{r:?}"
        );
        storage.lock().unwrap().busy = false;

        storage.lock().unwrap().fail_next = true;
        let r = client
            .user()
            .write_name(MaxLen::new("new".to_string()).unwrap())
            .write()
            .await;
        assert!(
            matches!(
                r,
                Err(CallError::Device(UserInfoErrorOwned::Common(
                    CommonErrorOwned::Fault { code: 7 }
                )))
            ),
            "{r:?}"
        );
        assert_eq!(&*client.user().read_name().read().await.unwrap(), "old");
        assert_eq!(storage.lock().unwrap().flash_writes, 1);
    }
}
