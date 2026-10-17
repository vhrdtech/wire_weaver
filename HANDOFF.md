# Handoff: ww-logo-opus (logo redraw, P2534)

Done (commit 8c1f9cef): generator `docs/assets/src/logos.py` (`just logos`), new light/dark SVG + 2x PNG of both logos, old SVGs kept as `*_snake*.svg`, CHANGELOG entries (wire_weaver, shrink_wrap), `PNGs` allowed in typos.toml. Review sheet sent: /tmp/logo-rework/review/logo_opus_round1.png.

State: parked for the user's review. Not ready until the user approves.

Checks used (not in repo, in the job tmp): pixel check of the 8 copper ends (copper never inside the insulation, copper highlight 1.75..23 units out of the cut, insulation highlight colour right up to the cut), text ink box vs sleeve walls (9.5 / 9.5 padding, centre 314 = sleeve centre), 50% overlay vs the original cable (0.8% px differ in the cable band, highlights only), Chromium render matches rsvg.

Knobs if the user wants changes: SLEEVE_* constants (mouth height, shoulder length, round end), relief layers in `relief()` and Theme.relief_hi/relief_sh, rim fade stops.

Before `tpm work ready`: `git rm HANDOFF.md`.
