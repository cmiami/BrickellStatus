# AIS Discovery — Miami River / Brickell

Live discovery performed 2026-08-17 (Monday, 13:07–13:50 ET) against
`wss://stream.aisstream.io/v0/stream` with the development key, from this
machine. Raw captures and analysis scripts are session artifacts; the numbers
below are measured, not quoted.

## 1. The stream works again

The README's known-broken note (subscription accepted, no vessel detail,
[upstream #30](https://github.com/aisstream/aisstream/issues/30)) did not
reproduce. A wide-box subscription over Miami returned its first vessel message
within 4–7 seconds of subscribing, and a 20-minute capture collected 305
messages from 91 distinct vessels with zero stream errors. The upstream issue
remains open, so treat availability as intermittent-by-provider, but the
adapter and key are fine: the collector's own health/degraded plumbing is the
right response when the firehose goes quiet, and no code change is needed to
"fix" the connection itself.

Also verified: **concurrent connections on one key coexist** (three sockets at
once, all healthy, all receiving). Discovery tooling and the running app do not
fight over the key. AISStream's documented limits that do bind: at most one
subscription *update* per second, and the subscription must arrive within 3
seconds of connecting.

## 2. What the provider actually sends

Push WebSocket, **binary frames** carrying JSON (the Rust adapter already
handles both frame kinds). Each message wraps one AIS sentence:

| Observed type | Rate (wide box) | Payload worth keeping |
|---|---|---|
| `PositionReport` (Class A) | 7.3/min | Lat/Lon, Sog, Cog, TrueHeading (511 = n/a), NavigationalStatus, RateOfTurn |
| `StandardClassBPositionReport` | 3.6/min | Lat/Lon, Sog, Cog — yachts and small craft |
| `StaticDataReport` (msg 24 A/B) | 2.1/min | Class B name, callsign, ship type, dimensions |
| `ShipStaticData` (msg 5) | 1.7/min | Class A name, IMO, callsign, **Type**, **Dimension A–D**, **MaximumStaticDraught**, Destination, ETA |
| `AidsToNavigationReport` | 0.2/min | Buoys/markers — ignore |
| `UnknownMessage` | 0.1/min | Empty body, MetaData only — ignore |

Every message carries `MetaData { MMSI, ShipName, latitude, longitude,
time_utc }` — AISStream enriches position reports with its own static cache, so
names usually arrive without waiting for a static message.

**Latency (measured, n=305):** `time_utc` → local receipt p50 **2.0 s**, p90
2.2 s, p99 2.4 s. Effectively real time; the 30-minute problem is entirely
about geometry and inference, not transport.

**Effective per-vessel cadence (measured):** moving Class A ≈ **60 s** (nominal
2–10 s is not what arrives; terrestrial coverage/dedup flattens it), moving
Class B ≈ 2–12 min and patchy, moored anything ≈ 3–12 min. Design consequence:
the existing 6-minute `max_report_age` is right, and nothing in the ladder may
assume sub-minute updates. A countdown below T-5 must interpolate along the
last known track rather than expect a fresh fix.

**Query model:** there is no polling. One long-lived subscription per running
app; the engine reads the adapter's snapshot each cycle (currently every
engine pass; FL511 polls at 15 s alongside it).

## 3. What the data can and cannot say about a vessel

- **Size: yes.** Dimensions A–D give length/beam (observed: tug BAYOU TECHE
  107 m × 23 m), plus water draught (3.3 m) and ship type.
- **Height: no.** Standard AIS has no air-draft field (PRODUCT.md already says
  so). Proxies, in order of value:
  1. **Learned per-vessel history** — a hull that has only ever crossed while
     Brickell was up *is* taller than the closed clearance; one that crosses
     while down is a proven fits-under. This measures exactly the quantity we
     care about, per MMSI, forever.
  2. **Ship type 36 (sailing)** — mast vs ~7 m closed clearance: near-certain
     opener when underway on the river.
  3. **Length** as weak prior (long ⇒ tall superstructure or tug-with-tow).
