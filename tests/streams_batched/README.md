# streams_batched

Batched streams end to end (FEATURES.md API-41: batched-streams-and-properties). Plain, counted and reliable
streams whose items the device collects in a `ww_stream::batch::Batcher` and sends as one `Series` per event.

The typed client gives one item per batch entry, each with its own time, or whole batches. Runs the generated
async server against the real host event loop (`tests_common`).
