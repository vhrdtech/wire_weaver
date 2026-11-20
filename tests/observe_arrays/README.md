# observe_arrays

Observed properties under resource arrays (API-63), end to end: the generated server with an element table per
observed element, polled every millisecond, and the generated client through the real host event loop
(`tests_common`). Each element is observed by its own path and gets its own updates; a removed element ends
its observers.
