---
version: 1
slug: "web-index-html"
primary_target: "web/index.html"
related_targets: []
---

# ClusterScope Web — Overview & Trend

## Scope

- **Surface**: the embedded web dashboard at `web/index.html`, both views (总览 / 趋势).
- **Visitor mode**: Operate. The visitor completes a task: judge cluster health, find a free GPU, see who occupies a card, inspect history. Expression serves scanability.
- **Audience**: lab members and small-team cluster operators checking a GPU cluster from a desk browser, several times a day, next to a terminal.
- **Job / action**: read status in one gaze; select a node and GPU; inspect its processes; switch to trends; log in when the server requires it.
- **States**: online / degraded / offline nodes, busy / idle GPUs, hot temperature, empty cluster, loading, error, unauth, long process commands, many nodes.
- **Constraints**: dependency-free static assets embedded in the server; same-origin CSP; no external fonts or CDNs; preserve REST/WS contracts and keyboard access; reduce-motion respected.

## Direction contract

**THESIS** — The cluster is a live departure board: every node and GPU sits in a fixed row and cell, values change in place, and a flip happens only when a value actually moves. It refuses the rounded card grid with one neon accent.

**OWN-WORLD** — Matte charcoal board (`#141516`) inside a steel frame (`#2c2f31`), ivory type (`#f2efe6`), amber attention (`#f0b429`), oxide red for offline/critical (`#d64545`), muted green for online (`#5fbf8f`). Fixed cells carry a center seam; data is tabular mono, labels are a calm grotesk; one amber row lamp marks selection.

**STORY** — The visitor sees health and occupancy in one gaze, picks a node row, reads the GPUs and the processes on the selected card, then switches to history. Trust comes from columns that never move and changes that only repaint the cell.

**FIRST VIEWPORT** — 1440×900: 62px top bar (brand, 总览/趋势, live + WS, clock, refresh, login); four summary cells (online 4/5, GPU busy 11/32, average CPU 37%, max temp 78°C); board header plus five node rows with fixed columns and eight GPU cells each; the selected row opens a detail band with eight GPU rows and a process table; a footer status line. Selecting a node or GPU is the only primary action.

**FORM** — Dealt challenger 6, the split-flap concourse board, chosen over the assigned calibration sheet; not on the grounded list. `seed:fabc856adeb5c889`.

**FINISH** — unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance.
