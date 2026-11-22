# reconnect_revision

Tests reconnects onto another revision of the device's API on the same wire line (CLT-81), end to end on the simulated WebSocket and tokio's paused clock: a keep-trying connection, `Commander::reconnect` and `Commander::reconnect_as`. A device of a compatible revision (a method added) resumes every stream and observer; one whose stream item type changed ends that stream only and refuses its calls. Its APIs are `reconnect_revision_api` (w1.0), `reconnect_revision_added_api` and `reconnect_revision_changed_api` (both w1.1).
