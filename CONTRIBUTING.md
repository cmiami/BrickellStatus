# Contributing to PuenteGonorrea

Thanks for helping build a calmer signal desk. The bar is simple: preserve the
truth of the evidence, preserve the user's right to silence, and leave the
interface cleaner than you found it.

## Before opening a pull request

1. Open or find an issue for behavior that changes a public contract. Small
   documentation and test corrections can go straight to a pull request.
2. Keep the change focused. Do not mix dependency refreshes, generated assets,
   visual redesign, and collector behavior in one review unless they are
   inseparable.
3. Use synthetic fixtures. Never commit credentials, phone numbers, message
   contents, precise private locations, hardware identifiers, SQLite files, or
   private feed URLs.
4. Explain the user-visible result, failure behavior, and how you verified it.
5. Request review from the code owner. `main` requires an approving review,
   fresh approval after later pushes, passing checks, and resolved discussion.

By contributing, you agree that your contribution is licensed under the
project's **MIT** terms unless you explicitly say otherwise before it is
accepted.

## Development setup

Required versions are intentionally pinned where reproducibility matters:

- Rust `1.97.1` through [`rust-toolchain.toml`](rust-toolchain.toml);
- Node.js 24 or newer;
- npm `11.18.0` exactly;
- Xcode Command Line Tools for a macOS desktop build;
- PlatformIO only for E213 firmware work;
- `brew install nsis llvm` plus `cargo install --locked cargo-xwin` only for
  cross-building the Windows installer (see
  [`docs/WINDOWS_RELEASE.md`](docs/WINDOWS_RELEASE.md)).

```sh
git clone https://github.com/cmiami/PuenteGonorrea.git
cd PuenteGonorrea
npm --prefix apps/console ci
cargo test --workspace
npm --prefix apps/console run check
npm --prefix apps/console test
```

Start the full desktop shell with:

```sh
npm --prefix apps/console run tauri:dev
```

The console requires the native runtime. Browser-only development can inspect
static layout, but live commands fail visibly and never substitute local data.

## Required checks

Run the checks relevant to your change, and run the complete set before asking
for final review:

```sh
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
npm --prefix apps/console run check
npm --prefix apps/console test
npm --prefix apps/console run build
```

For E213 changes, build both supported panel environments listed in
[`firmware/e213/README.md`](firmware/e213/README.md). State whether you tested
on physical hardware; a successful compile is not a physical delivery test.

For macOS release work, follow [`docs/MACOS_RELEASE.md`](docs/MACOS_RELEASE.md)
and include the measured DMG size. For Windows release work, follow
[`docs/WINDOWS_RELEASE.md`](docs/WINDOWS_RELEASE.md) and include the measured
installer size. Release artifacts must remain at or below the repository's
25 MiB limit.

## Product contracts worth defending

- A legal bridge-opening slot is context, never proof of an opening.
- Freshness and availability are explicit; missing data does not become
  `CLEAR`.
- Each channel and each material sub-rule has a real enable/disable gate.
  Disabled collection stops polling, not merely rendering.
- Collection, display presence, interrupt eligibility, and destinations are
  separate decisions.
- Location uses the shared searchable, draggable global map. Raw coordinates
  belong in the advanced escape hatch, and device location is one-shot only
  after an explicit OS permission action.
- Personal forecast heads-ups must never impersonate NWS or another official
  authority.
- WhatsApp sends fail closed without recorded opt-in and use only the official
  Cloud API.
- An e-paper frame is not delivered until the board returns `ACK INK1`.
- Test fixtures never enter the production outbox.

## Interface changes

Tender's Log is a ruled, daylight instrument rather than a generic dashboard.
Read [`PRODUCT.md`](PRODUCT.md) and [`DESIGN.md`](DESIGN.md) before changing its
surface. Preserve keyboard access, visible focus, reduced-motion behavior,
plain-language state labels, and monochrome-safe meaning. Include screenshots
for material UI changes and test long, empty, offline, permission-denied, and
narrow-window states.

## Collectors and integrations

New sources should normalize into the shared observation model rather than
inventing a second alert path. Include captured or synthetic fixtures, timeout
and malformed-data tests, provider attribution, freshness semantics,
deduplication identity, and a documented fail-safe state. Internet-facing
collectors must preserve the repository's SSRF and response-boundary controls.

Do not label an undocumented endpoint as a supported public API. Keep it
behind an adapter, fixture-test the observed schema, and fail visibly when the
shape changes.

## Dependencies

The JavaScript manifest follows npm's `latest` tag, while `.npmrc` rejects
releases younger than 48 hours and the reviewed lockfile pins the exact graph.
Cargo has no meaningful `@latest` literal: Dependabot proposes current
compatible Rust releases after the same two-day cooldown, and `Cargo.lock`
remains reviewable and committed. GitHub Actions version updates use the same
delayed, reviewed pull-request path. [Dependabot cooldowns apply to version
updates, not security updates](https://docs.github.com/en/code-security/reference/supply-chain-security/dependabot-options-reference#cooldown),
but neither kind is exempt from review or required checks here.

Do not enable lifecycle scripts or merge an automated dependency pull request
without inspecting its provenance, changelog, lockfile delta, licenses, audit
result, and test result. There is intentionally no dependency auto-merge.

More detail for the console is in
[`apps/console/DEPENDENCIES.md`](apps/console/DEPENDENCIES.md).

## Pull-request shape

A useful description answers:

- What changes for the user?
- Which source, policy, surface, or delivery contract changes?
- What happens when the network, permission, credential, or device is absent?
- Which automated and physical checks passed?
- Does it change stored preferences, privacy, dependencies, or release size?

The repository policy is documented in
[`docs/BRANCH_PROTECTION.md`](docs/BRANCH_PROTECTION.md). Maintainers create
releases; contributors should not add tags or generated DMGs to a pull request.
