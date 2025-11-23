# wire_weaver_usb_link

Maximum message length is limited to `min(u32::MAX, max_remote_message_len)`,
regardless of the USB endpoint maximum packet size (commonly 64 (FS) or 512 (HS bulk) or 1024 (HS IRQ)).