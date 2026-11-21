# ww_trace_context

> W3C trace context types as WireWeaver requests carry them

`TraceContext` is a W3C `traceparent` (version 00): the trace id, the caller's span id and the flags, 25 bytes on
the wire. A client puts it in a request's context, and a server hands it to the handler in `wire_weaver::CallContext`,
so the spans a request makes join the caller's trace. The types are `no_std` and need no allocator; `wire_weaver`
re-exports them. The crate keeps its own API snapshots, so snapshots of crates that use the types can refer to them.

See the [main repo](https://github.com/vhrdtech/wire_weaver) for the full picture and
[documentation](https://vhrdtech.github.io/wire_weaver) for how requests carry their context.