- **Draught as an exclusion filter:** the river's maintained depth is ~4.6 m.
  Cruise ships (8–9 m draught, moored at PortMiami all through the capture)
  are physically incapable of entering the river; any draught ≳ 4.5 m can be
  dropped from bridge reasoning outright.
- **NavigationalStatus is decorative.** The outbound tugs broadcast status 5
  ("moored") while making 4–5 kn; MSRC STEADFAST broadcasts 15 (undefined)
  while moored. Moored-vs-underway must be *behavioral*: position variance
  under ~50 m across 10+ minutes = moored, regardless of claim.
- **Trash exists.** Live MMSIs 123456789, 444444444, and a 99-prefix ATON
  appeared within 20 minutes. Range checks are insufficient; position-plausible
  movement is the real gate.

## 4. Noise census (why the corridor matters)

Of 221 position reports in 20 minutes over a 16 × 20 km box: **1** was on the
river, 2 near the bridge (a *moored* pleasure craft 65 m from the bascule —
IRON GRYPHON, the perfect false-positive trap), ~70 % moored port/marina
traffic (cruise ships, Moran tugs, dinner boats), the rest bay transits. A
research vessel rounding Claughton Island briefly pointed straight at the
river mouth at 8 kn — course-toward-bridge heuristics false-positive on
exactly this; corridor membership kills it.

The signal-to-noise choice is therefore not the radius knob (2–30 km around
the bridge) but **corridor membership**: on-river or approach-fan, moving,
river-plausible. The production subscription now separates prediction geometry
from discovery geometry: six tight corridor boxes drive live intent, while
three bounded outer aprons retain earlier movement without claiming every hull
in the bay is headed to Brickell:

```
lower river    [[25.7660,-80.2020],[25.7760,-80.1840]]   mouth → I-95 (incl. Brickell)
mid river      [[25.7730,-80.2130],[25.7870,-80.2000]]   I-95 → NW 5 St
upper-mid      [[25.7850,-80.2400],[25.8020,-80.2100]]   NW 5 St → NW 22 Ave
upper river    [[25.7990,-80.2600],[25.8100,-80.2380]]   NW 22 Ave → Palmer Lake
north approach [[25.7620,-80.1860],[25.7790,-80.1280]]   ICW + Main Channel to Government Cut
south approach [[25.7440,-80.1900],[25.7720,-80.1740]]   ICW from the Rickenbacker
Cut apron      [[25.7400,-80.1300],[25.7900,-80.0850]]   jetties → near-shore approaches
north ICW      [[25.7750,-80.1700],[25.8250,-80.1300]]   early north-bay approach
south ICW      [[25.7000,-80.1900],[25.7500,-80.1450]]   early south-bay approach
```

Production shape: **one connection, river/approach boxes + the three discovery
aprons**, with two-tier client-side handling — inside the river corridor every
vessel is tracked; outer-apron fixes feed the durable route catalog but still
need to follow a marked channel toward the mouth before they become predictive
evidence (see §6).
(`FiltersShipMMSI` exists, caps at 50 MMSIs, and applies per-subscription —
client-side tiering is strictly more flexible at these message rates.)

## 5. A complete correlated event, observed live

During discovery the exact scenario this feature exists for happened once,
end to end — tracked by AIS and confirmed by a 30-second FL511 poll:

| Time (UTC) | Evidence | Meaning |
|---|---|---|
| 17:13 | AIS: SARA + COSTA V at NW 22 Ave, 4.6 kn, COG ≈ 122° | Outbound convoy detected, s ≈ 4.9 km |
| 17:25:13 | AIS: SARA past NW 17 Ave, s = 4.4 km, 4.5 kn made good | **T-30 rung fires**; projected bridge-up (ETA − 5 min pre-open) ≈ 17:51:36 |
| 17:29–34 | FL511: NW 12 Ave + NW 5 St **UP**, upper spans closed behind | cascade confirms direction + progress |
| 17:42:29 | FL511: W Flagler + SW 1 St **UP** together; AIS: SARA s = 1.8 km | rungs cross-check: geometry says T-11 |
| 17:48:30 | FL511: SW 2 Ave **UP** | T-6 gate, on schedule |
| **17:50:30** | FL511: **Brickell UP** | actual bridge-up — **66 s from the projection made 25 min earlier** |
| 17:52–53 | AIS: all three tugs at S Miami Ave, ~3 kn | crossing during the up interval; ledger rows confirmed |
| 17:57–59 | AIS: tugs past Brickell, accelerating toward the mouth | Brickell still up ≥ 18:00:38 — up-interval ≥ 10 min for a 3-tug tow |

