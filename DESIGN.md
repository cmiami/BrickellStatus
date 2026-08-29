---
name: BrickellStatus
description: A weatherproof dispatch instrument for signals worth acting on.
colors:
  marine-ink: "#0F2A44"
  channel-blue: "#174F78"
  steel-rule: "#5A6B7C"
  cool-paper: "#DDE3E8"
  frost-sheet: "#F4F7F9"
  graphite: "#111418"
  muted-graphite: "#46515B"
  safety-amber: "#F2A900"
  amber-ink: "#765300"
  amber-sheet: "#FFE2A0"
  success: "#176B47"
  success-sheet: "#DCEEE5"
  danger: "#8D2D28"
  corridor-violet: "#5B3E8C"
  corridor-wash: "rgba(110, 79, 163, 0.14)"
  corridor-rule: "rgba(91, 62, 140, 0.42)"
  corridor-sheet: "#ECE6F5"
  nav-muted: "#CBD8E2"
  nav-subdued: "#B9CAD7"
  signal-green: "#4AC18A"
  eink-ink: "#050505"
  eink-paper: "#F9F9F5"
  device-frame: "#20262B"
  white: "#FFFFFF"
typography:
  display:
    fontFamily: "Barlow Condensed Variable, Barlow Condensed, Arial Narrow, sans-serif"
    fontSize: "clamp(3.5rem, 6.5vw, 6rem)"
    fontWeight: 700
    lineHeight: 0.78
    letterSpacing: "-0.025em"
  headline:
    fontFamily: "Barlow Condensed Variable, Barlow Condensed, Arial Narrow, sans-serif"
    fontSize: "clamp(2rem, 5vw, 4.5rem)"
    fontWeight: 700
    lineHeight: 0.92
    letterSpacing: "-0.012em"
  body:
    fontFamily: "Public Sans Variable, Public Sans, system-ui, sans-serif"
    fontSize: "1rem"
    fontWeight: 450
    lineHeight: 1.5
  label:
    fontFamily: "Barlow Condensed Variable, Barlow Condensed, Arial Narrow, sans-serif"
    fontSize: "0.8125rem"
    fontWeight: 600
    lineHeight: 1
    letterSpacing: "0.09em"
  body-small:
    fontFamily: "Public Sans Variable, Public Sans, system-ui, sans-serif"
    fontSize: "0.875rem"
    fontWeight: 450
    lineHeight: 1.5
  caption:
    fontFamily: "Public Sans Variable, Public Sans, system-ui, sans-serif"
    fontSize: "0.75rem"
    fontWeight: 500
    lineHeight: 1.4
  micro:
    fontFamily: "Barlow Condensed Variable, Barlow Condensed, Arial Narrow, sans-serif"
    fontSize: "0.6875rem"
    fontWeight: 600
    lineHeight: 1.1
    letterSpacing: "0.06em"
  title:
    fontFamily: "Barlow Condensed Variable, Barlow Condensed, Arial Narrow, sans-serif"
    fontSize: "1.25rem"
    fontWeight: 650
    lineHeight: 1.1
  section:
    fontFamily: "Barlow Condensed Variable, Barlow Condensed, Arial Narrow, sans-serif"
    fontSize: "1.75rem"
    fontWeight: 700
    lineHeight: 0.95
  status:
    fontFamily: "Barlow Condensed Variable, Barlow Condensed, Arial Narrow, sans-serif"
    fontSize: "clamp(2rem, 3vw, 3.1rem)"
    fontWeight: 700
    lineHeight: 0.88
  display-compact:
    fontFamily: "Barlow Condensed Variable, Barlow Condensed, Arial Narrow, sans-serif"
    fontSize: "clamp(2.7rem, 5.5vw, 5.4rem)"
    fontWeight: 700
    lineHeight: 0.82
  device-label:
    fontFamily: "Barlow Condensed Variable, Barlow Condensed, Arial Narrow, sans-serif"
    fontSize: "clamp(0.55rem, 1.25vw, 0.8125rem)"
    fontWeight: 700
    lineHeight: 1
  device-micro:
    fontFamily: "Public Sans Variable, Public Sans, system-ui, sans-serif"
    fontSize: "clamp(0.45rem, 1vw, 0.6875rem)"
    fontWeight: 600
    lineHeight: 1.1
  device-state:
    fontFamily: "Barlow Condensed Variable, Barlow Condensed, Arial Narrow, sans-serif"
    fontSize: "clamp(1.5rem, 5vw, 3.6rem)"
    fontWeight: 700
    lineHeight: 0.84
  device-eta:
    fontFamily: "Barlow Condensed Variable, Barlow Condensed, Arial Narrow, sans-serif"
    fontSize: "clamp(0.75rem, 2vw, 1.4rem)"
    fontWeight: 700
    lineHeight: 1
