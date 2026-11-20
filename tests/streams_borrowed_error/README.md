# streams_borrowed_error

Stream functions whose error type borrows (FEATURES.md API-35: stream-functions-with-a-borrowed-error-type).
The sync server has plain and counted streams, the async one adds a reliable stream; both open a stream and
refuse a second one with that error.
