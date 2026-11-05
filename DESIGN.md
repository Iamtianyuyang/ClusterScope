---
name: ClusterScope
description: A split-flap status board for Linux GPU clusters.
colors:
  void: "#0d0e0f"
  frame: "#17191b"
  panel: "#121416"
  cell: "#0f1011"
  core-idle: "#24272a"
  ink: "#f2efe6"
  muted: "#a7aca9"
  dim: "#8b9194"
  line: "#2c2f31"
  line-soft: "#212427"
  steel: "#3a3d40"
  steel-hi: "#41464a"
  steel-lo: "#1f2326"
  amber: "#f0b429"
  amber-soft: "#3a2f13"
  red: "#ef6b6b"
  green: "#6fcf9f"
typography:
  display:
    fontFamily: "Barlow Condensed, Arial Narrow, sans-serif"
    fontSize: "27px"
    fontWeight: 600
    lineHeight: 1
    letterSpacing: "-0.02em"
    fontFeature: "tnum"
  title:
    fontFamily: "Barlow Condensed, Arial Narrow, sans-serif"
    fontSize: "21px"
    fontWeight: 600
    lineHeight: 1
    letterSpacing: "0.03em"
    fontFeature: "tnum"
  headline:
    fontFamily: "Barlow Condensed, Arial Narrow, Noto Sans SC, sans-serif"
    fontSize: "18px"
    fontWeight: 600
    lineHeight: 1.2
    letterSpacing: "-0.01em"
  board-data:
    fontFamily: "Barlow Condensed, Arial Narrow, sans-serif"
    fontSize: "14px"
    fontWeight: 600
    lineHeight: 1
    letterSpacing: "0.02em"
    fontFeature: "tnum"
  label:
    fontFamily: "Barlow Condensed, Arial Narrow, Noto Sans SC, sans-serif"
    fontSize: "10px"
    fontWeight: 500
    lineHeight: 1
    letterSpacing: "0.1em"
  body:
    fontFamily: "Helvetica Neue, Noto Sans SC, PingFang SC, system-ui, sans-serif"
    fontSize: "13px"
    fontWeight: 400
    lineHeight: 1.45
  mono:
    fontFamily: "ui-monospace, SFMono-Regular, Consolas, monospace"
    fontSize: "11px"
    fontWeight: 400
    lineHeight: 1.4
rounded:
  none: "0px"
  pill: "9999px"
spacing:
  hairline: "1px"
  xs: "3px"
  sm: "6px"
  md: "8px"
  lg: "12px"
  xl: "18px"
  xxl: "24px"
components:
  button-primary:
    backgroundColor: "{colors.amber}"
    textColor: "{colors.void}"
    typography: "{typography.label}"
    rounded: "{rounded.none}"
    padding: "9px 14px"
  button-secondary:
    backgroundColor: "{colors.frame}"
    textColor: "{colors.muted}"
    typography: "{typography.label}"
    rounded: "{rounded.none}"
    padding: "7px 10px"
  flap:
    backgroundColor: "{colors.cell}"
    textColor: "{colors.ink}"
    typography: "{typography.board-data}"
    rounded: "{rounded.none}"
    padding: "0 4px"
    height: "34px"
  flap-hot:
    backgroundColor: "{colors.cell}"
    textColor: "{colors.amber}"
    typography: "{typography.board-data}"
    rounded: "{rounded.none}"
    height: "34px"
  flap-critical:
    backgroundColor: "{colors.cell}"
    textColor: "{colors.red}"
    typography: "{typography.board-data}"
    rounded: "{rounded.none}"
    height: "34px"
  core-block:
    backgroundColor: "{colors.core-idle}"
    rounded: "{rounded.none}"
    size: "6px"
    height: "12px"
  core-block-busy:
    backgroundColor: "{colors.muted}"
    rounded: "{rounded.none}"
    size: "6px"
    height: "12px"
  core-block-hot:
    backgroundColor: "{colors.amber}"
    rounded: "{rounded.none}"
    size: "6px"
    height: "12px"
  summary-cell:
    backgroundColor: "{colors.frame}"
    textColor: "{colors.ink}"
    typography: "{typography.display}"
    rounded: "{rounded.none}"
    padding: "9px 14px"
  board-row:
    backgroundColor: "{colors.panel}"
    textColor: "{colors.ink}"
    rounded: "{rounded.none}"
    padding: "6px 12px"
  input:
    backgroundColor: "{colors.cell}"
    textColor: "{colors.ink}"
    typography: "{typography.body}"
    rounded: "{rounded.none}"
    padding: "8px 10px"
  tab:
    backgroundColor: "{colors.frame}"
    textColor: "{colors.muted}"
    typography: "{typography.label}"
    rounded: "{rounded.none}"
    padding: "0 16px"
  status-lamp:
    backgroundColor: "{colors.green}"
    rounded: "{rounded.pill}"
    size: "6px"
