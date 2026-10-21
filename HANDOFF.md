# Handoff: session ww-hub

Plan: ~/git/tpm/prj/P2401-wireweaver/design/ww-hub.md (the design, read first). Umbrella FEATURES SRV-13.

Done (step 1, 21 Oct 2026): design written, FEATURES items SRV-13 (umbrella), SRV-28..33, CLT-38..40, TR-16, TOOL-17, TOOL-18 as planned items; tpm tasks P2401#21oct-2101 (API-21), #21oct-2102 (CLT-38), #21oct-2102b (TR-16 + SRV-28, kept by this session), #21oct-2102c (SRV-29), #21oct-2102d (CLT-39), #21oct-2102e (CLT-40 + SRV-30), #21oct-2103 (TOOL-17), #21oct-2103b (TOOL-18), #21oct-2103c (SRV-32), #21oct-2103d (SRV-33 design), #21oct-2103e (hardware check), #21oct-2039 rewritten as SRV-31. INT-12 was claimed by mistake (INT is introspection) and is unused.

Next (this session): TR-16 local socket transport, then SRV-28 ww_hub_api + ww_hub process (after API-21 for owned returns in the hub API server).

Decisions: one listener, WWHB preamble picks hub or device (frozen); lock files with std File::try_lock; open devices on demand + linger; newest hub wins via handover; WW_HUB=auto|off|required; federation via ssh-forwarded socket first.
