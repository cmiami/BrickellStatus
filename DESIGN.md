---
name: Tender's Log
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

# Design System: Tender's Log

## Overview

**Creative North Star: "The Working Tender's Log"**

Tender's Log turns an operator's weatherproof log sheet into a precise personal signal instrument. It feels written for daylight, motion, and consequential glances: cool paper, marine ink, clipped evidence strips, registration marks, and one live time rail. The system is tactile without cosplay and civic without becoming institutional.

The visual hierarchy always answers three questions in order: what is happening, when might it matter, and why does the system believe it. Density belongs in the evidence log and configuration surfaces; the current decision remains large, plain, and calm. The web interface and monochrome e-paper layout share the same typography, line hierarchy, state words, and strip grammar rather than imitating one another pixel for pixel.

**Key Characteristics:**

- Weatherproof cool-paper fields, never warm stationery.
- Condensed status lettering paired with highly legible civic body text.
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
- **Body** (450, `1rem`, `1.5`): Explanations and source details, limited to about 68 characters per line.
- **Body Small** (450, `0.875rem`, `1.5`): Dense configuration explanations that still need ordinary sentence legibility.
- **Label** (600, `0.8125rem`, `0.09em`, uppercase): Source, time, state, and control labels.
- **Caption** (500, `0.75rem`, `1.4`): Freshness, source age, and delivery metadata.
- **Micro** (600, `0.6875rem`, `0.06em`, uppercase): Column headers and compact device/register annotations only.

**The Instrument Voice Rule.** Condensed uppercase states facts; Public Sans sentences explain them. Never set paragraphs in the display face.

## Layout

The primary surface is a working log organized around an off-center vertical time rail. The current decision owns the broad field; evidence strips dock to the rail; source health and controls occupy a narrower ledger column. Desktop uses a 12-column grid with a 3/5/4 working split. Compact widths preserve the decision first, collapse evidence beneath it in time order, and convert the rail from vertical to horizontal only when the remaining width would make labels unreadable.

Configuration surfaces retain the ruled ledger: channel index at left, selected channel form in the central work area, delivery policy at right. Spacing follows a 4px base with 16px control rhythm, 24–40px group rhythm, and 64px or more between major log sections.

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
- **Background:** Frost sheets on cool paper; graphite inversion only for a confirmed open bridge or critical official alerts.
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

### Location Desk

Location is a visual act before it is a number. Any configuration that materially depends on place uses the shared global MapLibre surface: world pan/zoom, search, a written active-location register, saved markers, and one draggable amber candidate pin. A search result or one-shot device sample is staged first, tuned on the map, given a human name, assigned explicit collector gates, and only then saved. Latitude, longitude, time zone, and radius remain available under an **Advanced coordinates** disclosure for engineers and recovery. Attribution is always visible; an unavailable WebGL or tile service falls back to search and the saved-area ledger without pretending the map is live.

### Circuit Gate

Every channel and independently runnable sub-rule has a written operational gate. `RUNNING`, `PARKED`, `UNAVAILABLE`, and `NEEDS ATTENTION` are distinct from display presence. Turning a gate off stops its owned polling and evaluation while leaving its settings editable. Active gates use a success registration edge; parked gates use steel; faults use the danger edge. This pattern is shared by bridge evidence adapters, rain, wind, official alerts, hurricanes, news, earthquakes, markets, device routes, and WhatsApp consent.

### Device Proof Desk

Hardware setup is a three-step ruled procedure—wake, discover, prove—not a serial-port form. USB and BLE candidates identify their actual transport; Bluetooth copy describes direct GATT connection rather than promising OS bonding. A route becomes healthy only after a physical `ACK INK1`. Friendly names, ports, and UUID-adjacent details are advanced selectors. The same connected/connecting/unavailable/error vocabulary appears in the Outputs screen and platform tray.

## Do's and Don'ts

### Do:

- **Do** lead with the decision, timing range, and evidence sentence before configuration or history.
- **Do** preserve readable state words and line forms in every monochrome e-paper translation.
- **Do** let evidence strips carry density while the current-decision field keeps substantial empty space.
- **Do** expose source age, offline state, and user routing policy wherever a signal appears.
- **Do** reserve amber for an item that can genuinely interrupt the user's attention.

### Don't:

- **Don't** use generic SaaS card grids, glass panels, glossy gradients, or floating pill badges.
- **Don't** mimic parchment, naval uniforms, rope, rivets, or other literal maritime costume.
- **Don't** show schedule eligibility as a predicted bridge opening.
- **Don't** rely on color, iconography, or a confidence number without plain-language state.
- **Don't** rasterize interface text from the concept board; core UI remains semantic and accessible.