---

# Design System: ClusterScope

## Overview

**Creative North Star: "The Concourse Board"**

ClusterScope is a live departure board for a GPU cluster, not a dashboard of cards. Every node is a fixed row, every GPU a fixed cell, and every value changes in place. The page sits inside a beveled steel case; the data is printed on matte flaps in condensed ivory type; a single amber lamp marks the row you are reading. The world is a rail concourse status board seen from a working desk, not a control-room fantasy.

Density is the product. The board is designed so a lab member can read cluster health, GPU occupancy, temperature, and who owns a card in one gaze, then open a node's detail without losing column positions. The trend view is the same board language applied to plot cards: solid for utilization, dashed for memory, fixed cells, no floating chrome.

The system refuses the rounded card grid with a cyan glow, the hero-metric strip, and the "control panel" costume. It also refuses animated values on every refresh: the flip is a focused, rare signal, not a decorative loop. Data that does not move does not animate.

**Key Characteristics:**
- Fixed 14-column board with 8 GPU cells per node.
- Near-black field, ivory condensed data, amber selection, state-only green/red.
- Steel case with corner fasteners; inset flap depth and a 2px hinge seam.
- One authored motion: a 180ms flap turn on the focused row and selected GPU.
- A per-core CPU strip at the foot of the process pane, repeated under the trend legend.
- Server onboarding lives on the board: a quiet "pending" row for a host that is installing or has not reported yet.
- Square corners and hairlines; circles only for lamps and fasteners.

## Colors

A near-black board palette with one warm signal and three semantic states. The field carries the data; colour is spent on state and focus.

