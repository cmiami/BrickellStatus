# Working in this repository

BrickellStatus is a local Rust runtime with a Svelte 5 console and Tauri shell.
Read `CONTRIBUTING.md` for product contracts and `DESIGN.md` before UI changes.

## Where changes belong

- `crates/collectors`: provider adapters and captured/synthetic fixtures.
- `crates/policy`: bridge confidence, operating schedule, and priority rules.
- `crates/runtime`: preferences, collection scheduling, and snapshot assembly;
  channel rules live in `src/engine/channel_rules.rs`.
- `crates/storage`: SQLite schema, migrations, retention, and outbox transactions.
- `crates/eink` and `crates/delivery`: rendering, hardware protocols, messaging.
- `apps/console/src/lib/{tow-inference,route-hulls}.ts`: portable tow detection and
  display spacing, mirrored in MiamiBridges `src/shared`. Keep both copies identical;
  use `towGroups.test.ts` and `RiverLine.test.ts` for native regressions.
- `apps/console`: UI. Use `preferencesEditor.svelte.ts` for autosaving forms and
  `state.ts` for shared polling; do not create competing refresh/save loops.
- `apps/desktop/src-tauri`: native commands, secrets, firmware, and output workers.
- `scripts/calibrate_bridge.py`: read-only forecast scoring against observed history.
- `scripts/audit_bridge_model.py` and `audit_ais_timing.py`: chronological model
  evaluation; see `docs/MODEL_AUDIT.md` before changing prediction mathematics.
- `firmware/panel`: physical panel firmware; follow its README for all board builds.

## Verification

Install dependencies with `npm --prefix apps/console ci`. Rust and npm versions
are pinned in `rust-toolchain.toml` and `apps/console/package.json`.

Run from any working directory using the script's absolute path, or from the
repository root:

```sh
node scripts/verify.mjs console  # Svelte/TS check, UI tests, production build
node scripts/verify.mjs core     # Python tests, Rust fmt/test/clippy; no Tauri assets
node scripts/verify.mjs all      # complete checks; prepares generated Tauri inputs
```

During iteration, narrow the check to the affected behavior:

```sh
cargo test -p brickellstatus-collectors open_meteo
cargo test -p brickellstatus-runtime provider_weather_periods
npm --prefix apps/console test -- src/lib/state.test.ts
python3 -m unittest discover -s scripts -p 'test_*.py'
```

Direct desktop/workspace Cargo commands need `apps/console/build` and generated
`apps/desktop/src-tauri/resources/{licenses,firmware}` first. Generate them with
`npm --prefix apps/console run desktop:prepare`, or use `verify.mjs all`.
The verification script does not build firmware or exercise physical hardware;
cached firmware with a different source build id is omitted from the local bundle.
Run the complete checks once before final review; avoid parallel Cargo commands
that compete for the same build lock.

## Evidence and scope

- Missing or stale evidence must stay unknown. Never manufacture freshness,
  historical speed/course, confirmed bridge continuity, or observed outcomes.
- Open-Meteo hourly/minutely accumulation timestamps end the preceding period.
  Keep `starts_at`/`ends_at` normalized at the collector boundary.
- Cover fixes with behavior tests that fail for the original defect. Remove
  tests of dead features and literal setup; keep fixtures out of live delivery.
- Use synthetic databases for tests/benchmarks. Never commit local histories,
  credentials, generated bundles, or private settings.
- For an explicitly requested model audit, take a read-only SQLite backup of
  local history first. Purge overlapping label horizons at chronological splits;
  weight AIS examples by passage and bootstrap by opening, not individual fixes.
  Report alert precision/recall and ETA coverage separately. Do not call a
  crossing during an opening proof that the vessel required the lift.
- Exact prediction replays retain material changes for 30 days. Export them with
  `scripts/export_bridge_replay.py`, then pipe the JSONL into
  `cargo run -p brickellstatus-policy --example replay_bridge --locked`.
- Avoid dependency upgrades, release builds, hardware flashing, live messaging,
  and calibration-weight changes unless they are part of the requested work.
