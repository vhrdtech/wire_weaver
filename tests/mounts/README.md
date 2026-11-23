# mounts

Server and client of mount slots (SRV-76): the `mounts_api` TV answers the presence of its hub and extension slots, hands requests under a slot's `api` to the slot's handler with the slot's path taken off, refuses an absent slot with `Unavailable`, and names its extensions through `ext_keys`. The generated and dynamic clients reach the slots.
