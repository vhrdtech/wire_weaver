#![no_main]

//! The link setup hello (EVO-23): random bytes never panic `Hello::from_bytes`, the `find` of any medium
//! (`hello::usb`, `udp`, `rtt`, `ws`) or `Message::decode`, and every hello found is found again in its own frame.

use libfuzzer_sys::fuzz_target;
use ww_link::{Hello, Kind, Message, Role, hello};

fuzz_target!(|data: &[u8]| {
    if let Ok(h) = Hello::from_bytes(data) {
        // a reader takes the first four generations and the token: written back, the known fields agree
        let again = Hello::from_bytes(&h.to_bytes()).unwrap();
        assert_eq!(again, h);
    }
    if let Some(h) = hello::usb::find(data) {
        assert_eq!(hello::usb::find(&hello::usb::frame(&h)), Some(h));
    }
    if let Some(h) = hello::udp::find(data) {
        assert_eq!(hello::udp::find(&hello::udp::frame(&h)), Some(h));
    }
    for role in [Role::Host, Role::Device] {
        if let Some((h, at)) = hello::rtt::find(data, role) {
            assert!(at.end <= data.len());
            assert_eq!(hello::rtt::find(&hello::rtt::frame(&h), role).map(|(h, _)| h), Some(h));
        }
    }
    if let Some(h) = hello::ws::find(data) {
        assert_eq!(hello::ws::find(&hello::ws::frame(&h)), Some(h));
    }
    let _ = Message::decode(Kind::Hello as u8, data);
});
