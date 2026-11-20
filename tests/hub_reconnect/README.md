# hub_reconnect

Tests clients of a device behind the ww hub when the hub goes (CLT-40) or hands over to a newer one (SRV-30). Serves the device as `inproc:` for an in-process hub and on a local socket (`ipc:`) for a hub in another process that is killed and started again. Its API is `hub_reconnect_api`.