rounded:
  registration: "2px"
  control: "4px"
spacing:
  hairline: "1px"
  xs: "4px"
  sm: "8px"
  md: "16px"
  lg: "24px"
  xl: "40px"
  section: "64px"
components:
  button-primary:
    backgroundColor: "{colors.marine-ink}"
    textColor: "{colors.white}"
    typography: "{typography.label}"
    rounded: "{rounded.registration}"
    padding: "14px 18px"
  button-secondary:
    backgroundColor: "{colors.frost-sheet}"
    textColor: "{colors.graphite}"
    typography: "{typography.label}"
    rounded: "{rounded.registration}"
    padding: "13px 17px"
  field:
    backgroundColor: "{colors.frost-sheet}"
    textColor: "{colors.graphite}"
    rounded: "{rounded.registration}"
    padding: "12px 14px"
  dispatch-strip:
    backgroundColor: "{colors.frost-sheet}"
    textColor: "{colors.graphite}"
    rounded: "{rounded.registration}"
    padding: "12px 14px"
---

# Design System: BrickellStatus

## Overview

**Creative North Star: "The Working BrickellStatus"**

BrickellStatus turns an operator's weatherproof log sheet into a precise personal signal instrument. It feels written for daylight, motion, and consequential glances: cool paper, marine ink, clipped evidence strips, registration marks, and one live time rail. The system is tactile without cosplay and civic without becoming institutional.

The visual hierarchy always answers three questions in order: what is happening, when might it matter, and why does the system believe it. Density belongs in history and configuration surfaces; the current decision remains large, plain, and calm. The web interface and monochrome e-paper layout share the same typography, line hierarchy, state words, and strip grammar rather than imitating one another pixel for pixel. Visible copy uses ordinary words; technical implementation terms belong in developer logs and exported details, not primary headings or actions.

**Key Characteristics:**

- Weatherproof cool-paper fields, never warm stationery.
- Condensed status lettering paired with highly legible civic body text.
- Short, common labels that remain readable at a glance and never split a status word.
- Evidence physically registers against a live time rail.
- Safety amber is a scarce interrupt marker, not ambient decoration.
- Square, clipped, ruled components with minimal optical depth.

## Colors

The palette is a restrained daylight instrument: cold paper and graphite, with marine ink for structure and one safety amber for material attention.

### Primary

- **Marine Ink** (`#0F2A44`): Navigation, primary action, stamps, and the strongest structural fields.
- **Channel Blue** (`#174F78`): Selected channels, live links, and secondary registration marks.

### Secondary

- **Safety Amber** (`#F2A900`): Reserved for interrupt-eligible signals, warning edges, and the single item requiring action now.

### Tertiary

- **Corridor Violet** (`#5B3E8C`): The live Miami River route, AIS vessel courses, and vessel marks. It shows where vessel data came from, never urgency or severity.

### Neutral

- **Graphite** (`#111418`): Primary copy and open/critical inverse fields.
- **Muted Graphite** (`#46515B`): Metadata whose freshness remains acceptable.
- **Steel Rule** (`#5A6B7C`): Dividers, inactive registration, and technical annotation.
- **Cool Paper** (`#DDE3E8`): App canvas and recessed working floor.
- **Frost Sheet** (`#F4F7F9`): Evidence strips, forms, and log sheets.
- **White** (`#FFFFFF`): Text on marine or graphite fields and clean high-contrast controls.

**The Amber Ration Rule.** Safety amber may identify one interrupt or one action cluster per viewport; if everything is marked, nothing is urgent.

## Typography

**Display Font:** Barlow Condensed Variable (with Arial Narrow fallback)

**Body Font:** Public Sans Variable (with system-ui fallback)

**Label Font:** Barlow Condensed Variable

**Character:** Barlow Condensed brings the tall, economical authority of transit and marine instrumentation without pretending to be a vintage stencil. Public Sans keeps explanations, configuration, and accessibility copy calm and unmistakably contemporary.

