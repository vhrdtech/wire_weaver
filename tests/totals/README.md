# totals

Tests of running totals over plain sinks (`ww_api::Totals<T>`, API-70): `TotalsSender` on the host,
`ww_stream::totals::TotalsRx` on the device, over a simulated UDP link that drops and reorders datagrams, over the
simulated WebSocket, and with a second sender to a device that kept its state.
