//! `wire_weaver_server` on a calloop loop through `ww_calloop` (SRV-70): the loop runs on a thread of its own with
//! the server scheduled on its executor, clients connect over WebSocket and UDP from tokio. A mount table on the loop
//! reaches a backend on a Unix socket (SRV-78).

#[cfg(test)]
mod tests;