### Hierarchy

- **Display** (700, `clamp(3.5rem, 6.5vw, 6rem)`, `0.78`): One current state and its ETA; never marketing copy.
- **Headline** (700, `clamp(2rem, 5vw, 4.5rem)`, `0.92`): Channel titles, alert titles, and major section conclusions.
- **Title** (650, `1.25rem`, `1.1`): Strip titles and configuration group names.
- **Status** (700, `clamp(2rem, 3vw, 3.1rem)`, `0.88`): System and hardware states that must remain whole at ordinary widths.
- **Body** (450, `1rem`, `1.5`): Explanations and source details, limited to about 68 characters per line.
- **Body Small** (450, `0.875rem`, `1.5`): Dense configuration explanations that still need ordinary sentence legibility.
- **Label** (600, `0.8125rem`, `0.09em`, uppercase): Source, time, state, and control labels.
- **Caption** (500, `0.75rem`, `1.4`): Freshness, source age, and delivery metadata.
- **Micro** (600, `0.6875rem`, `0.06em`, uppercase): Column headers and compact device/register annotations only.

**The Instrument Voice Rule.** Condensed uppercase states facts; Public Sans sentences explain them. Never set paragraphs in the display face.

**The Plain Status Rule.** Name the bridge before its road consequence: `BRIDGE OPEN · TRAFFIC BLOCKED`, `BRIDGE CLOSED · TRAFFIC FLOWING`, or `NO READING · TRAFFIC STATUS UNKNOWN`; system `WORKING`, `NEEDS ATTENTION`, or `OFFLINE`; actions such as `REFRESH`, `CONNECT`, and `DISCONNECT`. Never use `ROAD OPEN` for a closed bridge: it makes the road and the movable span sound like the same object. Never expose internal terms such as poll, collector circuit, engine verdict, dispatch, or route as the primary label when a common word says the same thing.

## Layout

The primary surface is a working log organized around an off-center vertical time rail. The current decision owns the broad field; evidence strips dock to the rail; source health and controls occupy a narrower ledger column. Desktop uses a 12-column grid with a 3/5/4 working split. Compact widths preserve the decision first, collapse evidence beneath it in time order, and convert the rail from vertical to horizontal only when the remaining width would make labels unreadable.

Configuration surfaces retain the ruled ledger: channel index at left, selected channel form in the central work area, and a preview or summary at right. Spacing follows a 4px base with 16px control rhythm, 24–40px group rhythm, and 64px or more between major sections. Long status words stay intact; reflow the surrounding grid or reduce within the documented Status scale before allowing a mid-word break.

## Elevation & Depth

The canvas is flat and ruled. Depth appears only where a dispatch strip is physically "clipped" above the log: a narrow graphite contact shadow and a 1px upper highlight. Dialogs use an opaque frost sheet over a dimmed graphite scrim; there is no glass, blur, glow, or ambient card shadow.

**The Working Surface Rule.** Permanent structure is flat. Only movable evidence and temporary overlays may lift from the sheet.

## Shapes

Corners stay square or receive a 2px registration radius; form controls may use 4px to preserve comfortable focus rings. Evidence strips use a clipped left registration notch or metal-pin mark, never rounded pills. State is also expressed through border form: solid for live, dashed for pending, shortened rule for stale, and struck rule for disabled.

## Components

### Buttons

- **Shape:** Compact rectangle with 2px registration corners, never a capsule.
- **Primary:** Marine field, white label, 14px × 18px padding; optional arrow or registration mark at the far edge.
- **Hover / Focus:** Channel-blue shift on hover; 2px graphite outer focus rule with 2px offset. Pressing moves the label down by 1px rather than adding glow.
- **Secondary:** Frost field with graphite rule and graphite text.

### Chips

- **Style:** Use only for compact source or destination tokens. They are squared tags with a left state rule, not decorative pills.
- **State:** Selected tokens fill marine; interrupt-eligible tokens add one amber left edge plus a written `INTERRUPTS` label.

### Cards / Containers

- **Corner Style:** 2px maximum.
- **Background:** Frost sheets on cool paper; graphite inversion only for a confirmed bridge-up reading or critical official alerts.
- **Shadow Strategy:** No resting card shadows. Movable dispatch strips may use the contact shadow from Elevation.
- **Border:** 1px steel rule, with stronger graphite registration at the owning edge.
- **Internal Padding:** 16px compact, 24px standard, 40px for the current-decision field.

