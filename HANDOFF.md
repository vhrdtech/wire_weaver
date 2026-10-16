# Handoff: ww-docs-levels-2 (DOC-8)

Reading levels for every section but SerDes. Design notes: ~/git/tpm/prj/P2401-wireweaver/design/docs-levels.md.

- Baseline: `uv run --with zensical zensical build --clean` reports 3 issues (ww_i2c.md anchors).
- Sections handed to forks: API; Std library; Transport + Evolution; Dev tools + Examples. Nav (zensical.toml),
  CHANGELOG, FEATURES.md, screenshots and design notes done in the main conversation.
- Not touched: docs/serdes/*, docs/types.md (session ww-tuple-structs).