Three tugs, all Class A, all on the watchlist-to-be. A single mid-river fix
plus the constant-speed model predicted bridge-up within ±1 minute at a
25-minute horizon; total tracked warning from first detection was 37 minutes.
Two calibration facts fell out: tows make ~4.5 kn on the river, and tenders
**pre-open ~5 minutes ahead of vessel arrival** — the ladder must count down
to bridge-up, not to vessel-at-bridge.

The same hour also produced both counter-polarities: Brickell opened
**AIS-silently** at 17:31:12 (= 13:31 ET, the half-hour schedule slot — the
case only schedule/slot logic can warn about), and Class B BRIGHT SIDE ran
the river inbound at 6.4 kn under closed bridges — a confirmed **fits-under**
negative example for the ledger.

Ledger seed rows from today: SARA (367705810), COSTA V (371705000), PEPIN
(367705830) — openers; BRIGHT SIDE (338215012) — fits-under.

## 6. The model: river coordinates, not bridge distance

Straight-line distance/bearing to the bridge (the current classifier) breaks
on a river that bends ~90°: mid-river vessels head "away from" the bridge on
half the reaches. Replace it with a **centerline arc-length coordinate**:

- Fixed polyline through the FL511 bridge coordinates (surveyed, already in
  `fl511.rs`) plus mouth and upper-river anchors — the observed convoy track
  validates the line through the middle reaches.
- Project each fix to the centerline → `s` (meters along river, 0 at
  Brickell, positive upriver, negative toward the bay) and `d` (offset).
  `|d|` ≲ 120 m means on-river.
- Along-river speed `v_s = Δs/Δt` across successive fixes. Robust where
  course-vs-bearing math is not; sign gives direction (outbound = s falling).
- **ETA = s / v_s**, clamped by the legal schedule (below).

Physical rungs, at the observed 4.3 kn (bounds at 3.5 / 5.0 kn):

| Gate | s (m) | ETA @4.3 kn | slow/fast |
|---|---|---|---|
| NW 27 Ave | 5 849 | T-44 | T-54 / T-38 |
| NW 22 Ave | 4 883 | T-37 | T-45 / T-32 |
| **NW 17 Ave** | 4 002 | **T-30** | T-37 / T-26 |
| NW 12 Ave | 3 099 | T-23 | T-29 / T-20 |
| NW 5 St | 2 185 | T-16 | T-20 / T-14 |
| W Flagler | 1 459 | T-11 | T-14 / T-9 |
| SW 1 St | 1 312 | T-10 | T-12 / T-9 |
| SW 2 Ave | 761 | T-6 | T-7 / T-5 |
| S Miami Ave (unpublished) | 384 | T-3 | T-4 / T-2 |
| Brickell | 0 | — | — |

Each FL511 upstream opening confirms the AIS-derived `s` at a known gate; the
two evidence streams cross-check each other rung by rung.

**Inbound: channel branches, not a radius.** An inbound ship does not
approach the bridge radially — it steams westbound through the marked,
dredged entrance channel into the mouth. So the seaward side is modeled
exactly like the river: **two approach polylines** continuing the same
`s` coordinate negative past the mouth (mouth = s −530 m), one per real
route:

- **North branch** — Government Cut jetties → Main Channel / Fisherman's
  Channel along Dodge Island → ICW west leg past Bayfront → mouth.
  Roughly 5 km of marked channel: entering at the Cut ≈ **T-33 @ 5 kn**
  (river leg included); the Dodge Island west-end turn ≈ T-10.
- **South branch** — ICW from the Rickenbacker (≈ 3.2 km) → mouth,
  entering ≈ **T-25 @ 5 kn**.

