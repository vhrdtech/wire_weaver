#![no_main]

//! The link setup hello (EVO-23): random bytes never panic `Hello::from_bytes`, `hello::usb::find` or
//! `Message::decode`, and every hello read back writes the same first bytes again.

use libfuzzer_sys::fuzz_target;
use ww_link::{Hello, Kind, Message, hello};

fuzz_target!(|data: &[u8]| {
    if let Ok(h) = Hello::from_bytes(data) {
        // a reader takes the first four generations and the token: written back, the known fields agree
        let again = Hello::from_bytes(&h.to_bytes()).unwrap();
        assert_eq!(again, h);
    }
    for prefix_len in 0..2 {
        if let Some(h) = hello::usb::find(data, prefix_len) {
            assert_eq!(hello::usb::find(&hello::usb::frame(&h), 0), Some(h));
        }
    }
    let _ = Message::decode(Kind::Hello as u8, data);
});