### Primary
- **Signal Amber** (#f0b429): the selected row lamp, selected GPU border and text, hot values, and the primary action. Amber is attention, never general decoration.

### Secondary
- **Online Green** (#6fcf9f): online lamps and the connected live label only.
- **Fault Red** (#ef6b6b): offline nodes, critical temperatures, and errors only.

### Neutral
- **Board Void** (#0d0e0f): the page field outside the case.
- **Steel Frame** (#17191b): the case and top bar ground; the frame edge uses #41464a top, #383d41 left, #1f2326 bottom, #262a2d right.
- **Panel** (#121416): board and chart surfaces.
- **Flap Cell** (#0f1011): the individual flap face.
- **Ivory Ink** (#f2efe6): primary data and headings.
- **Muted Steel** (#a7aca9): secondary data and status labels.
- **Dim Steel** (#8b9194): metadata, indices, units, and helper text.
- **Hairline** (#2c2f31): 1px rules, borders, and the 2px flap seam's lower highlight (#3a3e41).
- **Core Trough** (#24272a): idle CPU core blocks in the per-core strip.
- **Amber Wash** (#3a2f13): the selected process segment and quiet amber fills.

### Named Rules

**The One Lamp Rule.** Selection is a single amber row lamp, an amber node name, and at most a 1px amber border on the selected GPU cell. No second glow, no halo, no second accent.

**The State-Only Colour Rule.** Green, amber, and red only carry online, attention, and offline/critical meaning. They never become a decorative series.

**The Near-Black Rule.** The ground runs #0d0e0f to #17191b. Do not drift to blue-slate or warm cream; the board is neutral charcoal with a bevel, not a "tech" tint.

## Typography

**Display Font:** Barlow Condensed (with Arial Narrow, then the CJK fallback)
**Body Font:** Helvetica Neue / Noto Sans SC / PingFang SC (system stack)
**Label/Mono Font:** ui-monospace for process commands and the process table

**Character:** Condensed signage for every Latin letter and digit, with tabular numerals so the board never jitters; Chinese copy falls through to the platform CJK face rather than being squeezed into the Latin width. Process commands stay monospace because they are code, not signage.

### Hierarchy
- **Display** (600, 27px/1, -0.02em, tnum): the four summary values; the largest data voice on the screen.
- **Title** (600, 21px/1, 0.03em, tnum): the live clock.
- **Headline** (600, 18px/1.2, -0.01em): view headings; Chinese uses the CJK fallback.
- **Board data** (600, 14px/1, 0.02em, tnum): flap values, node names, GPU values; 11px for the detail row numbers and load/uptime.
- **Label** (500, 10px/1, 0.1em, uppercase): column headers, tab labels, summary labels, footer metadata.
- **Body** (400, 13px/1.45): the base document and dialog prose; 12px for dialog intro and error copy.
- **Mono** (400, 11px/1.4): process commands, PIDs, and the process table.

### Named Rules

**The Flap Face Rule.** All Latin and digits on the board use Barlow Condensed with `tnum`; the process table and command strings use mono. A system display face is a fallback only for scripts the condensed face does not cover.

**The Chinese Fallthrough Rule.** Never fake condensed Chinese. Set the CJK stack after the Latin face and let the browser fall through; the label keeps its weight and tracking.

## Layout

The app is a single steel case with a 60px top bar, the active view, and a 27px footer. On desktop the page is built to fit 1440×900 exactly, without a scrollbar.

The overview is a four-cell summary strip, a fixed 14-column board, and a two-pane detail band. A node that reports no GPUs collapses the detail to a single full-width CPU-process pane instead of leaving an empty GPU pane; if no node reports a GPU, the eight board headers merge into one centered `GPU` header while each no-GPU row keeps its `仅 CPU · 无 GPU` note. The selected node's per-core CPU strip sits at the foot of the process pane; the trend view repeats it under the legend, before the chart wall. Board columns are: node (1fr), status (94px), CPU (82px), memory (82px), eight GPU columns (1fr each), load (64px), uptime (94px); the GPU cell spans eight columns. Rows are 58px; the detail band is a 1.05fr / 0.95fr split; the GPU list and process table scroll inside their panes (max 260px) so the page stays a board.

At 1180px the uptime column drops. At 940px the board becomes stacked node panels: the header hides, CPU and memory each carry a persistent label, the GPU strip becomes 8 (then 4) columns, and the detail becomes one column. At 640px the process table becomes a divider list; the full command wraps. The trend view uses a full-width CPU card followed by a four-column GPU grid; on narrower screens the grid folds to three, two, then one column.

Spacing rhythm: 6px between cells, 8px between panels, 12px inside rows, 18px page gutters, 24px dialog padding. More space above a heading than below it.

## Elevation & Depth

Depth is material, not floating. The page sits in a 6px beveled steel case (light top and left, dark bottom and right) with 3px corner fasteners; the inner edge has a 1px dark seam. Flap cells are inset two-leaf modules: a subtle top-to-bottom gradient, a 2px center hinge with a dark upper half and a steel lower highlight, and inset shadows at the top and bottom. No element floats on a soft drop shadow.

The only neutral shadows are the login dialog and toast, which use an offset and a wide blur (#000 at 30–90px) to sit above the board. The selected GPU has a 1px amber border and no halo.

### Shadow Vocabulary
- **Flap inset** (`inset 0 1px 0 rgb(255 255 255 / .1), inset 0 -1px 0 #000, inset 0 7px 10px rgb(0 0 0 / .26), inset 0 -7px 10px rgb(255 255 255 / .02)`): every flap cell.
- **Case seam** (`inset 0 0 0 1px #0b0c0d, inset 0 1px 0 rgb(255 255 255 / .06)`): the inner edge of the steel case.
- **Dialog lift** (`0 30px 90px #000`): the login dialog.
- **Toast lift** (`0 14px 40px rgb(0 0 0 / .55)`): the transient status message.

### Named Rules

**The Board-in-a-Case Rule.** The steel case and inset flap are the depth system. A soft floating card in the middle of the board is a different world.

**The No-Halo Rule.** A selected cell is a 1px amber border, not a zero-offset glow. A state lamp is a solid dot, not a bloom.

## Shapes

Square by default. Panels, flaps, buttons, inputs, tabs, the dialog, and the chart cards have 0px radius. The only circles are the 5–6px status lamps and the 3px case fasteners. Rules are 1px hairlines; the flap hinge is 2px; bars are 2–5px. No rounded cards, no pills, no clipped organic shapes.

**The Square Rule.** If a corner needs a radius to look friendly, it is the wrong container for this world.

## Components

### Buttons
- **Shape:** square (0px).
- **Primary:** amber fill (#f0b429), void text (#0d0e0f), 11px uppercase label, 9px 14px padding. Hover brightens 8%; active scales to .98.
- **Secondary / Icon:** #1d1f21 fill, hairline border, muted text; 36px icon square or 7px 10px text; hover raises border and text to ivory. Focus is a 2px amber ring with 2px offset.

### Flap cells
- **Shape:** 34px high, square, 1px #2a2d2f border, inset depth, 2px hinge.
- **Value:** Barlow Condensed 600 14px, tabular, centered; `%` is part of the value.
- **States:** normal ivory; hot amber (util ≥85 or temp ≥70); critical red (temp ≥80); idle dim (util <5); selected 1px amber border. The flip is a 180ms rotateX on the face, only on the focused row and selected GPU, disabled under reduced motion.

### Board rows
- **Shape:** 58px row, 1px bottom hairline, no radius.
- **Selected:** #211f19 wash, amber lamp, amber node name, 1px inset amber top/bottom rules.
- **Offline:** opacity .48 on data; red status; all values `—`.
- **Hover:** #191b1d, pointer devices only.

### Pending onboarding row
- **Shape:** the same 58px board row and column positions, but #101214 ground; the node name is muted, the lamp and status dot are dim steel, and CPU/memory are `—` at .75 opacity.
- **Status:** `待接入` in dim steel; `安装中` in amber while the SSH install request is in flight. The dim lamp must not read as online green.
- **GPU span:** the eight GPU columns carry a single quiet note ending in a text `移除` button; while installing the remove action is hidden.
- **Behavior:** stored in the browser's `localStorage`; removed automatically when a node with the same `node_id`, hostname, or IP appears. The row is a receipt for an action, not a second node type.

### Add-server dialog
- **Shape:** the 420px dialog widened to 560px, same square steel panel, 18px padding, and dark backdrop.
- **Form:** two-column label/input grid (single column under 640px); 38px inputs on #0f1011 with 1px hairlines and the amber focus ring. SSH username, host, password, port, optional node ID, and Server gRPC address.
- **Actions:** text `取消` plus the amber `登录并安装`; while the request runs the whole form disables and the primary label becomes `正在登录并安装…`.
- **Copy:** state that the Server logs in as an ordinary user (no sudo/root), that it only writes into that user's HOME, and that the password is not stored; a second amber note warns when the page is served over plain HTTP.
- **Errors:** the same red inline error block, with the Server's real message (for example a refused SSH connection), never a generic toast alone.

### Summary strip
- **Shape:** four equal cells in a 1px hairline grid.
- **Type:** 10px uppercase label, 27px display value, 10px side note.
- **States:** side note green/amber/red only for availability or temperature attention.

### Process table / list
- **Desktop:** mono 11px table, sticky header, horizontal scroll, command ellipsis with a title; max 260px high.
- **Mobile:** the table is replaced by a `<ul>` divider list inside the pane; each entry shows PID, user, SM/CPU/memory, and the full command wrapped; max 320px high.
- **Mode switch:** a two-segment control (GPU 进程 / Top CPU), square, amber selected on #2a2519.

### CPU core strip
- **Shape:** 6×12px blocks, 2px gap, square; 32 or 64 cores wrap.
- **Placement:** foot of the process pane on overview; under the legend on trend.
- **Colors:** idle #24272a, busy muted steel, hot amber at ≥90%. All cores share one trough; state is the only colour.
- **Readout:** `CPU 核心 · N` label at the left; `平均 x% · 最高 y%` at the right; `role="img"` label on the strip; each block carries a title for pointer inspection.

### Trend cards
- **Shape:** square chart cards, 1px hairline, 140px plot (128px under 940).
- **Series:** solid ivory CPU or amber GPU utilization; dashed steel memory/VRAM; area fill at 4–7% of the series hue.
- **Header:** 11px name, 10px values, y-axis 100/50/0 at 9px, x-axis start/end at 9px.
- **Legend:** solid = CPU 占用 / GPU 利用率; dashed = 内存 / 显存.

### Login dialog
- **Shape:** 420px square panel, 1px steel border, dark backdrop, 30px 90px neutral lift.
- **Controls:** 38px inputs on #0f1011, 1px hairline, amber focus ring; 11px uppercase labels; amber primary button.
- **Reason copy:** the subtitle changes with the trigger — read access says the Server requires a login; opening it from server onboarding says an application admin role is required and explicitly not an OS administrator.
- **Close:** drawn 15px SVG cross at 1.8px stroke, not a text glyph.

### Navigation
- **Style:** two text tabs in the top bar, 11px uppercase label, 1.8 keycap hint, 2px amber underline on the active tab.
- **Keyboard:** arrow keys move between tabs; 1/2 are global shortcuts; focus is always a 2px amber ring.

## Do's and Don'ts

### Do:
- **Do** keep every column fixed and every value in place; update the cell, never reflow the board.
- **Do** use amber for one selected row, one selected GPU, and state; keep the rest charcoal and ivory.
- **Do** keep the board inside the steel case and the flap cells inset; the material is the identity.
- **Do** set Latin and digits in Barlow Condensed with `tnum`; let Chinese fall through to the CJK face.
- **Do** reserve the flip for the focused row and selected GPU; respect `prefers-reduced-motion`.
- **Do** use divider lists for mobile process rows and wrap the full command.
- **Do** keep a pending onboarding row quieter than a live row, and collapse the eight GPU headers to one when nothing in the cluster reports a GPU.

### Don't:
- **Don't** turn the board into a rounded card grid with a cyan accent.
- **Don't** add a zero-offset glow to a lamp, a selected cell, or a button.
- **Don't** animate every value on a 3-second refresh; the board would become noise.
- **Don't** use a system display face for the board's Latin lettering; the condensed signage face is the voice.
- **Don't** nest cards inside the detail pane; use one pane and divider rows.
- **Don't** use a hero-metric strip, a kicker above a heading, gradient text, or a decorative side stripe.