Intent detection falls out of the geometry: a vessel whose fixes track the
branch polyline (|d| small) with `s` falling toward the mouth is *following
the markers in*; a bay transit crossing the channel at an angle never
accumulates along-channel progress. The shared ICW segment right off the
mouth still carries through-traffic, so inbound confidence rises as branch
progress accumulates and hardens when the vessel commits to the mouth turn —
plus instantly for watchlist/sailing vessels or a matching pilots booking.
Branch waypoints should be traced once from the NOAA ENC harbor chart and
then *calibrated from observed tracks* — today's capture already surveyed the
ICW leg off the mouth (WALTON SMITH's fixes run it at −80.1825) and every
recorded inbound transit refines the line.

## 7. The vessel ledger (per-MMSI opening propensity)

The policy layer prices this in
(`BridgeObservation::AisTrack.opening_propensity`, factor
`0.50 + 0.50·score`, unknown = 0.75), and the runtime now supplies the
Beta-smoothed per-MMSI result from durable history:

- **`ais_transits`** permanently retains each interpolated bridge-line
  crossing: MMSI, direction, crossing time, speed, outcome and source session.
  A plausible sign change can cross from farther than 600 m between sparse
  Class B reports; it is no longer discarded merely because both endpoints
  were not inside an arbitrary bridge bubble.
- **`ais_vessel_ledger`** is the catalog: name, friendly class, call sign, IMO,
  latest destination, length, beam, draught, first/last seen, opening count and
  fits-under count. Bare position reports never blank identity learned from a
  later or earlier static packet.
- **`ais_track_fixes`** retains one fix per vessel per 30 seconds for one year.
  Once `transits_opened > 0`, that MMSI is exempt from pruning, so every
  observed movement of a known opener remains available to fit its habitual
  corridor and timing.
- **Correlation** only labels a crossing when successful FL511 readings prove
  bridge state across the required before/after window. Restarts and collection
  gaps split intervals; an open-ended stale row is never extended to the
  present by assumption. Propensity is the Beta-smoothed opened share.
- **Moored→underway transition** on a ledger vessel whose propensity is high
  is itself evidence — for an upper-river shipyard departure it is the
  earliest signal that exists, ahead of any geometry.

The pre-bundle-rename database is merged idempotently at startup for its AIS
catalog, crossings, fixes and pilots-board movements. Its bridge intervals are
deliberately not imported because they predate successful-reading continuity.
The runtime also stores a compact, model-versioned forecast sample each minute
for two years, so the catalog can be evaluated against false alerts, misses,
warning lead and ETA coverage instead of tuned from anecdotes.

Air draft never appears in AIS; the ledger *is* the height sensor.

## 8. Pilots board fusion

`bbpilots` bookings publish vessel names and boarding times; the collector
turns them into bridge ETAs with admitted placeholder offsets (60/20 min).
Fusing per event:

1. Match board name ↔ ledger name/MMSI (exact-ish uppercase match; tugs
   reappear constantly, so the ledger converges fast).
2. A matched booking pre-arms tracking: the vessel's AIS track supersedes the
   placeholder offset the moment it starts moving — schedule says *intent*,
   AIS says *where*.
3. Booked transits are 33 CFR 117.261-exempt tows — the ladder must not clamp
   them to blackout ends.

## 9. The warning ladder

A staged countdown — `T-30 → T-20 → T-15 → T-10 → T-5 → T-4 → T-3 →
IMMINENT` — published only while its confidence gate holds:

- **Drivers**, best available per cycle: outbound AIS `s/v_s` (tightest);
  FL511 cascade gates (independent confirmation, also works for AIS-silent
  vessels); inbound channel-branch progress (§6); matched pilots bookings
  (wide until the vessel moves); moored→underway ledger transitions (arms the
  ladder early at low precision).
- **Confidence gating:** a rung is announced only when the ETA interval's
  upper and lower bounds both sit inside the rung's window and the driving
  evidence is fresh; multiple independent drivers tighten the interval
  (existing corroboration machinery). "High confidence at T-30" is honestly
  achievable for: river-borne outbound traffic, booked tows, and known
  openers underway in the bay. It is *not* achievable for AIS-silent
  sailboats — the schedule/slot logic remains their only handle, and the
  ladder must not pretend otherwise.
- **Schedule clamp:** when the geometric ETA lands inside a Brickell blackout
  and the vessel is not exempt, the countdown re-targets the blackout end
  (the schedule crate already computes next-valid-slot).
- **Hysteresis:** rungs advance forward freely, regress only on sustained
  contrary evidence (vessel stops/turns for > 2 fixes), and expire with their
  evidence — never silently hold a stale countdown (freshness doctrine).
- **Cadence realism:** at ≈ 60 s Class A cadence, rungs down to T-5 are
  data-driven; below that the display interpolates along the last track while
  FL511 at 15 s carries confirmation to IMMINENT/OPEN. Class B (most yachts
  and sailboats) arrives at 2–12 min cadence — expect their ladders to begin
  at T-10/T-20 unless the ledger spotted them in the bay first.
- **Vessel identity on every surface.** When a high-confidence opener drives
  the ladder, the app and the e-ink panel name it: vessel name (present in
  nearly every message via `MetaData.ShipName`), a friendly class word
  derived from AIS ship type + dimensions (`sailing`, `yacht`, `tug + tow`,
  `barge`, `cargo`, `passenger` — type 36 = sailing, 37 = pleasure, 31/32/52
  = tow/tug, 60s = passenger, 70s = cargo, 80s = tanker), length when known,
  and its rung — e.g. `SARA · tug + tow · T-15`. When several openers are
  inbound at once (convoys, queued yachts), the surfaces show the queue —
  lead vessel named with its rung, plus `+2 queued` — rather than only the
  soonest. Identity is display metadata, not evidence: an unnamed track
  scores exactly like a named one. Brickell remains the only *target*;
  upstream bridges stay indicators (existing `target`/`upstream` relations),
  and vessel cards always describe progress toward **Brickell**.

## 10. Implementation map

| Piece | Where | Change |
|---|---|---|
| Centerline + (s, d) projection | `crates/collectors/src/ais_stream.rs` (new `river.rs` module) | replace bearing/miss-distance classification; keep the secrecy/bounding plumbing as is |
| Corridor subscription | `AisStreamSubscription` | multi-box corridor + bay fan instead of one square; keep 2–30 km validation per box |
| Behavioral moored detection | collector track state | variance window over history points (already retained 1 h / 30 s buckets) |
| Expose > 1 track | collector (`MAX_EXPOSED_TRACKS`) | ladder + queue display need the top few openers (per direction), not global top-1 |
| Vessel class labels | collector | ship type + dims → friendly class word (`sailing`, `yacht`, `tug + tow`, `cargo`…) carried as item attributes |
| `ais_transits`, `ais_vessel_ledger`, `ais_track_fixes` | `crates/storage/schema.sql` | permanent outcomes/catalog; one-year general tracks and permanent known-opener tracks |
| `opening_propensity` wiring | `crates/runtime/src/engine.rs` `bridge_fact()` | Beta-smoothed ledger lookup by MMSI |
| Continuous outcome labels | storage bridge intervals + runtime successful-poll gate | split restarts/gaps; resolve a transit only against explicitly confirmed coverage |
| Forecast history | `bridge_forecast_samples`, `scripts/calibrate_bridge.py` | model-versioned episode precision/recall, false alerts, lead and ETA coverage |
| BBP ↔ AIS matching | runtime engine | name match at evidence-assembly time; booked+moving overrides placeholder offset |
| Ladder stage + clamp | `crates/policy/src/bridge.rs` | derive rung from fused ETA interval; blackout clamp via existing schedule; add rung to `BridgePrediction` |
| Surfaces | dto/eink/console | show rung + driving evidence sentence + vessel card (name · class · length · rung, `+N queued`) |

Sizing note: everything upstream of the policy change is additive and
independently testable; the fixture set should grow a corridor transit
(captured live today) and a fits-under pass so the correlation logic has both
polarities from day one.
