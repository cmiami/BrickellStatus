# Tow inference and positioning audit — 2026-09-04

The local app's read-only SQLite backup contains 109,758 position fixes from
August 18 through September 4, 2026. SARA and PEPIN have AIS working-tug classes;
the algorithm uses those classes and retained motion, never their names.

The previous hosted detector required three minutes of overlapping history and
assigned each companion to just one tug. Both tugs can work the same cargo.
The old display independently advanced each member for up to six minutes, then
replaced the older timestamp. This allowed rendered hulls to overlap and moved
the supposedly reported 3D anchor. The native schematic had no tow inference.

## Changes

- Retain the broad three-minute detector; add a strict sixty-second path with
  independent moving fixes, matching travel, bounded cross-track distance and
  stable separation. One fix, moored traffic, crossing/passing encounters,
  future observations and missing-course opposite directions do not qualify.
  Same-measure alongside tracks use geographic separation because corridor
  bank offsets are unsigned. Sparse astern tracks retain route-based separation;
  a geographic-only interpolation variant lost a sparse-curve regression and
  reduced candidate availability to 58.9%, so it was not selected.
- Several independently supported tugs can share a companion. Time-paired route
  offsets carry the relationship into display placement. They never alter AIS
  fixes, timestamps, speed, distances, bridge predictions or stored history.
- Raw observations feed the 90-second motion controller. Formation layout is a
  separate stage, with stable member ordering and enough space for the actual
  displayed hull, including selection enlargement. Bounded retries make room
  near the river mouth and Brickell without moving a hull across either gate.
- Waiting formations stay together. Unrelated idle vessels retain bank berths.
  Expired members stop driving formation placement. All sprites, including
  unlinked vessels, receive route spacing; dots and connectors retain the
  reported position in the native schematic and hosted 2D/3D views.
- Contract 9 carries optional inferred offsets. Contracts 1–8 omit the new
  field, including on hibernating sockets, for existing strict clients.
- Three-member mobile tow cards scroll their member list inside the available
  space, preserving the footer and all selectable identities.

## Replay results

Candidates are non-tug passages within three minutes of a same-direction SARA
or PEPIN crossing. Replay uses only observations available at each timestamp,
retains fifteen minutes of history, and expires current fixes after six minutes.
Direction is reconstructed from successive same-branch fixes; exact historical
hosted visibility/classifier decisions are not available in the native database.
The evaluation has 28 candidate passages and 32 eligible tug/companion episodes.
Eligibility requires both radios moving/current, the same projected branch and
raw route separation at most 500m during a ±10-minute crossing window. Each
pair episode gets equal weight; dense AIS senders do not dominate availability.

| Measure | Before | After |
| --- | ---: | ---: |
| Pair-weighted link availability | 45.6% | 61.1% |
| Candidate pair episodes linked at least once | 27/32 | 28/32 |
| Mean first-link delay, 27 episodes found by both | 128.2s | 98.6s |
| Overlapping display footprints, 587 pair checks | 361 | 0 |

Native inference uses the same portable implementation. The above before/after
availability numbers compare the hosted detector. They do not represent an
independently measured native production accuracy score. Native behavioral
regressions verify the adapter, multi-tug inference and schematic spacing.

**Co-passages are not confirmed tow labels.** These numbers measure candidate
coverage, detection timing and display defects, not tow precision/recall or
physical coupling accuracy. Some close traffic may be an escort or convoy.
The artwork's spacing does not claim the real towing arrangement. The replay's
footprint circles deliberately bound selected hull geometry; this is a display
collision check, not a navigation or clearance simulation. Inference took about
1–2ms at the 95th percentile on this local replay, not a production benchmark.

## Repeatable verification

1. In BrickellStatus, run `python3 scripts/export_tow_replay.py --db <local-db>
   --out /private/tmp/tow-tracks.json`. It creates a consistent read-only backup
   before export. Keep the export outside both repositories.
2. Preserve the baseline: `git show v0.2.2:worker/tow-groups.ts > /private/tmp/tow-before.ts`.
3. Run `bun scripts/audit-tows.ts /private/tmp/tow-tracks.json /private/tmp/tow-before.ts`.
4. Run `bun run test -- tests/worker/tow-groups.test.ts tests/ui/vessel-formation.test.ts
   tests/ui/tow-groups.test.ts tests/ui/corridor.test.ts` and `bun run verify`.
5. The 12/50-vessel deterministic browser scenarios include an asynchronous
   two-tug/cargo formation. Check mobile member scrolling and selected-member
   follow. Browser captures are synthetic desktop/WebKit checks, not physical
   device approval.

`src/shared/tow-inference.ts` and `route-hulls.ts` are mirrored verbatim in
BrickellStatus `apps/console/src/lib`. Native integration lives in `towGroups.ts`
and `riverSchematic.ts`. Change and verify both copies together.
