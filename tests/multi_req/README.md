# multi_req

Tests of multi requests (`MultiCall`, `MultiRead`, `MultiWrite`) on a `multi_req = true` server, and a table of
malformed ones from the host: every one is answered with an error, for the whole request or per index, and
none panics the device.
