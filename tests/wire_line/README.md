# wire_line

Link setup decides on the wire line, not the crate version (FEATURES.md EVO-25: link-setup-decides-on-the-wire-line).
Firmware of `wire_line_api` 0.3.0 at wire w2.3 (`wire_line_old_api`) and a host built against 0.4.0 at w2.4, a
crate breaking bump on the same line, connect; the host learns the device's wire version from link setup and
refuses the new method before sending anything. The same firmware on another line is refused, naming both
lines.
