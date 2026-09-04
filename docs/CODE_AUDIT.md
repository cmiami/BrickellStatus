# Code audit — 2026-09-04

This pass covered the Rust workspace, Svelte console, calibration script,
firmware packaging, and local/CI verification. Findings below have concrete
code changes and regression checks; this is not a certification of every
platform or physical device.

## Correctness fixes

- **Weather periods:** Open-Meteo timestamps end the preceding accumulation
  period. The collector used them as starts, shifting rain and gust warnings
  by 15 or 60 minutes and retaining elapsed periods. Hourly and minutely items
  now carry normalized starts and ends. The hourly request includes the elapsed
  first period when sizing its lookahead. Tests run the provider fixture through
  the runtime rules. Source: [Open-Meteo parameter definitions](https://open-meteo.com/en/docs).
- **Forecast scoring:** incomplete future observation windows no longer become
  negative outcomes. Explicit unknown readings and session boundaries are not
  bridged into continuous coverage. Recall includes openings within the last
  eligible forecast's horizon. The binomial calculation uses log probabilities
  to avoid overflowing on larger histories. Read-only database paths now escape
  URI metacharacters correctly. Model weights were not changed.
- **Historical AIS data:** missing per-fix speed/course remain missing; the
  vessel's latest motion and posture are not copied into old positions.
- **Preference editing:** channels, outputs, and map share one autosave helper.
  It retains edits during an outstanding save, adopts backend normalization,
  merges unrelated store changes, and flushes on navigation. Queued writes
  merge their edits into the latest confirmed settings at execution time,
  preserving changes from the page just left. A failed pin save remains open
  and can be retried.
- **Polling and errors:** periodic snapshot/display reads wait for completion
  before rescheduling. Stopped polls cannot publish results, and old snapshot
  reads cannot overwrite a completed save. Pushed display status survives an
  older pending read. Manual refresh failures now produce a visible notice.
- **History access:** the observer follows the bound scroll container and
  resumes after exhaustion changes. Explicit “Show more” buttons allow access
  to every loaded row without IntersectionObserver.
- **Firmware identity:** cached images must contain the exact current build id
  before packaging. Stale images are omitted, incomplete variants leave no
  partial payload, and a fresh build with missing/mismatched images fails.
  The Rust agreement test no longer silently skips malformed bundles or accepts
  a dirty build merely because it contains the clean revision as a prefix.

## Cleanup and performance

- Deleted the unused 619-line `riverchart.ts` renderer and its 206-line test file.
  The active `riverSchematic.ts` renderer and its behavior tests remain.
- Removed a literal-setup availability test, unused imports/store exports,
  an unused function parameter, and the inert hub's unused async runtime.
- Replaced duplicated autosave implementations with the shared editor.
- Enabled TypeScript unused-local and unused-parameter diagnostics.
- Reused the activity log's date formatter rather than allocating one per row.
- Reused existing AIS projections when persisting fixes.
- Predictor reads fetch only learned outcome counts, without aggregating
  crossing histories or silently limiting learned vessels to 2,000.
- Known-opener aggregation reads only matching hull histories. Recent bridge
  intervals and crossings use timestamp indexes instead of sorting whole tables.

Synthetic SQLite benchmark: 1,000 hulls, 120,000 crossings, 20 new known openers;
in-memory database, median of seven local runs. These measure individual SQL
queries, not end-to-end application latency.

| Query | Before | After |
| --- | ---: | ---: |
| Known openers | 32.698 ms | 1.187 ms |
| Predictor inputs | 37.266 ms | 0.823 ms |

The known-opener result rows matched exactly. `EXPLAIN QUERY PLAN` confirmed
that both recent-history queries no longer need temporary sorting trees.

## Verification workflow

`node scripts/verify.mjs [console|core|all]` runs the relevant checks and stops
on failure. It checks the Node requirement immediately and prepares generated
Tauri inputs for full verification. CI's quality job calls the same command.
`AGENTS.md` maps ownership, targeted checks, and evidence boundaries.

Regression coverage includes slow/failed polls, save races and normalization,
map save retries, history pagination, provider period boundaries, unobserved
forecast outcomes, large binomial samples, unlimited learned hulls, missing
historical motion, and stale/incomplete firmware caches.

Final `verify.mjs all` passed: 590 Rust tests, 162 console tests, 10 Python tests,
Rust formatting, Clippy with warnings denied, Svelte/TypeScript checking, and
the production console build. All four PlatformIO panel environments built
successfully; their packaged images matched the manifest's complete build id.

Verification used Rust 1.97.1, npm 11.18.0, and Node 24.15.0. The machine's
default Node is still 24.12.0; a checksum-verified Node 24.15.0 installation in
`/tmp` was used without changing the global installation. The runner reports
an unsupported Node version before starting checks.

Browser rendering could not be checked because the in-app browser was
unavailable. The design detector ran with its regex fallback; that cannot
verify computed contrast or responsive layout. Its single status-stripe
warning was retained as part of the existing documented visual language.
No physical device or live message delivery was exercised.