### Inputs / Fields

- **Style:** Frost background, steel underline or full 1px rule, 2px corners, visible units beside numeric values.
- **Focus:** Graphite 2px focus rule plus a small channel-blue registration tick.
- **Error / Disabled:** Errors use an amber left rule plus explicit text; disabled fields are struck by a short diagonal rule and retain readable copy.

### Navigation

Navigation reads like the index tabs of a working log. Active destinations receive a marine field or a full-height marine registration rule. Mobile navigation becomes a bottom index rail with text plus simple geometric symbols; selection never depends on an icon alone.

### Dispatch Strip

Every observation and delivered notice uses the same strip anatomy: source mark, plain-language title, observed time, freshness, destination/status, and an expandable evidence note. Solid, dashed, shortened, and struck rules communicate live, pending, stale, and disabled states. An interrupt adds one amber edge; corroboration adds a doubled registration mark.

### Confidence Stamp

Confidence is a rectangular ink stamp containing the numeric score and a word (`LOW`, `MODERATE`, `HIGH`, `CONFIRMED`). It is never a speedometer, radial gauge, or unexplained percentage. The explanation remains adjacent or one action away.

### Map

Location is visual before it is numeric. Place-based settings use the shared MapLibre surface with pan/zoom, search, saved markers, real AIS vessel positions, and one draggable amber candidate pin. Search or one-shot device location stages a pin before saving. Latitude, longitude, time zone, and radius remain under **Advanced coordinates**. Attribution stays visible, and an unavailable map falls back to search and the saved-place list without pretending vessel positions are live.

Map also owns a persistent **Known Openers** catalog. It is durable history, not a filtered copy of the current AIS window, so a vessel remains browsable when it is no longer live or nearby. Each catalog row names the vessel or its MMSI and summarizes its confirmed Brickell opening record; selecting it opens the same vessel detail surface used by live marks, with identity, first and last sighting, confirmed bridge impacts, and recent passage history. Live readings and saved history occupy visibly separate sections so an old opening can never read as a current approach.

### Vessel Marks

Vessels are authored side-profile drawings, not generic map pins. The outer silhouette must identify the class before a user reads its label: a tug has a deep workboat hull and tall pilothouse; cargo has an aft house and stacked container mass; a tanker has a long flush deck and manifold; a sailboat has a curved hull, keel, and separated sloop rig; passenger, fishing, pilot, and yacht profiles each retain their own working anatomy. A missing or unrecognized type uses one complete Miami flybridge yacht. It is never dashed, hollow, ghosted, or marked with a question symbol.

Every profile shares a 44 × 26 drawing register and one waterline, then renders at the real 48–68 px live-view sizes. Recognition comes from bow, stern, sheer, cabin, and three to five class-defining details—not ornamental linework that only survives when zoomed in. Meaningful strokes remain at least 0.9 px at final size, windows remain at least 2 px, and masts or working gear never clip the view box.

The normal two-tone construction is a Corridor Violet hull and working gear, White superstructure, Marine glazing, and a restrained Marine register shadow. A **known opener** keeps that normal violet construction: it is the durable historical fact that Brickell has gone up for that vessel before, not a statement about where it is now. State the history in words as `KNOWN OPENER`; never turn the hull amber merely because it is in the catalog.

Amber Ink is reserved for a **current likely opener**: a live track whose present route and bridge-impact evidence say it is likely to raise Brickell on this passage. Pair amber with the written state `LIKELY TO OPEN BRICKELL`, and remove both as soon as the live judgement clears. A known opener moving away stays violet; a vessel with no recorded opening may become amber when its current evidence supports likely impact. If several vessels contribute to one predicted opening, they form one amber action cluster rather than spending amber on unrelated vessel history.

The Live schematic and its one scrollable right-hand register contain all and only current tracks the engine says will cross Brickell Avenue Bridge. “All” means no arbitrary top-three cap, priority cutoff, `+N` summary, second vessel list, or hidden qualifying vessel; it does not mean every AIS contact in Miami. Moving-away, moored, off-corridor, and no-path traffic belongs on Map and in stored history, not on the Brickell decision surface. An AIS sailing or yacht type may strengthen the opening-impact estimate after a route is committed, but vessel type alone never turns inbound bay traffic into a Brickell passage. The register states the full qualifying count, gives every row a stable two-digit number, names the vessel or falls back to its MMSI, and shows its recognizable type, direction, speed in knots, and Brickell ETA when one is supported. Durable `KNOWN OPENER` history remains separate from current `LIKELY TO OPEN BRICKELL` impact.

Every vessel honestly positioned on the schematic repeats its register number beside a compact detail plate containing its name, direction, knots, and ETA. Place all plates in one global collision pass: like matching-polarity magnets, plates may not cover any route ribbon, Brickell or another bridge, another vessel, or another plate. A leader connects each displaced plate to its exact AIS hull; if no legal plate position exists, the numbered hull and complete register row remain rather than forcing an overlap. Put two unmistakable chevrons ahead of each hull along its actual route tangent—never behind it and never as a line through the vessel. Selecting a register row emphasizes its matching hull and plate without moving the AIS position.

Vessel color never invents a class, confidence level, or broadcast state. Exact diagonal travel belongs to the adjacent course arrow; the illustrated vessel stays upright and mirrors horizontally to face upriver or downriver.

### Channel Status

Every channel has a written `ON`, `OFF`, `UNAVAILABLE`, or `NEEDS ATTENTION` state separate from whether it appears on the display. Turning a channel off stops updates and alerts while leaving its settings editable. Active states use a success registration edge, off uses steel, and faults use the danger edge. Detailed source names and failure reasons remain available on System Health.

### E-Paper Headline Card

A headline card is a compact front page, not a generic alert. Its header names
`NEWS` and urgency; the ruled subject row names the publisher; the display face
gets up to two headline lines; and the lower field gets up to three smaller
lines of synopsis. Freshness remains in the bottom tape. Do not spend the
synopsis field repeating the headline, drawing an action box around a publisher,
or listing “related” stories that cannot be opened from the panel. When a feed
supplies no synopsis, show only truthful item metadata such as byline and
publication age—never manufacture story detail.

### Panel Connection

Panel setup is a three-step procedure—switch it on, find it, and send a test frame. USB and Bluetooth candidates identify the real connection type. A panel becomes ready only after the hardware acknowledges a complete frame. Names and ports are shown when they help identify the device; UUID-like details stay secondary. The same connected, connecting, unavailable, and error words appear in Outputs and the platform tray.

## Do's and Don'ts

### Do:

- **Do** lead with the decision, timing range, and evidence sentence before configuration or history.
- **Do** preserve readable state words and line forms in every monochrome e-paper translation.
- **Do** let evidence strips carry density while the current-decision field keeps substantial empty space.
- **Do** expose source age, offline state, and user routing policy wherever a signal appears.
- **Do** reserve amber for an item that can genuinely interrupt the user's attention.
- **Do** pair `BRIDGE OPEN` with `TRAFFIC BLOCKED`, and `BRIDGE CLOSED` with `TRAFFIC FLOWING`; use `NO READING` with `TRAFFIC STATUS UNKNOWN` when position is unavailable.
- **Do** label an immediate data update `REFRESH`; reserve source and transport jargon for exported details or developer logs.
- **Do** keep side-profile vessel art upright and mirror it left or right; a separate route arrow carries the exact diagonal heading.
- **Do** judge vessel marks unlabeled at 48, 56, and 68 px, including mirrored and expected-opener states.

### Don't:

- **Don't** use generic SaaS card grids, glass panels, glossy gradients, or floating pill badges.
- **Don't** mimic parchment, naval uniforms, rope, rivets, or other literal maritime costume.
- **Don't** show schedule eligibility as a predicted bridge opening.
- **Don't** rely on color, iconography, or a confidence number without plain-language state.
- **Don't** rasterize interface text from the concept board; core UI remains semantic and accessible.
- **Don't** split a status word to preserve a grid; reflow the grid and keep the word intact.
- **Don't** use implementation vocabulary as interface personality. Say channel, alert, message, panel, source, and refresh unless greater precision is genuinely necessary.
- **Don't** rotate side-profile boats as though they were top-down hulls; cabins, masts, and readable details must never turn upside down.
- **Don't** use a ghost outline, dashed hull, question mark, or AIS/broadcast decoration when a vessel type is unknown; draw the generic Miami yacht and keep uncertainty in the adjacent words.
